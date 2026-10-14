use crate::db::SharedSession;
use crate::models::*;
use serde_json::Value;

fn esc_sql(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''")),
        other => format!("'{}'", other.to_string().replace('\\', "\\\\").replace('\'', "''")),
    }
}

fn csv_cell(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn qid(kind: DbKind, name: &str) -> String {
    match kind {
        DbKind::Postgres | DbKind::Pgvector => format!("\"{}\"", name.replace('"', "\"\"")),
        DbKind::Sqlite => format!("\"{}\"", name.replace('"', "\"\"")),
        _ => format!("`{}`", name.replace('`', "``")),
    }
}

/// 归一化后的类型，跨引擎迁移先落到这里再按目标渲染
#[derive(Debug, Clone, PartialEq)]
enum Canon {
    Bool,
    TinyInt,
    SmallInt,
    Int,
    BigInt,
    Decimal(String),
    Float,
    Double,
    Date,
    Time,
    DateTime,
    Timestamp,
    Year,
    Char(String),
    Varchar(String),
    Text,
    Json,
    Blob,
    Enum,
    Uuid,
    Interval,
    Other,
}

fn type_args(t: &str) -> String {
    let s = t.trim();
    match (s.find('('), s.rfind(')')) {
        (Some(i), Some(j)) if j > i => s[i + 1..j].to_string(),
        _ => String::new(),
    }
}

fn type_base(t: &str) -> String {
    t.split('(')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace("unsigned", "")
        .replace("zerofill", "")
        .trim()
        .to_string()
}

fn canon_of(col: &ColumnInfo) -> Canon {
    let raw = col.data_type.trim().to_ascii_lowercase();
    let base = type_base(&raw);
    let args = type_args(&raw);
    match base.as_str() {
        "tinyint" => {
            if args == "1" {
                Canon::Bool
            } else {
                Canon::TinyInt
            }
        }
        "bool" | "boolean" => Canon::Bool,
        "smallint" | "int2" => Canon::SmallInt,
        "mediumint" | "int3" => Canon::Int,
        "int" | "integer" | "int4" => Canon::Int,
        "bigint" | "int8" => Canon::BigInt,
        "serial" | "bigserial" | "smallserial" => Canon::BigInt,
        "decimal" | "numeric" | "money" | "smallmoney" => Canon::Decimal(args),
        "float" | "real" | "float4" => Canon::Float,
        "double" | "double precision" | "float8" => Canon::Double,
        "date" => Canon::Date,
        "time" | "time without time zone" | "timetz" => Canon::Time,
        "datetime" => Canon::DateTime,
        "timestamp" | "timestamp without time zone" | "timestamp with time zone" => {
            Canon::Timestamp
        }
        "year" => Canon::Year,
        "char" | "character" | "bpchar" | "nchar" => Canon::Char(args),
        "varchar" | "character varying" | "nvarchar" => Canon::Varchar(args),
        "text" | "tinytext" | "mediumtext" | "longtext" | "citext" | "clob" => Canon::Text,
        "json" | "jsonb" => Canon::Json,
        "blob" | "tinyblob" | "mediumblob" | "longblob" | "bytea" | "binary" | "varbinary"
        | "image" => Canon::Blob,
        "enum" | "set" => Canon::Enum,
        "uuid" => Canon::Uuid,
        "interval" => Canon::Interval,
        _ => {
            if base.contains("int") {
                Canon::Int
            } else if base.contains("char") || base.contains("text") {
                Canon::Text
            } else {
                Canon::Other
            }
        }
    }
}

fn paren(args: &str) -> String {
    if args.trim().is_empty() {
        String::new()
    } else {
        format!("({})", args.trim())
    }
}

fn auto_inc(col: &ColumnInfo) -> bool {
    col.extra
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
        .contains("auto_increment")
}

/// 自增 / 身份列：MySQL 看 extra，PG 的 serial 只能从 nextval(...) 默认值认出来
fn is_identity(col: &ColumnInfo) -> bool {
    auto_inc(col) || col
        .default
        .as_deref()
        .unwrap_or("")
        .trim_start()
        .starts_with("nextval(")
}

/// 目标引擎的类型串
fn render_type(target: DbKind, col: &ColumnInfo) -> String {
    let c = canon_of(col);
    let ai = is_identity(col) && col.primary_key;
    match target {
        DbKind::Mysql => match &c {
            Canon::Bool => "tinyint(1)".into(),
            Canon::TinyInt => "tinyint".into(),
            Canon::SmallInt => "smallint".into(),
            Canon::Int => {
                if ai { "int auto_increment".into() } else { "int".into() }
            }
            Canon::BigInt => {
                if ai { "bigint auto_increment".into() } else { "bigint".into() }
            }
            Canon::Decimal(a) => format!("decimal{}", paren(a)),
            Canon::Float => "float".into(),
            Canon::Double => "double".into(),
            Canon::Date => "date".into(),
            Canon::Time => "time".into(),
            Canon::DateTime => "datetime".into(),
            Canon::Timestamp => "timestamp".into(),
            Canon::Year => "year".into(),
            Canon::Char(a) => format!("char{}", paren(a)),
            Canon::Varchar(a) => format!("varchar{}", if a.trim().is_empty() { "(255)".into() } else { paren(a) }),
            Canon::Text => "text".into(),
            Canon::Json => "json".into(),
            Canon::Blob => "longblob".into(),
            Canon::Enum | Canon::Other => "text".into(),
            Canon::Uuid => "char(36)".into(),
            Canon::Interval => "varchar(64)".into(),
        },
        DbKind::Postgres | DbKind::Pgvector => match &c {
            Canon::Bool => "boolean".into(),
            Canon::TinyInt | Canon::SmallInt => "smallint".into(),
            Canon::Int => {
                if ai { "serial".into() } else { "integer".into() }
            }
            Canon::BigInt | Canon::Year => {
                if ai { "bigserial".into() } else { "bigint".into() }
            }
            Canon::Decimal(a) => format!("numeric{}", paren(a)),
            Canon::Float => "real".into(),
            Canon::Double => "double precision".into(),
            Canon::Date => "date".into(),
            Canon::Time => "time".into(),
            Canon::DateTime | Canon::Timestamp => "timestamp".into(),
            Canon::Char(a) => format!("char{}", paren(a)),
            Canon::Varchar(a) => format!("varchar{}", paren(a)),
            Canon::Text | Canon::Enum | Canon::Other => "text".into(),
            Canon::Json => "jsonb".into(),
            Canon::Blob => "bytea".into(),
            Canon::Uuid => "uuid".into(),
            Canon::Interval => "interval".into(),
        },
        DbKind::Sqlite => match &c {
            Canon::Blob => "blob".into(),
            Canon::Float | Canon::Double | Canon::Decimal(_) => "real".into(),
            Canon::TinyInt
            | Canon::SmallInt
            | Canon::Int
            | Canon::BigInt
            | Canon::Bool
            | Canon::Year => "integer".into(),
            _ => "text".into(),
        },
        _ => "TEXT".into(),
    }
}

/// 默认值跨引擎改写：布尔列的 0/1、PG 的 `::type` 转换后缀
fn adapt_default(target: DbKind, rendered_ty: &str, raw: &str) -> Option<String> {
    let mut d = raw.trim().to_string();
    if d.is_empty() || d.eq_ignore_ascii_case("NULL") {
        return None;
    }
    if matches!(target, DbKind::Mysql | DbKind::Sqlite) {
        if let Some(i) = d.find("::") {
            d = d[..i].trim().to_string();
        }
    }
    if matches!(target, DbKind::Postgres | DbKind::Pgvector)
        && rendered_ty.eq_ignore_ascii_case("boolean")
    {
        match d.trim_matches('\'').to_ascii_lowercase().as_str() {
            "1" | "true" | "t" => return Some("true".into()),
            "0" | "false" | "f" => return Some("false".into()),
            _ => {}
        }
    }
    Some(d)
}

/// 建表语句（含主键），PG 额外返回列注释语句
pub fn create_table_statements(
    kind: DbKind,
    table: &str,
    cols: &[ColumnInfo],
) -> Vec<String> {
    let mut parts: Vec<String> = cols
        .iter()
        .map(|c| {
            let ty = render_type(kind, c);
            let mut p = format!("{} {}", qid(kind, &c.name), ty);
            if !c.nullable && !(is_identity(c) && c.primary_key) {
                p.push_str(" NOT NULL");
            }
            // 自增 / serial 列由目标引擎自己管理序列，跨引擎带 nextval 默认值必然报错
            if !ty.ends_with("serial") && !is_identity(c) {
                if let Some(d) = c
                    .default
                    .as_deref()
                    .and_then(|raw| adapt_default(kind, &ty, raw))
                {
                    p.push_str(&format!(" DEFAULT {d}"));
                }
            }
            if kind == DbKind::Mysql {
                if let Some(cm) = c.comment.as_deref().map(str::trim) {
                    if !cm.is_empty() {
                        p.push_str(&format!(" COMMENT '{}'", cm.replace('\'', "''")));
                    }
                }
            }
            p
        })
        .collect();
    let pks: Vec<String> = cols
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| qid(kind, &c.name))
        .collect();
    if !pks.is_empty() {
        parts.push(format!("PRIMARY KEY ({})", pks.join(", ")));
    }
    let t = qid(kind, table);
    let mut out = vec![format!(
        "CREATE TABLE IF NOT EXISTS {} (\n  {}\n)",
        t,
        parts.join(",\n  ")
    )];
    if matches!(kind, DbKind::Postgres | DbKind::Pgvector) {
        for c in cols {
            if let Some(cm) = c.comment.as_deref().map(str::trim) {
                if !cm.is_empty() {
                    out.push(format!(
                        "COMMENT ON COLUMN {}.{} IS '{}'",
                        t,
                        qid(kind, &c.name),
                        cm.replace('\'', "''")
                    ));
                }
            }
        }
    }
    out
}

