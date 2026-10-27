//! 结构页「AI 补全注释与索引建议」的提示词与结果解析。
//! 这里只做纯字符串与 JSON 处理，取结构、调模型、写库都在命令层。

use crate::models::*;
use serde_json::Value;
use std::collections::HashSet;

pub struct TableShape<'a> {
    pub kind: &'a str,
    pub database: &'a str,
    pub table: &'a str,
    pub table_comment: Option<&'a str>,
    pub columns: &'a [ColumnInfo],
    pub indexes: &'a [IndexInfo],
    pub fks: &'a [ForeignKeyInfo],
    pub sample_header: &'a [String],
    pub sample_rows: &'a [Vec<Value>],
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::String(s) => {
            let t = s.chars().take(40).collect::<String>();
            if s.chars().count() > 40 {
                format!("{t}…")
            } else {
                t
            }
        }
        other => other.to_string(),
    }
}

pub fn build_prompt(s: &TableShape) -> String {
    let mut p = String::new();
    p.push_str("你在为一张已存在的数据库表补全中文注释，并给出索引建议。\n\n");
    p.push_str(&format!(
        "引擎：{}\n表：{}.{}\n现有表注释：{}\n\n字段：\n",
        s.kind,
        s.database,
        s.table,
        s.table_comment.unwrap_or("（无）")
    ));
    for c in s.columns {
        let mut line = format!("- {} {}{}", c.name, c.data_type, if c.nullable { " 可空" } else { " 非空" });
        if c.primary_key {
            line.push_str(" 主键");
        }
        if let Some(d) = &c.default {
            line.push_str(&format!(" 默认={d}"));
        }
        if let Some(e) = &c.extra {
            if !e.trim().is_empty() {
                line.push_str(&format!(" {}", e.trim()));
            }
        }
        line.push_str(&format!(" 注释={}", c.comment.as_deref().unwrap_or("（无）")));
        p.push_str(&line);
        p.push('\n');
    }
    p.push_str("\n现有索引：\n");
    if s.indexes.is_empty() {
        p.push_str("（无）\n");
    }
    for i in s.indexes {
        p.push_str(&format!(
            "- {} ({}){}{}\n",
            i.name,
            i.columns.join(", "),
            if i.unique { " 唯一" } else { "" },
            if i.primary { " 主键索引" } else { "" }
        ));
    }
    if !s.fks.is_empty() {
        p.push_str("\n外键：\n");
        for f in s.fks {
            p.push_str(&format!("- {} → {}.{}\n", f.column, f.ref_table, f.ref_column));
        }
    }
    if !s.sample_rows.is_empty() {
        p.push_str("\n样例数据（最多 5 行，仅用于判断业务含义）：\n");
        p.push_str(&s.sample_header.join(" | "));
        p.push('\n');
        for row in s.sample_rows.iter().take(5) {
            p.push_str(&row.iter().map(cell).collect::<Vec<_>>().join(" | "));
            p.push('\n');
        }
    }
    p.push_str(
        "\n只输出一个 JSON 对象，不要任何解释文字、不要 Markdown 代码块，结构如下：\n\
         {\"table_comment\":\"...\",\"column_comments\":[{\"name\":\"字段名\",\"comment\":\"...\"}],\
         \"index_notes\":[{\"name\":\"索引名\",\"note\":\"...\"}],\
         \"suggested_indexes\":[{\"name\":\"idx_xxx\",\"columns\":[\"a\",\"b\"],\"unique\":false,\"reason\":\"...\"}]}\n\
         要求：\n\
         1) 注释用简体中文；表注释不超过 60 字，字段注释不超过 40 字，直接说业务含义，不要复述类型。\n\
         2) column_comments 只放确实需要补或写得不准确的字段；字段名必须来自上面的列表，不要发明。\n\
         3) index_notes 为每个现有索引用一句话说明它服务什么查询。\n\
         4) suggested_indexes 只建议有明确价值的（外键列、常见等值/范围过滤列、需要唯一的组合），\
         不要与主键或现有索引重复，最多 5 条；没把握就给空数组。\n",
    );
    p
}

/// 从模型回复里抠出 JSON：容忍 ```json 代码块和前后废话
fn json_slice(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    Some(&text[start..=end])
}