fn build_create_sql(kind: DbKind, table: &str, cols: &[ColumnInfo]) -> String {
    create_table_statements(kind, table, cols)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// 二级索引；PG / SQLite 的索引名是库级唯一的，加表名前缀避免撞名
fn create_index_sql(kind: DbKind, table: &str, idx: &IndexInfo) -> String {
    let name = if matches!(kind, DbKind::Mysql) {
        idx.name.clone()
    } else {
        format!("idx_{}_{}", table, idx.name)
    };
    let cols: Vec<String> = idx.columns.iter().map(|c| qid(kind, c)).collect();
    format!(
        "CREATE {}INDEX {} ON {} ({})",
        if idx.unique { "UNIQUE " } else { "" },
        qid(kind, &name),
        qid(kind, table),
        cols.join(", ")
    )
}

/// 行数上限的统一语义：None 用默认值，Some(<=0) 表示不限制（全部行）
pub fn row_limit(max_rows: Option<i64>, default: u64) -> u64 {
    match max_rows {
        None => default,
        Some(v) if v <= 0 => u64::MAX,
        Some(v) => v as u64,
    }
}

/// 详情页里的可读上限，用于日志和回显
pub fn row_limit_label(limit: u64) -> String {
    if limit == u64::MAX {
        "全部行".into()
    } else {
        limit.to_string()
    }
}

async fn fetch_all(
    session: &SharedSession,
    database: &str,
    kind: DbKind,
    table: &str,
    max_rows: u64,
) -> Result<(Vec<ColumnInfo>, Vec<String>, Vec<Vec<Value>>), String> {
    let cols = session.list_columns(database, None, table).await?;
    let mut all: Vec<Vec<Value>> = Vec::new();
    let (columns, _) = scan_pages(session, database, kind, table, max_rows, |_columns, rows| {
        all.extend_from_slice(rows);
        Ok(false)
    })
    .await?;
    Ok((cols, columns, all))
}

/// 按页取数，单页最多 500 行，且不超过剩余预算 `max_rows`
async fn fetch_page(
    session: &SharedSession,
    database: &str,
    kind: DbKind,
    table: &str,
    page: u64,
    budget: u64,
) -> Result<(Vec<String>, Vec<Vec<Value>>), String> {
    let select = format!("SELECT * FROM {}", qid(kind, table));
    let page_size = 500u64;
    let p = session
        .query_page(database, &select, page, page_size.min(budget.max(1)))
        .await?;
    let mut rows = p.rows;
    if rows.len() as u64 > budget {
        rows.truncate(budget as usize);
    }
    Ok((p.columns, rows))
}

/// 逐页遍历整表：回调返回 `true` 提前停止。避免把全部行一次性堆在内存里。
async fn scan_pages<F>(
    session: &SharedSession,
    database: &str,
    kind: DbKind,
    table: &str,
    max_rows: u64,
    mut on_page: F,
) -> Result<(Vec<String>, u64), String>
where
    F: FnMut(&[String], &[Vec<Value>]) -> Result<bool, String>,
{
    const PAGE: u64 = 500;
    let mut columns: Vec<String> = Vec::new();
    let mut seen = 0u64;
    let mut page = 1u64;
    loop {
        let budget = max_rows.saturating_sub(seen);
        if budget == 0 {
            break;
        }
        let (cols, rows) = fetch_page(session, database, kind, table, page, budget).await?;
        if columns.is_empty() && !cols.is_empty() {
            columns = cols;
        }
        let n = rows.len() as u64;
        let stop = on_page(&columns, &rows)?;
        seen += n;
        // 没取满一页 = 已到末尾
        if stop || n < budget.min(PAGE) {
            break;
        }
        page += 1;
        if page > 2_000_000 {
            break;
        }
    }
    Ok((columns, seen))
}

/// Build CREATE + INSERT dump for one table.
pub async fn dump_table_sql(
    session: &SharedSession,
    kind: DbKind,
    database: &str,
    table: &str,
    include_structure: bool,
    include_data: bool,
    max_rows: u64,
) -> Result<String, String> {
    let mut out = String::new();
    out.push_str(&format!(
        "-- AgentDB dump\n-- table: {table}\n-- at: {}\n\n",
        chrono::Utc::now().to_rfc3339()
    ));
    let cols = session.list_columns(database, None, table).await?;
    if include_structure {
        out.push_str(&build_create_sql(kind, table, &cols));
        out.push_str("\n\n");
    }
    if include_data {
        let (_, total) = scan_pages(
            session, database, kind, table, max_rows, |columns, rows| {
                let col_list = columns
                    .iter()
                    .map(|c| qid(kind, c))
                    .collect::<Vec<_>>()
                    .join(", ");
                for row in rows {
                    let vals: Vec<String> = row.iter().map(esc_sql).collect();
                    out.push_str(&format!(
                        "INSERT INTO {} ({}) VALUES ({});\n",
                        qid(kind, table),
                        col_list,
                        vals.join(", ")
                    ));
                }
                Ok(false)
            },
        )
        .await?;
        out.push_str(&format!("\n-- rows: {total}\n"));
    }
    Ok(out)
}

/// Dump all tables in a database (SQL-like engines only).
pub async fn dump_database_sql(
    session: &SharedSession,
    kind: DbKind,
    database: &str,
    include_structure: bool,
    include_data: bool,
    max_rows: u64,
) -> Result<String, String> {
    let tables = session.list_tables(database, None).await?;
    let mut out = String::new();
    out.push_str(&format!(
        "-- AgentDB database dump\n-- db: {database}\n-- at: {}\n\n",
        chrono::Utc::now().to_rfc3339()
    ));
    for t in tables {
        if matches!(t.table_type.as_deref(), Some("VIEW") | Some("MATERIALIZED VIEW")) {
            continue;
        }
        match dump_table_sql(
            session,
            kind,
            database,
            &t.name,
            include_structure,
            include_data,
            max_rows,
        )
        .await
        {
            Ok(sql) => {
                out.push_str(&sql);
                out.push_str("\n");
            }
            Err(e) => {
                out.push_str(&format!("-- skip {table}: {e}\n", table = t.name));
            }
        }
    }
    Ok(out)
}

pub fn to_csv(columns: &[String], rows: &[Vec<Value>]) -> String {
    let mut s = String::from("\u{FEFF}"); // BOM for Excel
    s.push_str(&columns.join(","));
    s.push('\n');
    for row in rows {
        let cells: Vec<String> = row.iter().map(csv_cell).collect();
        s.push_str(&cells.join(","));
        s.push('\n');
    }
    s
}

/// SpreadsheetML 2003 — Excel can open without a zip writer.
pub fn to_excel_xml(columns: &[String], rows: &[Vec<Value>]) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\"?>\n");
    s.push_str("<?mso-application progid=\"Excel.Sheet\"?>\n");
    s.push_str("<Workbook xmlns=\"urn:schemas-microsoft-com:office:spreadsheet\"\n");
    s.push_str(" xmlns:ss=\"urn:schemas-microsoft-com:office:spreadsheet\">\n");
    s.push_str("<Worksheet ss:Name=\"Sheet1\"><Table>\n");
    s.push_str("<Row>");
    for c in columns {
        s.push_str(&format!(
            "<Cell><Data ss:Type=\"String\">{}</Data></Cell>",
            xml_escape(c)
        ));
    }
    s.push_str("</Row>\n");
    for row in rows {
        s.push_str("<Row>");
        for cell in row {
            let (ty, text) = match cell {
                Value::Null => ("String", String::new()),
                Value::Number(n) => ("Number", n.to_string()),
                Value::Bool(b) => ("String", b.to_string()),
                Value::String(s0) => ("String", s0.clone()),
                other => ("String", other.to_string()),
            };
            s.push_str(&format!(
                "<Cell><Data ss:Type=\"{}\">{}</Data></Cell>",
                ty,
                xml_escape(&text)
            ));
        }
        s.push_str("</Row>\n");
    }
    s.push_str("</Table></Worksheet></Workbook>\n");
    s
}

/// 前端可选的脱敏规则，非法值直接拒绝而不是静默跳过
pub const MASK_MODES: &[&str] = &[
    "none",
    "mask-all",
    "keep-edges",
    "email",
    "phone",
    "idcard",
    "bankcard",
    "name",
    "hash",
    "null",
    "replace",
];

fn cell_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn norm_col(s: &str) -> String {
    let t = s.trim().trim_matches('"').trim_matches('`').to_lowercase();
    match t.rsplit_once('.') {
        Some((_, last)) if !last.is_empty() => last.to_string(),
        _ => t,
    }
}

fn clamp_count(v: Option<i64>) -> usize {
    v.unwrap_or(0).clamp(0, 64) as usize
}

fn take_edges(chars: &[char], head: usize, tail: usize) -> (String, usize, String) {
    let h: String = chars.iter().take(head).collect();
    let t: String = if tail == 0 {
        String::new()
    } else {
        chars[chars.len() - tail..].iter().collect()
    };
    let body = chars.len().saturating_sub(h.chars().count() + t.chars().count());
    (h, body, t)
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// FNV-1a 64 位摘要，用于「同值同码」的匿名化（非密码学强度）
fn fnv1a64(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_011b);
    }
    format!("{h:016x}")
}

fn keep_edges(text: &str, head: usize, tail: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= head + tail {
        return "*".repeat(chars.len().max(1));
    }
    let (h, body, t) = take_edges(&chars, head, tail);
    format!("{h}{}{t}", "*".repeat(body))
}