/// 解析并做确定性校验：字段名不认识就丢掉，索引引用不存在的列或与现有索引重复也丢掉
pub fn parse_enrichment(
    text: &str,
    known_columns: &[String],
    existing_indexes: &[IndexInfo],
) -> Result<TableEnrichment, String> {
    let slice = json_slice(text).ok_or_else(|| {
        format!("模型没有返回 JSON，原始回复：{}", text.chars().take(300).collect::<String>())
    })?;
    let mut out: TableEnrichment = serde_json::from_str(slice)
        .map_err(|e| format!("JSON 解析失败：{e}；原始内容：{}", slice.chars().take(300).collect::<String>()))?;

    let known: HashSet<&str> = known_columns.iter().map(String::as_str).collect();
    out.column_comments.retain(|c| {
        let hit = known.contains(c.name.as_str());
        hit && !c.comment.trim().is_empty()
    });
    out.table_comment = out.table_comment.trim().to_string();

    let taken: HashSet<Vec<String>> = existing_indexes
        .iter()
        .map(|i| i.columns.clone())
        .collect();
    let mut seen: HashSet<Vec<String>> = HashSet::new();
    out.suggested_indexes.retain(|s| {
        !s.columns.is_empty()
            && s.columns.iter().all(|c| known.contains(c.as_str()))
            && !taken.contains(&s.columns)
            && seen.insert(s.columns.clone())
    });
    out.suggested_indexes.truncate(5);
    let names: HashSet<&str> = existing_indexes.iter().map(|i| i.name.as_str()).collect();
    for s in out.suggested_indexes.iter_mut() {
        if s.name.trim().is_empty() || names.contains(s.name.as_str()) {
            s.name = format!("idx_{}", s.columns.join("_"));
        }
    }
    out.index_notes
        .retain(|n| !n.note.trim().is_empty() && existing_indexes.iter().any(|i| i.name == n.name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: ty.into(),
            nullable: true,
            primary_key: false,
            default: None,
            comment: None,
            extra: None,
            max_length: None,
        }
    }

    fn idx(name: &str, cols: &[&str]) -> IndexInfo {
        IndexInfo {
            name: name.into(),
            columns: cols.iter().map(|s| s.to_string()).collect(),
            unique: false,
            primary: false,
        }
    }

    #[test]
    fn prompt_lists_columns_indexes_and_samples() {
        let cols = vec![col("id", "int"), col("status", "tinyint")];
        let mut c = cols[0].clone();
        c.primary_key = true;
        c.nullable = false;
        let cols = vec![c, cols[1].clone()];
        let head = vec!["id".to_string(), "status".to_string()];
        let rows = vec![vec![json!(1), json!("0")]];
        let shape = TableShape {
            kind: "mysql",
            database: "app",
            table: "orders",
            table_comment: None,
            columns: &cols,
            indexes: &[idx("idx_status", &["status"])],
            fks: &[],
            sample_header: &head,
            sample_rows: &rows,
        };
        let p = build_prompt(&shape);
        assert!(p.contains("引擎：mysql"));
        assert!(p.contains("表：app.orders"));
        assert!(p.contains("- id int 非空 主键 注释=（无）"));
        assert!(p.contains("- idx_status (status)"));
        assert!(p.contains("id | status"));
        assert!(p.contains("1 | 0"));
    }

    #[test]
    fn parses_fenced_json() {
        let cols: Vec<String> = ["id", "status", "user_id"].iter().map(|s| s.to_string()).collect();
        let text = "好的：\n```json\n{\n  \"table_comment\": \"订单主表\",\n  \"column_comments\": [{\"name\":\"status\",\"comment\":\"订单状态\"},{\"name\":\"ghost\",\"comment\":\"瞎写的\"}],\n  \"suggested_indexes\": [{\"name\":\"\",\"columns\":[\"user_id\"],\"unique\":false,\"reason\":\"按用户查订单\"},{\"name\":\"dup\",\"columns\":[\"status\"],\"reason\":\"和现有索引重复\"}]\n}\n```";
        let got = parse_enrichment(text, &cols, &[idx("idx_status", &["status"])]).unwrap();
        assert_eq!(got.table_comment, "订单主表");
        assert_eq!(got.column_comments.len(), 1);
        assert_eq!(got.column_comments[0].name, "status");
        // 只留一条：user_id 的索引，名字自动补出来
        assert_eq!(got.suggested_indexes.len(), 1);
        assert_eq!(got.suggested_indexes[0].columns, vec!["user_id".to_string()]);
        assert_eq!(got.suggested_indexes[0].name, "idx_user_id");
    }

    #[test]
    fn rejects_non_json_with_raw_text() {
        let cols = vec!["id".to_string()];
        let err = parse_enrichment("这张表看起来是订单表", &cols, &[]).unwrap_err();
        assert!(err.contains("模型没有返回 JSON"), "{err}");
        assert!(err.contains("订单表"), "{err}");
    }

    #[test]
    fn drops_index_notes_for_unknown_index() {
        let cols = vec!["id".to_string()];
        let text = json!({
            "index_notes": [{"name": "idx_status", "note": "按状态筛"}, {"name": "nope", "note": "瞎编"}]
        })
        .to_string();
        let got = parse_enrichment(&text, &cols, &[idx("idx_status", &["status"])]).unwrap();
        assert_eq!(got.index_notes.len(), 1);
        assert_eq!(got.index_notes[0].name, "idx_status");
    }
}