fn mask_value(rule: &MaskRule, raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let head = clamp_count(rule.keep_head);
    let tail = clamp_count(rule.keep_tail);
    match rule.mode.as_str() {
        "mask-all" => "*".repeat(raw.chars().count().clamp(1, 12)),
        "keep-edges" => keep_edges(raw, head, tail),
        "email" => match raw.split_once('@') {
            Some((local, domain)) if !local.is_empty() && !domain.is_empty() => {
                let first = local.chars().next().unwrap_or('*');
                format!("{first}***@{domain}")
            }
            _ => "*".repeat(raw.chars().count().clamp(1, 12)),
        },
        "phone" => {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
            if !is_digits(&digits) || digits.len() < 7 {
                return "*".repeat(raw.chars().count().clamp(1, 12));
            }
            let chars: Vec<char> = digits.chars().collect();
            let (h, body, t) = take_edges(&chars, 3, 4);
            format!("{h}{}{t}", "*".repeat(body))
        }
        "idcard" => {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit() || *c == 'X').collect();
            match digits.len() {
                15 => keep_edges(&digits, 6, 3),
                18 => keep_edges(&digits, 6, 4),
                _ => keep_edges(raw, head.max(3), tail.max(2)),
            }
        }
        "bankcard" => {
            let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() < 8 {
                return "*".repeat(raw.chars().count().clamp(1, 12));
            }
            let chars: Vec<char> = digits.chars().collect();
            let body = chars.len() - 4;
            let t: String = chars[body..].iter().collect();
            format!("{}{t}", "*".repeat(body))
        }
        "name" => keep_edges(raw, 1, 0),
        "hash" => fnv1a64(raw),
        "null" => String::new(),
        "replace" => rule
            .mask_text
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "***".into()),
        _ => raw.to_string(),
    }
}

pub struct MaskReport {
    pub applied: Vec<String>,
    pub unmatched: Vec<String>,
}

/// 就地按规则改写行值；非字符串列会被转成脱敏后的文本
pub fn apply_masks(columns: &[String], rows: &mut [Vec<Value>], rules: &[MaskRule]) -> MaskReport {
    let mut applied = Vec::new();
    let mut unmatched = Vec::new();
    for rule in rules {
        if rule.column.trim().is_empty() || rule.mode == "none" {
            continue;
        }
        let want = norm_col(&rule.column);
        let mut hit = false;
        for (idx, col) in columns.iter().enumerate() {
            if norm_col(col) != want {
                continue;
            }
            hit = true;
            for row in rows.iter_mut() {
                if let Some(cell) = row.get_mut(idx) {
                    let text = cell_text(cell);
                    if !text.is_empty() || !cell.is_null() {
                        *cell = Value::String(mask_value(rule, &text));
                    }
                }
            }
        }
        if hit {
            applied.push(rule.column.clone());
        } else {
            unmatched.push(rule.column.clone());
        }
    }
    MaskReport { applied, unmatched }
}

/// 单行写入目标；返回 false 表示这行没有可写字段（整行都是默认值），跳过
async fn sync_write_row(
    dst: &SharedSession,
    dst_kind: DbKind,
    dst_db: &str,
    dst_table: &str,
    columns: &[String],
    row: &[Value],
    row_no: usize,
    pk_cols: &[String],
) -> Result<bool, String> {
    let mut map = serde_json::Map::new();
    for (i, c) in columns.iter().enumerate() {
        if let Some(v) = row.get(i) {
            map.insert(c.clone(), v.clone());
        }
    }
    if dst_kind == DbKind::Redis {
        // Redis 没有表：每行落成一个 string 键，值是整行 JSON
        let id = pk_cols
            .iter()
            .find_map(|p| {
                map.get(p)
                    .filter(|v| !v.is_null())
                    .map(|v| v.to_string().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| row_no.to_string());
        let mut out = serde_json::Map::new();
        out.insert("key".into(), Value::String(format!("{dst_table}:{id}")));
        out.insert(
            "value".into(),
            Value::String(Value::Object(map).to_string()),
        );
        return dst.insert_row(dst_db, "string", out).await.map(|_| true);
    }
    map.retain(|_, v| !v.is_null());
    if map.is_empty() {
        return Ok(false);
    }
    dst.insert_row(dst_db, dst_table, map).await.map(|_| true)
}

/// 同步一张表（或把 SQL 表写进 Mongo 集合 / Redis 键），逐页取数逐行写，不整表驻留内存
pub async fn sync_table(
    src: &SharedSession,
    src_kind: DbKind,
    src_db: &str,
    src_table: &str,
    dst: &SharedSession,
    dst_kind: DbKind,
    dst_db: &str,
    dst_table: &str,
    create_if_missing: bool,
    max_rows: u64,
) -> Result<SyncTableResult, String> {
    let sql_kind = |k: DbKind| {
        matches!(
            k,
            DbKind::Mysql | DbKind::Postgres | DbKind::Pgvector | DbKind::Sqlite
        )
    };
    let create_target = create_if_missing && sql_kind(dst_kind) && sql_kind(src_kind);
    let mut created = false;
    if create_target {
        let cols = src.list_columns(src_db, None, src_table).await?;
        let ddl = build_create_sql(dst_kind, dst_table, &cols);
        let _ = dst.execute(dst_db, &ddl).await;
        created = true;
    }
    let pk_cols: Vec<String> = if dst_kind == DbKind::Redis {
        src.list_columns(src_db, None, src_table)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name)
            .collect()
    } else {
        Vec::new()
    };

    const PAGE: u64 = 500;
    let mut columns: Vec<String> = Vec::new();
    let mut page = 1u64;
    let mut fetched = 0u64;
    let mut row_no = 0usize;
    let mut copied = 0u64;
    let mut failed = 0u64;
    let mut blank = 0u64;
    let mut first_err = String::new();
    loop {
        let budget = max_rows.saturating_sub(fetched);
        if budget == 0 {
            break;
        }
        let (cols, rows) = fetch_page(src, src_db, src_kind, src_table, page, budget).await?;
        if columns.is_empty() && !cols.is_empty() {
            columns = cols;
        }
        let n = rows.len() as u64;
        for row in &rows {
            row_no += 1;
            match sync_write_row(
                dst,
                dst_kind,
                dst_db,
                dst_table,
                &columns,
                row,
                row_no,
                &pk_cols,
            )
            .await
            {
                Ok(true) => copied += 1,
                Ok(false) => blank += 1,
                Err(e) => {
                    failed += 1;
                    crate::logger::warn("sync_table", format!("row {row_no} skip: {e}"));
                    if first_err.is_empty() {
                        first_err = e;
                    }
                }
            }
        }
        fetched += n;
        if n < budget.min(PAGE) {
            break;
        }
        page += 1;
        if page > 2_000_000 {
            break;
        }
    }

    if copied == 0 && failed > 0 {
        return Err(format!(
            "写入{}失败：{first_err}",
            if dst_kind == DbKind::Redis {
                " Redis 键 "
            } else {
                dst_table
            }
        ));
    }

    let target = if dst_kind == DbKind::Redis {
        format!("{dst_table}:* 键")
    } else {
        dst_table.to_string()
    };
    let mut detail = format!("写入 {copied} 行到 {target}");
    if created {
        detail = format!("已建目标对象；{detail}");
    }
    if blank > 0 {
        detail.push_str(&format!("；{blank} 行整行为空，未写入"));
    }
    if failed > 0 {
        detail.push_str(&format!("；{failed} 行写入失败：{first_err}"));
    }
    Ok(SyncTableResult {
        table: if dst_kind == DbKind::Redis {
            format!("{dst_table}:*")
        } else {
            dst_table.to_string()
        },
        status: if created { "created" } else { "copied" }.into(),
        rows: copied as i64,
        failed_rows: failed as i64,
        detail,
    })
}

/// 整库同步：逐表调用 sync_table，把每张表的结果都收进报告
pub async fn sync_database(
    src: &SharedSession,
    src_kind: DbKind,
    src_db: &str,
    dst: &SharedSession,
    dst_kind: DbKind,
    dst_db: &str,
    create_if_missing: bool,
    max_rows_per_table: u64,
) -> Result<SyncReport, String> {
    let tables = src.list_tables(src_db, None).await?;
    let mut report = SyncReport::default();
    let mut scanned = 0usize;
    for t in &tables {
        // 视图 / 物化视图没有可复制的基表数据
        if matches!(
            t.table_type.as_deref(),
            Some("VIEW") | Some("MATERIALIZED VIEW")
        ) {
            continue;
        }
        scanned += 1;
        match sync_table(
            src,
            src_kind,
            src_db,
            &t.name,
            dst,
            dst_kind,
            dst_db,
            &t.name,
            create_if_missing,
            max_rows_per_table,
        )
        .await
        {
            Ok(r) => {
                report.total_rows += r.rows;
                report.failed_rows += r.failed_rows;
                if r.status == "created" {
                    report.created += 1;
                } else {
                    report.copied += 1;
                }
                report.tables.push(r);
            }
            Err(e) => {
                report.failed += 1;
                report.tables.push(SyncTableResult {
                    table: t.name.clone(),
                    status: "failed".into(),
                    rows: 0,
                    failed_rows: 0,
                    detail: e,
                });
            }
        }
    }
    report.message = sync_message(dst_db, &report, scanned, dst_kind);
    Ok(report)
}

/// 单表同步也包成报告，前端只认一种结构
pub fn single_table_report(
    dst_db: &str,
    one: SyncTableResult,
    dst_kind: DbKind,
) -> SyncReport {
    let mut report = SyncReport::default();
    report.total_rows = one.rows;
    report.failed_rows = one.failed_rows;
    if one.status == "created" {
        report.created = 1;
    } else {
        report.copied = 1;
    }
    report.tables.push(one);
    report.message = sync_message(dst_db, &report, 1, dst_kind);
    report
}

/// 结论一句话，始终以「已」开头供前端判定成功条；部分失败也写在同一句里
pub fn sync_message(dst_db: &str, r: &SyncReport, scanned: usize, dst_kind: DbKind) -> String {
    let ok_tables = r.created + r.copied;
    let mut msg = if scanned == 0 {
        format!("已同步 0 张表到 {dst_db}：源库里没有可同步的表")
    } else if r.total_rows == 0 && r.failed == 0 {
        format!("已同步 {ok_tables} 张表到 {dst_db}：都是空表，0 行")
    } else {
        format!("已同步 {ok_tables} 张表 / {} 行到 {dst_db}", r.total_rows)
    };
    if dst_kind == DbKind::Redis {
        msg.push_str("（每行一个 string 键）");
    }
    if r.failed > 0 {
        let shown: Vec<String> = r
            .tables
            .iter()
            .filter(|t| t.status == "failed")
            .take(5)
            .map(|t| format!("{}: {}", t.table, t.detail))
            .collect();
        msg.push_str(&format!("；{} 张表失败：{}", r.failed, shown.join(" | ")));
    }
    if r.failed_rows > 0 {
        msg.push_str(&format!("；{} 行写入失败", r.failed_rows));
    }
    msg
}

pub fn parse_csv(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let text = text.trim_start_matches('\u{FEFF}');
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cur = Vec::new();
    let mut cell = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                cell.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            cur.push(std::mem::take(&mut cell));
        } else if c == '\n' {
            cur.push(std::mem::take(&mut cell));
            if !(cur.len() == 1 && cur[0].is_empty()) {
                rows.push(std::mem::take(&mut cur));
            }
            cur.clear();
        } else if c == '\r' {
            // skip
        } else {
            cell.push(c);
        }
    }
    if !cell.is_empty() || !cur.is_empty() {
        cur.push(cell);
        if !(cur.len() == 1 && cur[0].is_empty()) {
            rows.push(cur);
        }
    }
    if rows.is_empty() {
        return (vec![], vec![]);
    }
    let header = rows.remove(0);
    (header, rows)
}

/// Parse SpreadsheetML 2003 XML (our Excel export format).
pub fn parse_excel_xml(text: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut in_cell = false;
    let mut in_data = false;
    for part in text.split_inclusive(['<', '>']) {
        // part includes tag or text chunks
        if part.starts_with('<') {
            let tag = part.trim_start_matches('<').trim_end_matches('>');
            let name = tag.split_whitespace().next().unwrap_or("");
            match name {
                "Row" | "Row/" => {
                    if !cur.is_empty() {
                        rows.push(std::mem::take(&mut cur));
                    }
                }
                "Cell" | "Cell/" => {
                    if in_cell {
                        cur.push(std::mem::take(&mut cell));
                    }
                    in_cell = true;
                }
                "/Cell" => {
                    cur.push(std::mem::take(&mut cell));
                    in_cell = false;
                }
                "/Row" => {
                    if in_cell {
                        cur.push(std::mem::take(&mut cell));
                        in_cell = false;
                    }
                    rows.push(std::mem::take(&mut cur));
                }
                "Data" => in_data = true,
                "/Data" => in_data = false,
                _ => {}
            }
        } else if in_cell && in_data {
            cell.push_str(&part.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\""));
        }
    }
    if rows.is_empty() {
        return (vec![], vec![]);
    }
    let header = rows.remove(0);
    (header, rows)
}

fn coerce_value(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if let Ok(n) = t.parse::<i64>() {
        return Value::from(n);
    }
    if let Ok(n) = t.parse::<f64>() {
        return serde_json::Number::from_f64(n)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(t.to_string()));
    }
    if t.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if t.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    Value::String(t.to_string())
}

pub struct ImportResult {
    pub inserted: u64,
    pub skipped: u64,
    pub created: bool,
    pub message: String,
}

/// Import CSV / SpreadsheetML rows into a table.
pub async fn import_tabular(
    session: &SharedSession,
    kind: DbKind,
    database: &str,
    table: &str,
    header: &[String],
    rows: &[Vec<String>],
    create_if_missing: bool,
) -> Result<ImportResult, String> {
    if header.is_empty() {
        return Err("文件没有表头".into());
    }
    let mut created = false;
    let mut cols: Vec<String> = header.to_vec();
    if create_if_missing
        && matches!(
            kind,
            DbKind::Mysql | DbKind::Postgres | DbKind::Pgvector | DbKind::Sqlite
        )
    {
        // try list columns first
        let existing = session
            .list_columns(database, None, table)
            .await
            .unwrap_or_default();
        if existing.is_empty() {
            let col_defs: Vec<ColumnInfo> = header
                .iter()
                .map(|h| ColumnInfo {
                    name: h.clone(),
                    data_type: "TEXT".into(),
                    nullable: true,
                    primary_key: false,
                    default: None,
                    comment: None,
                    extra: None,
                    max_length: None,
                })
                .collect();
            let ddl = build_create_sql(kind, table, &col_defs);
            session
                .execute(database, &ddl)
                .await
                .map_err(|e| format!("创建目标表失败: {e}"))?;
            created = true;
        } else {
            cols = existing.into_iter().map(|c| c.name).collect();
        }
    }

    let mut inserted = 0u64;
    let mut skipped = 0u64;
    for row in rows {
        let mut map = serde_json::Map::new();
        for (i, name) in header.iter().enumerate() {
            if let Some(raw) = row.get(i) {
                let v = coerce_value(raw);
                if !v.is_null() {
                    map.insert(name.clone(), v);
                }
            }
        }
        // only keep known columns when table already exists
        if !created && !cols.is_empty() {
            map.retain(|k, _| cols.iter().any(|c| c == k));
        }
        if map.is_empty() {
            skipped += 1;
            continue;
        }
        match session.insert_row(database, table, map).await {
            Ok(_) => inserted += 1,
            Err(_) => skipped += 1,
        }
    }

    Ok(ImportResult {
        inserted,
        skipped,
        created,
        message: format!("导入完成：成功 {inserted} 行，跳过 {skipped} 行"),
    })
}

/// Split SQL script into statements (naive, skips $$ blocks).
pub fn split_sql_script(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if in_line_comment {
            cur.push(c);
            if c == '\n' {
                in_line_comment = false;
            }
            continue;
        }
        if in_block_comment {
            cur.push(c);
            if c == '*' && chars.peek() == Some(&'/') {
                cur.push(chars.next().unwrap());
                in_block_comment = false;
            }
            continue;
        }
        if !in_single && !in_double {
            if c == '-' && chars.peek() == Some(&'-') {
                in_line_comment = true;
                cur.push(c);
                continue;
            }
            if c == '/' && chars.peek() == Some(&'*') {
                in_block_comment = true;
                cur.push(c);
                continue;
            }
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
            cur.push(c);
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            cur.push(c);
            continue;
        }
        if c == ';' && !in_single && !in_double {
            let s = cur.trim().to_string();
            if !s.is_empty() && !s.chars().all(|ch| ch.is_whitespace() || ch == '-' || ch == '/') {
                if !s.trim_start().starts_with("--") && !is_only_comments(&s) {
                    out.push(s);
                }
            }
            cur.clear();
            continue;
        }
        cur.push(c);
    }
    let s = cur.trim().to_string();
    if !s.is_empty() && !is_only_comments(&s) {
        out.push(s);
    }
    out
}

fn is_only_comments(s: &str) -> bool {
    s.lines().all(|l| {
        let t = l.trim();
        t.is_empty() || t.starts_with("--") || t.starts_with("/*")
    })
}

pub struct ScriptResult {
    pub executed: u64,
    pub failed: u64,
    pub message: String,
}

pub async fn run_sql_script(
    session: &SharedSession,
    database: &str,
    sql: &str,
) -> Result<ScriptResult, String> {
    let stmts = split_sql_script(sql);
    let mut executed = 0u64;
    let mut failed = 0u64;
    let mut first_err = String::new();
    for stmt in stmts {
        match session.execute(database, &stmt).await {
            Ok(_) => executed += 1,
            Err(e) => {
                failed += 1;
                if first_err.is_empty() {
                    first_err = e;
                }
            }
        }
    }
    let mut msg = format!("SQL 脚本执行完成：成功 {executed} 条，失败 {failed} 条");
    if !first_err.is_empty() {
        msg.push_str(&format!("；首个错误：{first_err}"));
    }
    Ok(ScriptResult {
        executed,
        failed,
        message: msg,
    })
}

/// 迁移：结构 + 数据 + 二级索引，支持 MySQL / PostgreSQL / PgVector / SQLite 互转
pub async fn migrate_database(
    src: &SharedSession,
    src_kind: DbKind,
    src_db: &str,
    dst: &SharedSession,
    dst_kind: DbKind,
    dst_db: &str,
    opts: &MigrateOptions,
) -> Result<MigrateReport, String> {
    if !is_sql_kind(src_kind) || !is_sql_kind(dst_kind) {
        return Err("迁移支持 MySQL / PostgreSQL / PgVector / SQLite 之间互转".into());
    }
    let tables = src.list_tables(src_db, None).await?;
    let mut report = MigrateReport::default();
    for t in &tables {
        if matches!(
            t.table_type.as_deref(),
            Some("VIEW") | Some("MATERIALIZED VIEW")
        ) {
            report.skipped += 1;
            report.tables.push(MigrateTableResult {
                table: t.name.clone(),
                status: "skipped".into(),
                rows: 0,
                detail: "视图不迁移".into(),
            });
            continue;
        }
        match migrate_table(
            src, src_kind, src_db, dst, dst_kind, dst_db, &t.name, opts,
        )
        .await
        {
            Ok(r) => {
                match r.status.as_str() {
                    "created" => report.created += 1,
                    "copied" => report.copied += 1,
                    "skipped" => report.skipped += 1,
                    _ => {}
                }
                report.total_rows += r.rows;
                report.tables.push(r);
            }
            Err(e) => {
                report.failed += 1;
                report.tables.push(MigrateTableResult {
                    table: t.name.clone(),
                    status: "failed".into(),
                    rows: 0,
                    detail: e,
                });
            }
        }
    }
    Ok(report)
}

pub fn is_sql_kind(k: DbKind) -> bool {
    matches!(
        k,
        DbKind::Mysql | DbKind::Postgres | DbKind::Pgvector | DbKind::Sqlite
    )
}

async fn table_exists(dst: &SharedSession, db: &str, table: &str) -> bool {
    matches!(dst.list_columns(db, None, table).await, Ok(cols) if !cols.is_empty())
}

async fn migrate_table(
    src: &SharedSession,
    src_kind: DbKind,
    src_db: &str,
    dst: &SharedSession,
    dst_kind: DbKind,
    dst_db: &str,
    table: &str,
    opts: &MigrateOptions,
) -> Result<MigrateTableResult, String> {
    let cols = src.list_columns(src_db, None, table).await?;
    if cols.is_empty() {
        return Ok(MigrateTableResult {
            table: table.to_string(),
            status: "skipped".into(),
            rows: 0,
            detail: "源表没有字段信息".into(),
        });
    }
    let exists = table_exists(dst, dst_db, table).await;
    if exists && opts.on_exists == "skip" {
        return Ok(MigrateTableResult {
            table: table.to_string(),
            status: "skipped".into(),
            rows: 0,
            detail: "目标已存在，按设置跳过".into(),
        });
    }

    let mut detail: Vec<String> = Vec::new();
    if opts.with_structure {
        if exists && opts.on_exists == "recreate" {
            dst.execute(dst_db, &format!("DROP TABLE IF EXISTS {}", qid(dst_kind, table)))
                .await
                .map_err(|e| format!("清空目标表失败: {e}"))?;
            detail.push("已清空目标表".into());
        }
        for st in create_table_statements(dst_kind, table, &cols) {
            dst.execute(dst_db, &st)
                .await
                .map_err(|e| format!("建表失败: {e}"))?;
        }
        detail.push(if exists { "结构已对齐" } else { "已建表" }.into());
    }

    let mut rows = 0i64;
    let mut data_err = String::new();
    if opts.with_data {
        let limit = if opts.max_rows_per_table > 0 {
            opts.max_rows_per_table as u64
        } else {
            u64::MAX
        };
        let (_, columns, data) = fetch_all(src, src_db, src_kind, table, limit).await?;
        // MySQL 的 tinyint(1) 会被读成 0/1，落到 PG / SQLite 的 boolean 列要转成布尔
        let bool_cols: std::collections::HashSet<String> = cols
            .iter()
            .filter(|c| {
                matches!(
                    render_type(dst_kind, c).to_ascii_lowercase().as_str(),
                    "boolean" | "bool"
                )
            })
            .map(|c| c.name.clone())
            .collect();
        let mut fails = 0i64;
        let mut first_err = String::new();
        for row in &data {
            let mut map = serde_json::Map::new();
            for (i, c) in columns.iter().enumerate() {
                if let Some(v) = row.get(i) {
                    if v.is_null() {
                        continue;
                    }
                    let v = match (bool_cols.contains(c), v) {
                        (true, serde_json::Value::Number(n)) => {
                            serde_json::Value::Bool(n.as_i64().map(|i| i != 0).unwrap_or(true))
                        }
                        _ => v.clone(),
                    };
                    map.insert(c.clone(), v);
                }
            }
            if map.is_empty() {
                continue;
            }
            match dst.insert_row(dst_db, table, map).await {
                Ok(_) => rows += 1,
                Err(e) => {
                    crate::logger::warn("migrate_table", format!("{table} row skip: {e}"));
                    fails += 1;
                    if first_err.is_empty() {
                        first_err = e;
                    }
                }
            }
        }
        if rows == 0 && fails > 0 {
            data_err = format!("{} 行全部写入失败：{first_err}", fails);
        } else {
            let mut d = format!("写入 {rows} 行");
            if fails > 0 {
                d.push_str(&format!("，{fails} 行失败：{first_err}"));
            }
            detail.push(d);
            if rows > 0 {
                if let Some(seq) = serial_sequence_sql(dst_kind, table, &cols) {
                    match dst.execute(dst_db, &seq).await {
                        Ok(_) => detail.push("已回填自增序列".into()),
                        Err(e) => detail.push(format!("自增序列回填失败: {e}")),
                    }
                }
            }
        }
    }

    if opts.with_indexes {
        let idxs = src.list_indexes(src_db, None, table).await.unwrap_or_default();
        let mut ok = 0i64;
        let mut errs: Vec<String> = Vec::new();
        for ix in idxs.iter().filter(|i| !i.primary && !i.columns.is_empty()) {
            match dst
                .execute(dst_db, &create_index_sql(dst_kind, table, ix))
                .await
            {
                Ok(_) => ok += 1,
                Err(e) => errs.push(format!("{}: {e}", ix.name)),
            }
        }
        if ok > 0 {
            detail.push(format!("已建 {ok} 个索引"));
        }
        if !errs.is_empty() {
            detail.push(format!("索引失败 {} 个：{}", errs.len(), errs.join(" | ")));
        }
    }

    if !data_err.is_empty() {
        return Err(data_err);
    }

    Ok(MigrateTableResult {
        table: table.to_string(),
        status: if opts.with_data { "copied" } else { "created" }.to_string(),
        rows,
        detail: detail.join("；"),
    })
}

/// PG 的 serial 主键迁移完要回填序列，否则后续插入会撞主键
fn serial_sequence_sql(kind: DbKind, table: &str, cols: &[ColumnInfo]) -> Option<String> {
    if !matches!(kind, DbKind::Postgres | DbKind::Pgvector) {
        return None;
    }
    let pk = cols.iter().find(|c| c.primary_key && is_identity(c))?;
    Some(format!(
        "SELECT setval(pg_get_serial_sequence('{}','{}'), COALESCE(MAX({}), 1)) FROM {}",
        table.replace('\'', "''"),
        pk.name.replace('\'', "''"),
        qid(kind, &pk.name),
        qid(kind, table)
    ))
}
