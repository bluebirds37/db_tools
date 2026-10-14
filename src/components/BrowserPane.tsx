import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Table2,
  Play,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ListFilter,
  Zap,
  Plus,
  Pencil,
  Trash2,
  RefreshCw,
  Columns3,
  KeyRound,
  Link2,
  TerminalSquare,
  Search,
  X,
  Inbox,
  Loader2,
  Braces,
  CheckCircle2,
  MousePointerClick,
  AlertTriangle,
  Database,
} from "lucide-react";
import { useAppStore } from "../stores/appStore";
import {
  api,
  type ColumnInfo,
  type ConnectionConfig,
  type ForeignKeyInfo,
  type IndexInfo,
  type QueryPage,
  type TableInfo,
  type TablespaceInfo,
  type TableExtra,
} from "../services/api";
import { ConfirmDialog } from "./Modal";
import { TypePicker, defaultTypeOf } from "./fieldTypes";
import { TableDesigner } from "./TableDesigner";
import { DocumentEditor } from "./DocumentEditor";
import { ContextMenu, type ContextMenuState, type ContextMenuItem } from "./ContextMenu";
import { TransferDialog } from "./TransferDialog";
import { FilterSelect, fsOptions, type FsOption } from "./FilterSelect";

type Tab = "data" | "schema" | "query";

const SQL_KINDS = ["mysql", "postgres", "pgvector", "sqlite"];

/// 非关系引擎各自的取数入口：Redis 走命令，ES / Qdrant 走 REST，Mongo 走查询文档
const KIND_META: Record<
  string,
  { tab: string; placeholder: string; hint: string; schema: string }
> = {
  mysql: { tab: "SQL", placeholder: "SELECT * FROM `table` LIMIT 100;", hint: "Ctrl + Enter 执行", schema: "结构" },
  postgres: { tab: "SQL", placeholder: 'SELECT * FROM "table" LIMIT 100;', hint: "Ctrl + Enter 执行", schema: "结构" },
  pgvector: { tab: "SQL", placeholder: 'SELECT * FROM "table" LIMIT 100;', hint: "Ctrl + Enter 执行", schema: "结构" },
  sqlite: { tab: "SQL", placeholder: "SELECT * FROM table LIMIT 100;", hint: "Ctrl + Enter 执行", schema: "结构" },
  redis: {
    tab: "命令",
    placeholder: "GET user:1\nHSET user:1 name tom\nKEYS user:*",
    hint: "单条 Redis 命令直接执行；KEYS <pattern> 或只写 pattern 会以表格列出键",
    schema: "结构",
  },
  elasticsearch: {
    tab: "REST",
    placeholder: 'GET /_cat/indices?v\nPOST /my_index/_search {"query":{"match_all":{}}}',
    hint: "METHOD path [json] 走 REST；只写索引名或 索引|{json} 会执行 _search",
    schema: "映射",
  },
  qdrant: {
    tab: "REST",
    placeholder: 'GET /collections\nPOST /collections/my_col/points/scroll {"limit":20}',
    hint: "METHOD path [json] 走 REST；只写集合名会滚动列出点位",
    schema: "结构",
  },
  mongodb: {
    tab: "查询",
    placeholder: 'orders|{"status":1}\ndb.admin.{"ping":1}',
    hint: "集合|{过滤 JSON} 查询；db.<库>.{命令 JSON} 执行管理命令",
    schema: "字段",
  },
};

const KIND_DEFAULT = KIND_META.mysql;

/// 筛选条件的值控件与运算符集合按字段类型家族给（参考 Navicat 的筛选器）
type QFamily = "number" | "date" | "bool" | "text";

const familyOf = (t: string): QFamily => {
  const s = (t || "").toLowerCase();
  if (s.includes("bool")) return "bool";
  if (/(date|time|year)/.test(s)) return "date";
  if (/(int|num|dec|float|double|real|money|serial)/.test(s)) return "number";
  return "text";
};

type QOp = { v: string; label: string; noValue?: boolean; twoValues?: boolean };

const Q_OPS: Record<QFamily, QOp[]> = {
  text: [
    { v: "eq", label: "等于" },
    { v: "ne", label: "不等于" },
    { v: "like", label: "包含" },
    { v: "nlike", label: "不包含" },
    { v: "prefix", label: "开头是" },
    { v: "suffix", label: "结尾是" },
    { v: "in", label: "在列表中（逗号分隔）" },
    { v: "null", label: "为空", noValue: true },
    { v: "nnul", label: "不为空", noValue: true },
  ],
  number: [
    { v: "eq", label: "等于" },
    { v: "ne", label: "不等于" },
    { v: "gt", label: "大于" },
    { v: "gte", label: "大于等于" },
    { v: "lt", label: "小于" },
    { v: "lte", label: "小于等于" },
    { v: "between", label: "介于", twoValues: true },
    { v: "in", label: "在列表中（逗号分隔）" },
    { v: "null", label: "为空", noValue: true },
    { v: "nnul", label: "不为空", noValue: true },
  ],
  date: [
    { v: "eq", label: "等于" },
    { v: "gt", label: "晚于" },
    { v: "lt", label: "早于" },
    { v: "between", label: "介于", twoValues: true },
    { v: "null", label: "为空", noValue: true },
    { v: "nnul", label: "不为空", noValue: true },
  ],
  bool: [
    { v: "true", label: "为真", noValue: true },
    { v: "false", label: "为假", noValue: true },
    { v: "null", label: "为空", noValue: true },
    { v: "nnul", label: "不为空", noValue: true },
  ],
};

type QFilter = {
  id: number;
  col: string;
  logic: "and" | "or";
  op: string;
  val: string;
  val2: string;
  on: boolean;
};

type QSort = { id: number; col: string; dir: "asc" | "desc" };

const qfOp = (fam: QFamily, op: string) => Q_OPS[fam].find((o) => o.v === op);
const lit = (v: string) => `'${v.replace(/'/g, "''")}'`;
const numLit = (v: string) =>
  /^-?\d+(\.\d+)?$/.test(v.trim()) ? v.trim() : lit(v);

/// 这条草稿会不会真的产出子句：生成 SQL 与「应用」快照共用同一判断，计数才不会骗人
function qfEmits(f: QFilter, typeOf: (col: string) => string): boolean {
  if (!f.on || !f.col) return false;
  const meta = qfOp(familyOf(typeOf(f.col)), f.op);
  if (!meta) return false;
  const v = f.val.trim();
  if (!meta.noValue && !v) return false;
  if (meta.twoValues && !f.val2.trim()) return false;
  if (f.op === "in" && !v.split(/[,，]/).some((s) => s.trim())) return false;
  return true;
}

/// 逐条左折叠加括号，混用 and / or 时结果与界面顺序一致，不受 SQL 优先级影响
function buildWhereSql(
  kind: string,
  rows: QFilter[],
  typeOf: (col: string) => string
): string {
  let sql = "";
  for (const f of rows) {
    if (!qfEmits(f, typeOf)) continue;
    const fam = familyOf(typeOf(f.col));
    const v = f.val.trim();
    const c = ident(kind, f.col);
    const scalar =
      fam === "number" && !["like", "nlike"].includes(f.op) ? numLit : lit;
    let clause = "";
    switch (f.op) {
      case "eq": clause = `${c} = ${scalar(v)}`; break;
      case "ne": clause = `${c} <> ${scalar(v)}`; break;
      case "gt": clause = `${c} > ${scalar(v)}`; break;
      case "gte": clause = `${c} >= ${scalar(v)}`; break;
      case "lt": clause = `${c} < ${scalar(v)}`; break;
      case "lte": clause = `${c} <= ${scalar(v)}`; break;
      case "like": clause = `${c} LIKE ${lit(`%${v}%`)}`; break;
      case "nlike": clause = `${c} NOT LIKE ${lit(`%${v}%`)}`; break;
      case "prefix": clause = `${c} LIKE ${lit(`${v}%`)}`; break;
      case "suffix": clause = `${c} LIKE ${lit(`%${v}`)}`; break;
      case "between":
        clause = `${c} BETWEEN ${scalar(v)} AND ${scalar(f.val2)}`;
        break;
      case "in": {
        const items = v
          .split(/[,，]/)
          .map((s) => s.trim())
          .filter(Boolean);
        clause = `${c} IN (${items.map(scalar).join(", ")})`;
        break;
      }
      case "null": clause = `${c} IS NULL`; break;
      case "nnul": clause = `${c} IS NOT NULL`; break;
      case "true":
        clause = kind === "sqlite" ? `${c} = 1` : `${c} IS TRUE`;
        break;
      case "false":
        clause = kind === "sqlite" ? `${c} = 0` : `${c} IS FALSE`;
        break;
      default: continue;
    }
    sql = sql ? `(${sql} ${f.logic.toUpperCase()} ${clause})` : clause;
  }
  return sql ? ` WHERE ${sql}` : "";
}

function buildOrderSql(kind: string, sorts: QSort[]): string {
  const parts = sorts.filter((s) => s.col).map((s) => `${ident(kind, s.col)} ${s.dir.toUpperCase()}`);
  return parts.length ? ` ORDER BY ${parts.join(", ")}` : "";
}

/// ES 的元字段：只用于定位文档，不能写进 _source
const ES_META_FIELDS = [
  "_id",
  "_index",
  "_score",
  "_type",
  "_version",
  "_source",
  "_seq_no",
  "_primary_term",
  "_field_names",
  "_routing",
];

type Preset = { label: string; text: string; danger?: boolean };

/// 非 SQL 引擎的常用操作：{name} 会替换成当前选中的表/集合/索引，{db} 换成当前库
const ENGINE_PRESETS: Record<string, { group: string; items: Preset[] }[]> = {
  elasticsearch: [
    {
      group: "集群",
      items: [
        { label: "集群健康", text: "GET /_cluster/health" },
        { label: "节点列表", text: "GET /_cat/nodes?v" },
        { label: "索引一览", text: "GET /_cat/indices?v" },
        { label: "分片分布", text: "GET /_cat/shards/{name}?v" },
        { label: "产品与版本", text: "GET /" },
      ],
    },
    {
      group: "查询",
      items: [
        { label: "match_all 取 20 条", text: 'GET /{name}/_search\n{"query":{"match_all":{}},"size":20}' },
        { label: "按字段关键词", text: 'POST /{name}/_search\n{"query":{"match":{"_all":"关键字"}}}' },
        { label: "布尔组合", text: 'POST /{name}/_search\n{"query":{"bool":{"must":[{"match_all":{}}],"filter":[{"term":{"status":1}}]}}}' },
        { label: "文档计数", text: "GET /{name}/_count" },
        { label: "字段聚合", text: 'POST /{name}/_search\n{"size":0,"aggs":{"by_status":{"terms":{"field":"status","size":10}}}}' },
        { label: "按 id 取文档", text: "GET /{name}/_doc/1" },
        { label: "跨全部索引", text: 'POST /_search\n{"query":{"match_all":{}},"size":10}' },
      ],
    },
    {
      group: "写入",
      items: [
        { label: "指定 id 写入", text: 'PUT /{name}/_doc/1\n{"name":"tom","status":1}' },
        { label: "自动生成 id", text: 'POST /{name}/_doc\n{"name":"jerry","status":1}' },
        { label: "局部更新", text: 'POST /{name}/_update/1\n{"doc":{"status":2}}' },
        { label: "删除文档", text: "DELETE /{name}/_doc/1", danger: true },
        { label: "按条件删除", text: 'POST /{name}/_delete_by_query\n{"query":{"match_all":{}}}', danger: true },
        { label: "刷新索引", text: "POST /{name}/_refresh" },
      ],
    },
    {
      group: "映射",
      items: [
        { label: "查看映射", text: "GET /{name}/_mapping" },
        { label: "加字段", text: 'PUT /{name}/_mapping\n{"properties":{"remark":{"type":"keyword"}}}' },
        { label: "字段能力", text: "GET /_field_caps?index={name}&fields=*" },
      ],
    },
    {
      group: "索引管理",
      items: [
        { label: "建索引", text: 'PUT /{name}\n{"settings":{"number_of_shards":1,"number_of_replicas":0},"mappings":{"properties":{"id":{"type":"long"},"name":{"type":"keyword"}}}}' },
        { label: "索引设置", text: "GET /{name}/_settings" },
        { label: "改副本数", text: 'PUT /{name}/_settings\n{"index":{"number_of_replicas":0}}' },
        { label: "索引统计", text: "GET /{name}/_stats" },
        { label: "别名切换", text: 'POST /_aliases\n{"actions":[{"add":{"index":"{name}","alias":"{name}_latest"}}]}' },
        { label: "重建到副本", text: 'POST /_reindex\n{"source":{"index":"{name}"},"dest":{"index":"{name}_copy"}}' },
        { label: "删除索引", text: "DELETE /{name}", danger: true },
      ],
    },
  ],
  redis: [
    {
      group: "读",
      items: [
        { label: "列出键", text: "KEYS *" },
        { label: "键类型", text: "TYPE {name}" },
        { label: "取值", text: "GET {name}" },
        { label: "哈希全部", text: "HGETALL {name}" },
        { label: "列表区间", text: "LRANGE {name} 0 20" },
        { label: "集合成员", text: "SMEMBERS {name}" },
        { label: "有序集合", text: "ZRANGE {name} 0 20 WITHSCORES" },
        { label: "剩余过期", text: "TTL {name}" },
      ],
    },
    {
      group: "写",
      items: [
        { label: "设置字符串", text: "SET {name} value EX 300" },
        { label: "写哈希字段", text: "HSET {name} field value" },
        { label: "删除键", text: "DEL {name}", danger: true },
      ],
    },
    {
      group: "库信息",
      items: [
        { label: "当前库键数", text: "DBSIZE" },
        { label: "服务信息", text: "INFO" },
        { label: "内存使用", text: "INFO memory" },
      ],
    },
  ],
  qdrant: [
    {
      group: "集合",
      items: [
        { label: "集合列表", text: "GET /collections" },
        { label: "集合详情", text: "GET /collections/{name}" },
        { label: "点位计数", text: "GET /collections/{name}/count" },
        { label: "分片信息", text: "GET /collections/{name}/cluster-info" },
      ],
    },
    {
      group: "点位",
      items: [
        { label: "滚动取点", text: 'POST /collections/{name}/points/scroll\n{"limit":10,"with_payload":true}' },
        { label: "向量检索", text: 'POST /collections/{name}/points/search\n{"vector":[0.1,0.2,0.3],"limit":5,"with_payload":true}' },
        { label: "按 id 取点", text: 'POST /collections/{name}/points\n{"ids":[1,2]}' },
        { label: "删除点位", text: 'POST /collections/{name}/points/delete\n{"points":[1,2]}', danger: true },
      ],
    },
  ],
  mongodb: [
    {
      group: "查询",
      items: [
        { label: "全部文档", text: "{name}|{}" },
        { label: "条件过滤", text: '{name}|{"status":1}' },
        { label: "正则匹配", text: '{name}|{"name":{"$regex":"^a"}}' },
      ],
    },
    {
      group: "库命令",
      items: [
        { label: "集合统计", text: 'db.{db}.{"collStats":"{name}"}' },
        { label: "列出集合", text: 'db.{db}.{"listCollections":1}' },
        { label: "服务状态", text: 'db.admin.{"serverStatus":1}' },
        { label: "运行 ping", text: "db.admin.{\"ping\":1}" },
      ],
    },
  ],
};

type ColumnDraft = {
  name: string;
  data_type: string;
  nullable: boolean;
  default: string;
  comment: string;
};

const quoteOf = (kind: string) => (kind === "mysql" ? "`" : '"');

function sqlDefault(raw: string) {
  const def = raw.trim();
  return /^(current_timestamp|now\(\)|null|true|false|-?\d+(\.\d+)?)$/i.test(def)
    ? def
    : `'${def.replace(/'/g, "''")}'`;
}

/// 按引擎拼出修改一列所需的 DDL；返回空数组表示没有可执行的变更
function columnAlterSqls(
  kind: string,
  table: string,
  col: ColumnInfo,
  next: ColumnDraft
): string[] {
  const q = quoteOf(kind);
  const id = (n: string) => `${q}${n}${q}`;
  const t = id(table);
  const newName = next.name.trim();
  const type = next.data_type.trim();
  const renamed = !!newName && newName !== col.name;
  const target = renamed ? newName : col.name;
  const out: string[] = [];

  if (kind === "mysql") {
    const parts = [id(target), type || col.data_type];
    parts.push(next.nullable ? "NULL" : "NOT NULL");
    const def = next.default.trim();
    const origDef = (col.default ?? "").trim();
    // 读回来的默认值已经是可用的字面量形式，没改过就原样回填，避免二次加引号
    if (def) parts.push(`DEFAULT ${def === origDef ? def : sqlDefault(def)}`);
    else if (origDef) parts.push("DEFAULT NULL");
    if (col.extra) parts.push(col.extra);
    if (next.comment.trim() || col.comment)
      parts.push(`COMMENT '${next.comment.replace(/'/g, "''")}'`);
    out.push(
      `ALTER TABLE ${t} ${renamed ? "CHANGE COLUMN" : "MODIFY COLUMN"} ${id(col.name)} ${parts.join(" ")}`
    );
    return out;
  }

  // SQLite 改类型 / 约束需要整表重建，这里只做重命名，避免静默丢数据
  if (kind === "sqlite") {
    if (renamed)
      out.push(`ALTER TABLE ${t} RENAME COLUMN ${id(col.name)} TO ${id(newName)}`);
    return out;
  }

  if (renamed) out.push(`ALTER TABLE ${t} RENAME COLUMN ${id(col.name)} TO ${id(newName)}`);
  if (type && type.toUpperCase() !== col.data_type.toUpperCase())
    out.push(`ALTER TABLE ${t} ALTER COLUMN ${id(target)} TYPE ${type}`);
  if (next.nullable !== col.nullable)
    out.push(
      `ALTER TABLE ${t} ALTER COLUMN ${id(target)} ${next.nullable ? "DROP NOT NULL" : "SET NOT NULL"}`
    );
  const def = next.default.trim();
  if (def !== (col.default ?? "").trim())
    out.push(
      `ALTER TABLE ${t} ALTER COLUMN ${id(target)} ${def ? `SET DEFAULT ${sqlDefault(def)}` : "DROP DEFAULT"}`
    );
  if (next.comment !== (col.comment ?? ""))
    out.push(`COMMENT ON COLUMN ${t}.${id(target)} IS '${next.comment.replace(/'/g, "''")}'`);
  return out;
}

type IndexDraft = { name: string; columns: string; unique: boolean };
type FkDraft = {
  name: string;
  column: string;
  refTable: string;
  refColumn: string;
  onDelete: string;
  onUpdate: string;
};

const ident = (kind: string, n: string) => `${quoteOf(kind)}${n}${quoteOf(kind)}`;

/// PG 的非 public schema 必须限定，否则 search_path 会指到 public 上去
function qualifiedTable(kind: string, schema: string, table: string) {
  if ((kind === "postgres" || kind === "pgvector") && schema && schema !== "public")
    return `${ident(kind, schema)}.${ident(kind, table)}`;
  return ident(kind, table);
}

const slug = (s: string) => s.replace(/[^0-9a-zA-Z_]/g, "_");

const REF_ACTIONS_MYSQL = ["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"];
const REF_ACTIONS_PG = ["RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT", "NO ACTION"];
const REF_ACTIONS = [
  { field: "onDelete", label: "ON DELETE" },
  { field: "onUpdate", label: "ON UPDATE" },
] as const;

function autoIndexName(table: string, cols: string[]) {
  return `idx_${slug(table)}_${cols.map(slug).join("_")}`.slice(0, 60);
}
function autoFkName(table: string, col: string) {
  return `fk_${slug(table)}_${slug(col)}`.slice(0, 60);
}

function createIndexSql(kind: string, schema: string, table: string, d: IndexDraft, cols: string[]) {
  const name = d.name.trim() || autoIndexName(table, cols);
  const list = cols.map((c) => ident(kind, c)).join(", ");
  const t = qualifiedTable(kind, schema, table);
  return `CREATE ${d.unique ? "UNIQUE " : ""}INDEX ${ident(kind, name)} ON ${t} (${list})`;
}

function dropIndexSql(kind: string, schema: string, table: string, name: string) {
  if (kind === "mysql")
    return `ALTER TABLE ${ident(kind, table)} DROP INDEX ${ident(kind, name)}`;
  return `DROP INDEX ${qualifiedTable(kind, schema, name)}`;
}

function addFkSql(kind: string, schema: string, table: string, d: FkDraft) {
  const name = d.name.trim() || autoFkName(table, d.column.trim());
  const acts = `${d.onDelete ? ` ON DELETE ${d.onDelete}` : ""}${
    d.onUpdate ? ` ON UPDATE ${d.onUpdate}` : ""
  }`;
  return (
    `ALTER TABLE ${qualifiedTable(kind, schema, table)} ADD CONSTRAINT ${ident(kind, name)} ` +
    `FOREIGN KEY (${ident(kind, d.column.trim())}) REFERENCES ` +
    `${qualifiedTable(kind, schema, d.refTable.trim())} (${ident(kind, d.refColumn.trim())})${acts}`
  );
}

function dropFkSql(kind: string, schema: string, table: string, name: string) {
  const t = qualifiedTable(kind, schema, table);
  return kind === "mysql"
    ? `ALTER TABLE ${t} DROP FOREIGN KEY ${ident(kind, name)}`
    : `ALTER TABLE ${t} DROP CONSTRAINT ${ident(kind, name)}`;
}

/// Mongo 没有 DDL，索引管理走 run_command
function mongoIndexCreate(db: string, coll: string, name: string, cols: string[], unique: boolean) {
  const key = cols.map((c) => `"${c}":1`).join(",");
  return `db.${db}.{"createIndexes":"${coll}","indexes":[{"key":{${key}},"name":"${name}"${
    unique ? ',"unique":true' : ""
  }}]}`;
}
function mongoIndexDrop(db: string, coll: string, name: string) {
  return `db.${db}.{"dropIndexes":"${coll}","index":"${name}"}`;
}

/// Mongo 没有列级 DDL，字段级操作用 update 命令批量落到文档上
function mongoSetField(db: string, coll: string, field: string, valueJson: string) {
  return `db.${db}.${JSON.stringify({
    update: coll,
    updates: [{ q: {}, u: { $set: { [field]: JSON.parse(valueJson) } }, multi: true }],
  })}`;
}
function mongoRenameField(db: string, coll: string, from: string, to: string) {
  return `db.${db}.${JSON.stringify({
    update: coll,
    updates: [{ q: {}, u: { $rename: { [from]: to } }, multi: true }],
  })}`;
}
function mongoUnsetField(db: string, coll: string, field: string) {
  return `db.${db}.${JSON.stringify({
    update: coll,
    updates: [{ q: {}, u: { $unset: { [field]: "" } }, multi: true }],
  })}`;
}
function esMappingField(index: string, field: string, type: string) {
  return `PUT /${index}/_mapping ${JSON.stringify({ properties: { [field]: { type } } })}`;
}

/// Mongo 的字段值要按类型落成合法的 JSON 片段
function mongoValueJson(type: string, raw: string): string {
  const t = raw.trim();
  switch (type) {
    case "int":
    case "long":
      return String(Number.isFinite(Number(t)) ? Math.trunc(Number(t)) : 0);
    case "double":
    case "decimal":
      return String(Number.isFinite(Number(t)) ? Number(t) : 0);
    case "bool":
      return /^(1|true|yes)$/i.test(t) ? "true" : "false";
    case "null":
      return "null";
    case "date":
      return JSON.stringify({ $date: t || new Date().toISOString() });
    case "objectId":
      return JSON.stringify({ $oid: t });
    case "array":
    case "object":
      try {
        return JSON.stringify(JSON.parse(t || (type === "array" ? "[]" : "{}")));
      } catch {
        return type === "array" ? "[]" : "{}";
      }
    default:
      return JSON.stringify(raw);
  }
}

export function BrowserPane({
  connection,
  database,
  onTransfer,
}: {
  connection: ConnectionConfig | null;
  database: string;
  onTransfer?: (
    mode:
      | "export"
      | "dump-table"
      | "dump-db"
      | "sync"
      | "sync-db"
      | "import"
      | "run-sql",
    conn: ConnectionConfig,
    db: string,
    table?: string | null
  ) => void;
}) {
  const { activeTable, setActiveTable, setError, error } = useAppStore();
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [tablesLoading, setTablesLoading] = useState(false);
  const [tab, setTab] = useState<Tab>("data");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(50);
  const [data, setData] = useState<QueryPage | null>(null);
  const [columns, setColumns] = useState<ColumnInfo[]>([]);
  const [indexes, setIndexes] = useState<IndexInfo[]>([]);
  const [fks, setFks] = useState<ForeignKeyInfo[]>([]);
  const [tablespaces, setTablespaces] = useState<TablespaceInfo[]>([]);
  const [tableExtra, setTableExtra] = useState<TableExtra | null>(null);
  const [sql, setSql] = useState("");
  const [sqlResult, setSqlResult] = useState<QueryPage | null>(null);
  const [execMsg, setExecMsg] = useState<string | null>(null);
  const [execOut, setExecOut] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [editRow, setEditRow] = useState<Record<string, unknown> | null>(null);
  const [editPk, setEditPk] = useState<Record<string, unknown> | null>(null);
  const [insertOpen, setInsertOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<unknown[] | null>(null);
  const [ctxMenu, setCtxMenu] = useState<ContextMenuState | null>(null);
  const [insertData, setInsertData] = useState<Record<string, string>>({});
  const [tableFilter, setTableFilter] = useState("");
  const [schemas, setSchemas] = useState<string[]>([]);
  const [activeSchema, setActiveSchema] = useState<string>("");
  const [pageJump, setPageJump] = useState("1");
  const [sqlHistory, setSqlHistory] = useState<string[]>([]);
  const [transferMode, setTransferMode] = useState<"export"|"dump-table"|"dump-db"|"sync"|"sync-db"|null>(null);
  const [objectOpen, setObjectOpen] = useState(false);
  const [docOpen, setDocOpen] = useState(false);
  const [schemaNewCol, setSchemaNewCol] = useState({
    name: "",
    type: "TEXT",
    notNull: false,
    default: "",
  });
  const [colEdit, setColEdit] = useState<{ origin: string; draft: ColumnDraft } | null>(null);
  const [colSaving, setColSaving] = useState(false);
  const [newIdx, setNewIdx] = useState<IndexDraft>({ name: "", columns: "", unique: false });
  const [newFk, setNewFk] = useState<FkDraft>({
    name: "",
    column: "",
    refTable: "",
    refColumn: "",
    onDelete: "",
    onUpdate: "",
  });
  const [ddlBusy, setDdlBusy] = useState(false);
  const [refCols, setRefCols] = useState<string[]>([]);
  const [refColsLoading, setRefColsLoading] = useState(false);
  const refReq = useRef("");
  const { connections: allConns } = useAppStore();

  const kind = connection?.kind || "";
  const isSqlKind = SQL_KINDS.includes(kind);
  const kindMeta = KIND_META[kind] || KIND_DEFAULT;
  const presets = ENGINE_PRESETS[kind] || [];
  const presetCount = presets.reduce((n, g) => n + g.items.length, 0);
  const [presetsOpen, setPresetsOpen] = useState(true);
  const sqlRef = useRef<HTMLTextAreaElement | null>(null);
  const qfSeq = useRef(1);
  const [qfOpen, setQfOpen] = useState(false);
  const [qfRows, setQfRows] = useState<QFilter[]>([]);
  const [qSorts, setQSorts] = useState<QSort[]>([]);
  // 点「应用」时快照一份，改草稿不会偷偷重查
  const [qApplied, setQApplied] = useState<{ rows: QFilter[]; sorts: QSort[] }>({
    rows: [],
    sorts: [],
  });
  // Redis 的「表」是键类型分组，没有可创建的对象
  const newObjectLabel =
    kind === "elasticsearch"
      ? "新建索引"
      : kind === "mongodb"
        ? "新建集合"
        : kind === "qdrant"
          ? "新建向量集合"
          : kind === "redis"
            ? ""
            : "新建表";
  const docKind = kind === "elasticsearch" || kind === "mongodb";
  const showSchema = kind !== "redis";

  // 响应可能晚于下一次选择到达，用它丢弃过期结果
  const dbKey = `${connection?.id || ""}::${database || ""}`;
  const selectionKey = `${dbKey}::${activeSchema}`;
  const liveDbKey = useRef(dbKey);
  const liveKey = useRef(selectionKey);
  useEffect(() => {
    liveDbKey.current = dbKey;
    liveKey.current = selectionKey;
  }, [dbKey, selectionKey]);

  // 右侧加载页的等待秒数
  const [paneWaitSec, setPaneWaitSec] = useState(0);
  useEffect(() => {
    if (!tablesLoading) {
      setPaneWaitSec(0);
      return;
    }
    const start = Date.now();
    setPaneWaitSec(0);
    const id = setInterval(
      () => setPaneWaitSec(Math.floor((Date.now() - start) / 1000)),
      1000
    );
    return () => clearInterval(id);
  }, [tablesLoading]);

  const loadTables = useCallback(async () => {
    if (!connection || !database) return;
    setTablesLoading(true);
    try {
      const list = await api.listTables(
        connection.id,
        database,
        activeSchema || null
      );
      if (liveKey.current !== selectionKey) return;
      setTables(list);
    } catch (e) {
      if (liveKey.current !== selectionKey) return;
      setError(String(e));
      setTables([]);
    } finally {
      if (liveKey.current === selectionKey) setTablesLoading(false);
    }
  }, [connection, database, activeSchema, selectionKey, setError]);

  const loadSchemas = useCallback(async () => {
    if (!connection || !database) return;
    if (connection.kind !== "postgres" && connection.kind !== "pgvector") {
      setSchemas([]);
      return;
    }
    try {
      const list = await api.listSchemas(connection.id, database);
      if (liveDbKey.current !== dbKey) return;
      setSchemas(list);
      setActiveSchema((cur) => {
        if (cur && list.includes(cur)) return cur;
        if (list.includes("public")) return "public";
        return list[0] || "";
      });
    } catch {
      if (liveDbKey.current !== dbKey) return;
      setSchemas([]);
    }
  }, [connection, database, dbKey]);

  const loadPage = useCallback(async () => {
    if (!connection || !database || !activeTable) return;
    setLoading(true);
    try {
      const kind = connection.kind;
      let query = activeTable;
      if (kind === "mysql") {
        query = `SELECT * FROM \`${database}\`.\`${activeTable}\``;
      } else if (kind === "postgres" || kind === "pgvector") {
        const sch = activeSchema || "public";
        query = `SELECT * FROM "${sch}"."${activeTable}"`;
      } else if (kind === "sqlite") {
        query = `SELECT * FROM "${activeTable}"`;
      }
      if (SQL_KINDS.includes(kind)) {
        const typeOf = (col: string) =>
          columns.find((c) => c.name === col)?.data_type || "";
        query +=
          buildWhereSql(kind, qApplied.rows, typeOf) +
          buildOrderSql(kind, qApplied.sorts);
      }
      const res = await api.queryPage(connection.id, database, query, page, pageSize);
      setData(res);
    } catch (e) {
      setError(String(e));
      setData(null);
    } finally {
      setLoading(false);
    }
  }, [connection, database, activeTable, activeSchema, page, pageSize, setError, columns, qApplied]);

  const colTypeOf = (c: string) =>
    columns.find((x) => x.name === c)?.data_type || "";

  const qfColumnOptions: FsOption[] = columns.length
    ? columns.map((c) => ({ value: c.name, label: c.name, hint: c.data_type }))
    : (data?.columns || []).map((c) => ({ value: c, label: c }));

  const addQFilter = () => {
    if (columns.length === 0) loadSchema();
    const first = qfColumnOptions[0]?.value || "";
    setQfRows((list) => [
      ...list,
      {
        id: qfSeq.current++,
        col: first,
        logic: "and",
        op: "eq",
        val: "",
        val2: "",
        on: true,
      },
    ]);
    setQfOpen(true);
  };

  const patchQFilter = (id: number, patch: Partial<QFilter>) =>
    setQfRows((list) => list.map((r) => (r.id === id ? { ...r, ...patch } : r)));

  const removeQFilter = (id: number) =>
    setQfRows((list) => list.filter((r) => r.id !== id));

  const addQSort = () => {
    if (columns.length === 0) loadSchema();
    setQSorts((list) => [
      ...list,
      { id: qfSeq.current++, col: qfColumnOptions[0]?.value || "", dir: "asc" },
    ]);
    setQfOpen(true);
  };

  const patchQSort = (id: number, patch: Partial<QSort>) =>
    setQSorts((list) => list.map((s) => (s.id === id ? { ...s, ...patch } : s)));

  const removeQSort = (id: number) =>
    setQSorts((list) => list.filter((s) => s.id !== id));

  const applyQuery = () => {
    const kind = connection?.kind || "";
    const rows = qfRows.filter((r) => qfEmits(r, colTypeOf));
    const sorts = qSorts.filter((s) => s.col);
    const w = buildWhereSql(kind, rows, colTypeOf);
    const o = buildOrderSql(kind, sorts);
    setQApplied({ rows, sorts });
    setPage(1);
    // 全是没填值的行时不会产出任何子句，给一句提示免得看着像没反应
    if (!w && !o && (qfRows.length || qSorts.length))
      setExecMsg("没有可应用的条件：请勾选启用、选好字段并填写值");
  };

  const clearQuery = () => {
    setQfRows([]);
    setQSorts([]);
    setQApplied({ rows: [], sorts: [] });
    setPage(1);
  };

  /// 点表头循环：升序 → 降序 → 取消，并立即生效
  const toggleHeaderSort = (col: string) => {
    if (!isSqlKind) return;
    const cur = qSorts.findIndex((s) => s.col === col);
    let next: QSort[];
    if (cur < 0) next = [...qSorts, { id: qfSeq.current++, col, dir: "asc" }];
    else if (qSorts[cur].dir === "asc")
      next = qSorts.map((s, i) => (i === cur ? { ...s, dir: "desc" } : s));
    else next = qSorts.filter((_, i) => i !== cur);
    setQSorts(next);
    setQApplied((a) => ({ rows: a.rows, sorts: next }));
    setPage(1);
  };

  const loadSchema = useCallback(async () => {
    if (!connection || !database || !activeTable) return;
    try {
      const [cols, idx, fk, extra] = await Promise.all([
        api.listColumns(connection.id, database, activeTable, activeSchema || null),
        api.listIndexes(connection.id, database, activeTable, activeSchema || null).catch(() => []),
        api.listForeignKeys(connection.id, database, activeTable, activeSchema || null).catch(() => []),
        api
          .describeTableExtra(connection.id, database, activeTable, activeSchema || null)
          .catch(() => null),
      ]);
      setColumns(cols);
      setIndexes(idx);
      setFks(fk);
      setTableExtra(extra);
    } catch (e) {
      setError(String(e));
    }
  }, [connection, database, activeTable, setError]);

  const loadTablespaces = useCallback(async () => {
    if (!connection) return;
    if (connection.kind !== "postgres" && connection.kind !== "pgvector") {
      setTablespaces([]);
      return;
    }
    try {
      const list = await api.listTablespaces(connection.id);
      setTablespaces(list);
    } catch (e) {
      setError(`加载表空间失败: ${String(e)}`);
      setTablespaces([]);
    }
  }, [connection, setError]);

  useEffect(() => {
    setActiveSchema("");
    void loadSchemas();
  }, [connection?.id, database]);

  useEffect(() => {
    loadTables();
    loadTablespaces();
  }, [loadTables, loadTablespaces]);

  useEffect(() => {
    setPage(1);
    setQfRows([]);
    setQSorts([]);
    setQApplied({ rows: [], sorts: [] });
    setQfOpen(false);
  }, [activeTable, database, connection?.id, activeSchema]);

  // 切换连接或库时立刻丢掉上一个库的残留内容，避免停留在旧库界面
  useEffect(() => {
    setError(null);
    setSqlResult(null);
    setExecMsg(null);
    setData(null);
    setTables([]);
    setTablesLoading(Boolean(connection?.id && database));
    setSchemas([]);
    setTableFilter("");
    setColumns([]);
    setIndexes([]);
    setFks([]);
    setTableExtra(null);
    setActiveTable(null);
    const key = `sql_hist_${connection?.id || "x"}::${database || ""}`;
    try {
      const raw = localStorage.getItem(key);
      setSqlHistory(raw ? (JSON.parse(raw) as string[]) : []);
    } catch {
      setSqlHistory([]);
    }
  }, [connection?.id, database, setError, setActiveTable]);

  // 表空间属于集群级：只在换连接时清，换库时保留（换库不会重新拉取）
  useEffect(() => {
    setTablespaces([]);
  }, [connection?.id]);

  const pushSqlHistory = (statement: string) => {
    if (!connection || !statement.trim()) return;
    const key = `sql_hist_${connection.id}::${database || ""}`;
    setSqlHistory((prev) => {
      const next = [statement.trim(), ...prev.filter((s) => s !== statement.trim())].slice(0, 50);
      localStorage.setItem(key, JSON.stringify(next));
      return next;
    });
  };
  const removeSqlHistory = (idx: number) => {
    setSqlHistory((prev) => {
      const next = prev.filter((_, i) => i !== idx);
      localStorage.setItem(`sql_hist_${connection?.id || "x"}::${database || ""}`, JSON.stringify(next));
      return next;
    });
  };
  const clearSqlHistory = () => {
    setSqlHistory([]);
    localStorage.removeItem(`sql_hist_${connection?.id || "x"}::${database || ""}`);
  };

  useEffect(() => {
    if (tab === "data") loadPage();
    if (tab === "schema") loadSchema();
  }, [tab, loadPage, loadSchema]);

  // 换到没有该页签的引擎时退回数据页
  useEffect(() => {
    if (tab === "schema" && !showSchema) setTab("data");
  }, [tab, showSchema]);

  // 换表 / 换 schema 后字段编辑态就失效了
  useEffect(() => {
    setColEdit(null);
    setNewIdx({ name: "", columns: "", unique: false });
    setNewFk({
      name: "",
      column: "",
      refTable: "",
      refColumn: "",
      onDelete: "",
      onUpdate: "",
    });
    setRefCols([]);
    refReq.current = "";
  }, [activeTable, activeSchema, database]);

  /// 各引擎的输入分流：SQL 看动词，其余按各自的语法形态判断
  const classifyInput = (input: string): "exec" | "query" => {
    const s = input.trim();
    if (isSqlKind) {
      return /^\s*(insert|update|delete|drop|create|alter|truncate|replace|set|use)\b/i.test(s)
        ? "exec"
        : "query";
    }
    if (kind === "redis") {
      if (/^\s*keys\s+\S+/i.test(s)) return "query";
      if (/^(ping|echo|dbsize|info|randomkey|time|client|command)\b/i.test(s)) return "exec";
      // 带参数的是命令；单个 token 当作 key pattern 浏览
      return s.includes(" ") ? "exec" : "query";
    }
    if (kind === "mongodb") return /^db\./i.test(s) ? "exec" : "query";
    if (kind === "elasticsearch" || kind === "qdrant")
      return /^(get|post|put|delete|head)\s+\S/i.test(s) ? "exec" : "query";
    return "query";
  };

  /// 点常用操作：把 {name}/{db} 换成当前对象后填进输入框，不自动执行
  const applyPreset = (text: string) => {
    const fallbackName =
      kind === "redis"
        ? "my:key"
        : kind === "mongodb"
          ? "orders"
          : kind === "qdrant"
            ? "my_collection"
            : "my_index";
    const filled = text
      .replace(/\{name\}/g, activeTable || fallbackName)
      .replace(/\{db\}/g, database || "admin");
    setSql(filled);
    requestAnimationFrame(() => {
      const el = sqlRef.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(filled.length, filled.length);
    });
  };

  const runSql = async () => {
    if (!connection || !sql.trim()) return;
    setLoading(true);
    setExecMsg(null);
    setExecOut(null);
    setSqlResult(null);
    try {
      const trimmed = sql.trim();
      if (classifyInput(trimmed) === "exec") {
        const res = await api.executeSql(connection.id, database, trimmed);
        const msg = res.message || `影响 ${res.affected_rows} 行`;
        setExecMsg(isSqlKind ? msg : "已执行");
        if (!isSqlKind) setExecOut(msg);
        pushSqlHistory(trimmed);
        if (activeTable && isSqlKind) loadPage();
      } else {
        const res = await api.queryPage(connection.id, database, trimmed, 1, 200);
        setSqlResult(res);
        pushSqlHistory(trimmed);
      }
    } catch (e) {
      setError(String(e));
      pushSqlHistory(sql.trim());
    } finally {
      setLoading(false);
    }
  };

  const pkCols = columns.filter((c) => c.primary_key).map((c) => c.name);

  /// 文本型列不能按数字猜，否则 "007" 会变成 7、纯数字的 Redis key 会被写成数字
  const TEXTISH = /(char|text|clob|string|uuid|json|binary|blob|bytea|enum|path)/i;
  const toCellValue = (colName: string, raw: string): unknown => {
    if (raw === "null") return null;
    const col = columns.find((c) => c.name === colName);
    if (col && TEXTISH.test(col.data_type)) return raw;
    const n = Number(raw);
    return raw.trim() !== "" && !Number.isNaN(n) ? n : raw;
  };

  /// Redis 的「表」是键类型分组，新写入的键通常不在当前页，按 key 精确读回来回显
  const echoRedisKey = async (key: string) => {
    if (!connection || !key) return false;
    try {
      const res = await api.queryPage(connection.id, database, key, 1, 20);
      if (res.rows.length > 0) {
        setData(res);
        return true;
      }
    } catch {
      /* 读回失败交给下一次的列表刷新反馈 */
    }
    return false;
  };
  const rowToRecord = (row: unknown[]): Record<string, unknown> => {
    const obj: Record<string, unknown> = {};
    data?.columns.forEach((c, i) => {
      obj[c] = row[i];
    });
    return obj;
  };
  const extractPk = (rec: Record<string, unknown>) => {
    const pk: Record<string, unknown> = {};
    if (pkCols.length) {
      pkCols.forEach((k) => {
        pk[k] = rec[k];
      });
    } else {
      const first = data?.columns[0];
      if (first) pk[first] = rec[first];
    }
    return pk;
  };

  const doDelete = async (row: unknown[]) => {
    if (!connection || !activeTable) return;
    setPendingDelete(row);
  };

  const confirmDelete = async () => {
    if (!connection || !activeTable || !pendingDelete) return;
    try {
      const rec = rowToRecord(pendingDelete);
      const pk = extractPk(rec);
      const res = await api.deleteRow(connection.id, database, activeTable, pk);
      setPendingDelete(null);
      loadPage();
      setExecMsg(res.message || "已删除");
    } catch (e) {
      setError(String(e));
      setPendingDelete(null);
    }
  };

  const doInsert = async () => {
    if (!connection || !activeTable) return;
    try {
      const payload: Record<string, unknown> = {};
      Object.entries(insertData).forEach(([k, v]) => {
        if (v === "" || v === undefined) return;
        payload[k] = toCellValue(k, v);
      });
      const res = await api.insertRow(connection.id, database, activeTable, payload);
      setInsertOpen(false);
      setInsertData({});
      const key = typeof payload.key === "string" ? payload.key : "";
      const echoed = connection.kind === "redis" ? await echoRedisKey(key) : false;
      if (!echoed) loadPage();
      setExecMsg(res.message || "已插入 1 行");
    } catch (e) {
      setError(String(e));
    }
  };

  const isEsMeta = (k: string) =>
    kind === "elasticsearch" && ES_META_FIELDS.includes(k);

  const doUpdate = async () => {
    if (!connection || !activeTable || !editRow || !editPk) return;
    try {
      const payload: Record<string, unknown> = {};
      Object.entries(editRow).forEach(([k, v]) => {
        if (isEsMeta(k)) return;
        payload[k] = v;
      });
      const res = await api.updateRow(
        connection.id,
        database,
        activeTable,
        editPk,
        payload
      );
      const key = typeof editPk.key === "string" ? editPk.key : "";
      const echoed = connection.kind === "redis" ? await echoRedisKey(key) : false;
      setEditRow(null);
      setEditPk(null);
      if (!echoed) loadPage();
      setExecMsg(res.message || "已更新");
    } catch (e) {
      setError(String(e));
    }
  };

  const startColEdit = (c: ColumnInfo) =>
    setColEdit({
      origin: c.name,
      draft: {
        name: c.name,
        data_type: c.data_type,
        nullable: c.nullable,
        default: c.default ?? "",
        comment: c.comment ?? "",
      },
    });

  const patchDraft = (patch: Partial<ColumnDraft>) =>
    setColEdit((cur) => (cur ? { ...cur, draft: { ...cur.draft, ...patch } } : cur));

  const saveColumn = async () => {
    if (!connection || !activeTable || !colEdit) return;
    const col = columns.find((c) => c.name === colEdit.origin);
    if (!col) return;
    const to = colEdit.draft.name.trim();

    // Mongo 的「编辑」就是把字段在所有文档里改名
    if (connection.kind === "mongodb") {
      setColEdit(null);
      if (!to || to === col.name) return;
      await runDdl(
        mongoRenameField(database, activeTable, col.name, to),
        `已重命名字段 ${col.name} → ${to}`
      );
      return;
    }

    const stmts = columnAlterSqls(connection.kind, activeTable, col, colEdit.draft);
    if (!stmts.length) {
      setColEdit(null);
      return;
    }
    setColSaving(true);
    setError(null);
    try {
      for (const stmt of stmts) await api.executeSql(connection.id, database, stmt);
      setExecMsg(`已更新字段 ${colEdit.draft.name.trim() || col.name}`);
      setColEdit(null);
      loadSchema();
      loadTables();
    } catch (e) {
      setError(String(e));
    } finally {
      setColSaving(false);
    }
  };

  const runDdl = async (stmt: string, okMsg: string) => {
    if (!connection) return;
    setDdlBusy(true);
    setError(null);
    try {
      await api.executeSql(connection.id, database, stmt);
      setExecMsg(okMsg);
      loadSchema();
    } catch (e) {
      setError(String(e));
    } finally {
      setDdlBusy(false);
    }
  };

  const submitNewColumn = async () => {
    if (!connection || !activeTable) return;
    const name = schemaNewCol.name.trim();
    if (!name) {
      setError("请填写字段名");
      return;
    }
    const kindNow = connection.kind;
    const type = schemaNewCol.type.trim() || defaultTypeOf(kindNow);
    let stmt: string;
    let ok: string;
    if (kindNow === "mongodb") {
      stmt = mongoSetField(
        database,
        activeTable,
        name,
        mongoValueJson(type, schemaNewCol.default)
      );
      ok = `已为全部文档添加字段 ${name}`;
    } else if (kindNow === "elasticsearch") {
      stmt = esMappingField(activeTable, name, type);
      ok = `已添加映射字段 ${name}（${type}）`;
    } else {
      stmt =
        `ALTER TABLE ${qualifiedTable(kindNow, activeSchema, activeTable)} ADD COLUMN ` +
        `${ident(kindNow, name)} ${type}${schemaNewCol.notNull ? " NOT NULL" : ""}`;
      ok = `已添加字段 ${name}`;
    }
    await runDdl(stmt, ok);
    setSchemaNewCol({ name: "", type, notNull: false, default: "" });
  };

  const dropField = async (c: ColumnInfo) => {
    if (!connection || !activeTable) return;
    const label =
      connection.kind === "mongodb"
        ? `从全部文档中移除字段 ${c.name}`
        : `删除字段 ${c.name}`;
    if (!confirm(`${label}？`)) return;
    const stmt =
      connection.kind === "mongodb"
        ? mongoUnsetField(database, activeTable, c.name)
        : `ALTER TABLE ${qualifiedTable(connection.kind, activeSchema, activeTable)} DROP COLUMN ${ident(
            connection.kind,
            c.name
          )}`;
    await runDdl(stmt, `已移除字段 ${c.name}`);
  };

  const idxCols = newIdx.columns
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

  const submitIndex = async () => {
    if (!connection || !activeTable) return;
    const cols = idxCols;
    if (!cols.length) {
      setError("请至少选择一个索引字段");
      return;
    }
    const name = newIdx.name.trim() || autoIndexName(activeTable, cols);
    const stmt =
      connection.kind === "mongodb"
        ? mongoIndexCreate(database, activeTable, name, cols, newIdx.unique)
        : createIndexSql(connection.kind, activeSchema, activeTable, { ...newIdx, name }, cols);
    await runDdl(stmt, `已创建索引 ${name}`);
    setNewIdx({ name: "", columns: "", unique: false });
  };

  const loadRefColumns = async (t: string) => {
    const token = `${database}::${activeSchema}::${t}`;
    refReq.current = token;
    setRefCols([]);
    setRefColsLoading(false);
    if (!connection || !t) return;
    setRefColsLoading(true);
    try {
      const cols = await api.listColumns(connection.id, database, t, activeSchema || null);
      if (refReq.current !== token) return;
      setRefCols(cols.map((c) => c.name));
    } catch (e) {
      if (refReq.current !== token) return;
      setError(String(e));
    } finally {
      if (refReq.current === token) setRefColsLoading(false);
    }
  };

  const submitFk = async () => {    if (!connection || !activeTable) return;
    if (!newFk.column.trim() || !newFk.refTable.trim() || !newFk.refColumn.trim()) {
      setError("外键需要本表字段、引用表与引用字段");
      return;
    }
    const name = newFk.name.trim() || autoFkName(activeTable, newFk.column.trim());
    await runDdl(
      addFkSql(connection.kind, activeSchema, activeTable, { ...newFk, name }),
      `已添加外键 ${name}`
    );
    setNewFk({
      name: "",
      column: "",
      refTable: "",
      refColumn: "",
      onDelete: "",
      onUpdate: "",
    });
  };

  const filteredTables = useMemo(() => {    const q = tableFilter.trim().toLowerCase();
    if (!q) return tables;
    return tables.filter(
      (t) =>
        t.name.toLowerCase().includes(q) ||
        (t.tablespace || "").toLowerCase().includes(q)
    );
  }, [tables, tableFilter]);

  const totalPages = data ? Math.max(1, Math.ceil(data.total / pageSize)) : 1;

  const loadingPane = (
    <div className="h-full flex items-center justify-center">
      <div className="loading-pane animate-fade-up">
        <Loader2
          size={22}
          className="animate-spin"
          style={{ color: "var(--accent)" }}
        />
        <div className="text-sm font-medium">
          正在读取 <span className="mono">{database}</span> 的表
        </div>
        <div className="text-[11.5px]" style={{ color: "var(--text-subtle)" }}>
          {paneWaitSec < 2
            ? "建立会话中…"
            : paneWaitSec < 10
              ? `已等待 ${paneWaitSec}s · 首次访问该库需要先建立连接`
              : `已等待 ${paneWaitSec}s · 目标服务可能无响应，可先切到其他库`}
        </div>
        {paneWaitSec >= 10 && (
          <button className="btn" onClick={() => void loadTables()}>
            <RefreshCw size={13} /> 重新读取
          </button>
        )}
      </div>
    </div>
  );

  if (!connection) {
    return (
      <div className="h-full flex items-center justify-center">
        <div className="empty-state">
          <div className="empty-icon">
            <MousePointerClick size={24} />
          </div>
          <div className="text-sm font-medium" style={{ color: "var(--text-muted)" }}>
            尚未选择数据库
          </div>
          <div className="text-xs max-w-[280px]">
            在左侧展开一个连接并选择数据库，即可浏览表结构与数据
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full min-w-0">
      {/* 表列表 */}
      <div
        className="w-56 border-r flex flex-col shrink-0 min-h-0"
        style={{ borderColor: "var(--border)", background: "var(--bg-panel)" }}
      >
        <div
          className="px-3 pt-2.5 pb-2 border-b space-y-2 flex-none"
          style={{ borderColor: "var(--border)" }}
        >
          <div className="flex items-center gap-1.5">
            <Table2 size={13} style={{ color: "var(--accent)" }} />
            <span className="text-xs font-semibold truncate flex-1">
              {database || "数据库"}
            </span>
            {tablesLoading && tables.length === 0 ? (
              <RefreshCw
                size={12}
                className="animate-spin"
                style={{ color: "var(--text-subtle)" }}
              />
            ) : (
              <span key={tables.length} className="tag animate-pop-in">
                {tables.length}
              </span>
            )}
            {database && newObjectLabel && (
              <button
                className="btn btn-ghost"
                style={{ padding: "2px 5px" }}
                title={newObjectLabel}
                onClick={() => setObjectOpen(true)}
              >
                <Plus size={13} />
              </button>
            )}
          </div>
          {schemas.length > 0 && (
            <FilterSelect
              style={{ padding: "4px 8px", fontSize: 11.5, minHeight: 26 }}
              title="选择 schema"
              placeholder="选择 schema"
              value={activeSchema}
              options={schemas.map((s) => ({
                value: s,
                label: s,
                hint: "schema",
              }))}
              onChange={(v) => {
                setActiveSchema(v);
                setActiveTable(null);
              }}
            />
          )}
          <div className="relative">
            <Search
              size={12}
              className="absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none"
              style={{ color: "var(--text-subtle)" }}
            />
            <input
              className="input"
              style={{ paddingLeft: 26, paddingTop: 4, paddingBottom: 4, fontSize: 11.5 }}
              placeholder="筛选表…"
              value={tableFilter}
              onChange={(e) => setTableFilter(e.target.value)}
            />
          </div>
        </div>
        <div className="flex-1 overflow-y-auto p-1.5 space-y-0.5 stagger-in">
          {tablesLoading && tables.length === 0 && (
            <div className="space-y-1 px-1 pt-1 stagger-in">
              {Array.from({ length: 8 }).map((_, i) => (
                <div
                  key={i}
                  className="skeleton h-6"
                  style={{ opacity: 1 - i * 0.09 }}
                />
              ))}
            </div>
          )}
          {filteredTables.map((t) => {
            const name = t.name;
            const ts = t.tablespace;
            return (
            <div
              key={name}
              className={`tree-item text-xs ${activeTable === name ? "active" : ""}`}
              onClick={() => setActiveTable(name)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        setCtxMenu({
                          x: e.clientX,
                          y: e.clientY,
                          items: [
                            { type: "item", label: "打开表", onClick: () => setActiveTable(name) },
                            { type: "item", label: "刷新数据", onClick: () => { setActiveTable(name); setTimeout(() => loadPage(), 0); } },
                            { type: "item", label: "复制表名", onClick: () => void navigator.clipboard?.writeText(name) },
                            { type: "sep" },
                            { type: "item", label: "导出 CSV / Excel", onClick: () => {
                              setActiveTable(name);
                              onTransfer?.("export", connection, database, name);
                            }},
                            ...(isSqlKind
                              ? [
                                  {
                                    type: "item" as const,
                                    label: "转储表 (.sql)",
                                    onClick: () => {
                                      setActiveTable(name);
                                      onTransfer?.("dump-table", connection, database, name);
                                    },
                                  },
                                ]
                              : []),
                            { type: "item", label: "同步到其他库…", onClick: () => {
                              setActiveTable(name);
                              onTransfer?.("sync", connection, database, name);
                            }},
                          ] as ContextMenuItem[],
                        });
                      }}
            >
              <Table2 size={12} className="flex-none" style={{ color: activeTable === name ? "inherit" : "var(--text-subtle)" }} />
              <span className="truncate flex-1">{name}</span>
              {ts && (
                <span
                  className="tag flex-none"
                  style={{ fontSize: 9, padding: "0 4px" }}
                  title={`表空间: ${ts}`}
                >
                  {ts}
                </span>
              )}
            </div>
            );
          })}
          {filteredTables.length === 0 && !tablesLoading && (
            <div className="empty-state" style={{ padding: "28px 12px" }}>
              <div className="empty-icon" style={{ width: 44, height: 44, borderRadius: 14 }}>
                <Inbox size={18} />
              </div>
              <div className="text-xs">
                {tableFilter ? "没有匹配的表" : "暂无表，请展开连接并选择库"}
              </div>
              {!tableFilter && database && (
                <button className="btn" onClick={() => void loadTables()}>
                  <RefreshCw size={12} /> 重新读取表列表
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      {/* 主区域 */}
      <div className="flex-1 flex flex-col min-w-0 min-h-0 overflow-hidden">
        {/* 工具栏 */}
        <div className="browser-toolbar">
          <div className="split-tabs">
            <button className={tab === "data" ? "active" : ""} onClick={() => setTab("data")}>
              <Table2 size={13} /> 数据
            </button>
            {showSchema && (
              <button
                className={tab === "schema" ? "active" : ""}
                onClick={() => setTab("schema")}
              >
                <Columns3 size={13} /> {kindMeta.schema}
              </button>
            )}
            <button
              className={tab === "query" ? "active" : ""}
              onClick={() => setTab("query")}
            >
              {isSqlKind ? <TerminalSquare size={13} /> : <Braces size={13} />}
              {kindMeta.tab}
            </button>
          </div>
          {activeTable && (
            <span className="text-xs truncate min-w-0 max-w-[160px]" style={{ color: "var(--text-muted)" }}>
              <span className="mono font-semibold" style={{ color: "var(--text)" }}>
                {activeTable}
              </span>
            </span>
          )}
          <div className="flex-1 min-w-0" />
          <div
            className="toolbar-actions animate-slide-in-left"
            key={`acts-${tab}-${database}-${activeTable || ""}`}
          >
            {tab === "data" && activeTable && (
              <>
                <button
                  className="btn"
                  onClick={() => {
                    if (columns.length === 0) loadSchema();
                    const init: Record<string, string> = {};
                    columns.forEach((c) => (init[c.name] = ""));
                    setInsertData(init);
                    setInsertOpen(true);
                  }}
                >
                  <Plus size={14} /> <span className="btn-label">插入</span>
                </button>
                <button className="btn" onClick={loadPage} disabled={loading}>
                  <RefreshCw size={14} className={loading ? "animate-spin" : ""} />{" "}
                  <span className="btn-label">刷新</span>
                </button>
                {isSqlKind && (
                  <button
                    className={`btn ${qfOpen || qApplied.rows.length ? "btn-on" : ""}`}
                    title="按字段添加筛选条件与排序准则"
                    onClick={() => {
                      if (columns.length === 0) loadSchema();
                      setQfOpen((v) => !v);
                    }}
                  >
                    <ListFilter size={14} />{" "}
                    <span className="btn-label">筛选 &amp; 排序</span>
                    {qApplied.rows.length + qApplied.sorts.length > 0 && (
                      <span className="tag tag-accent">
                        {qApplied.rows.length + qApplied.sorts.length}
                      </span>
                    )}
                  </button>
                )}
                <button className="btn" onClick={() => setTransferMode("export")}>
                  <span className="btn-label">导出</span>
                </button>
                {docKind && (
                  <button
                    className="btn"
                    title="用 JSON 直接写一篇文档"
                    onClick={() => setDocOpen(true)}
                  >
                    <span className="btn-label">添加文档</span>
                  </button>
                )}
                {isSqlKind && (
                  <button className="btn" onClick={() => setTransferMode("dump-table")}>
                    <span className="btn-label">转储表</span>
                  </button>
                )}
                <button className="btn" onClick={() => setTransferMode("sync")}>
                  <span className="btn-label">同步</span>
                </button>
                <button
                  className="btn"
                  title="导入 CSV / Excel"
                  onClick={() => onTransfer?.("import", connection, database, activeTable)}
                >
                  <span className="btn-label">导入</span>
                </button>
              </>
            )}
            {tab === "data" && !activeTable && database && (
              <>
                {newObjectLabel && (
                  <button className="btn" onClick={() => setObjectOpen(true)}>
                    <Plus size={14} />{" "}
                    <span className="btn-label">{newObjectLabel}</span>
                  </button>
                )}
                {isSqlKind && (
                  <button className="btn" onClick={() => setTransferMode("dump-db")}>
                    <span className="btn-label">转储数据库</span>
                  </button>
                )}
                <button
                  className="btn"
                  title="把本库全部表同步到其他数据库（可为其他类型的数据库）"
                  onClick={() => setTransferMode("sync-db")}
                >
                  <span className="btn-label">同步整库</span>
                </button>
                {isSqlKind && (
                  <button
                    className="btn"
                    title="运行 SQL 脚本文件"
                    onClick={() => onTransfer?.("run-sql", connection, database, null)}
                  >
                    <span className="btn-label">运行 SQL 文件</span>
                  </button>
                )}
              </>
            )}
          </div>
        </div>

        {error && (
          <div className="error-banner">
            <AlertTriangle size={14} className="flex-none mt-0.5" />
            <span className="break-all">{error}</span>
            <button title="关闭" onClick={() => setError(null)}>
              <X size={14} />
            </button>
          </div>
        )}

        {execMsg && tab !== "query" && (
          <div className="notice-bar">
            <CheckCircle2 size={14} className="flex-none mt-0.5" />
            <span className="break-all">{execMsg}</span>
            <button title="关闭" onClick={() => setExecMsg(null)}>
              <X size={14} />
            </button>
          </div>
        )}

        <div className="flex-1 overflow-hidden flex flex-col">
          {tab === "data" && (
            <>
              {isSqlKind && qfOpen && (
                <div
                  className="qf-box animate-fade-up"
                  style={{ borderColor: "var(--border)" }}
                >
                  <div className="qf-head">
                    <span className="section-title">筛选</span>
                    <button
                      className="btn btn-ghost btn-icon"
                      title="添加筛选条件"
                      onClick={addQFilter}
                    >
                      <Plus size={13} />
                    </button>
                    <span className="qf-tip">
                      {qfRows.length ? `${qfRows.length} 条` : "点 + 添加条件"}
                    </span>
                  </div>

                  {qfRows.map((r, i) => {
                    const fam = familyOf(colTypeOf(r.col));
                    const meta = qfOp(fam, r.op);
                    return (
                      <div key={r.id} className="qf-row">
                        <input
                          type="checkbox"
                          checked={r.on}
                          title="启用该条件"
                          onChange={(e) =>
                            patchQFilter(r.id, { on: e.target.checked })
                          }
                        />
                        {i > 0 ? (
                          <FilterSelect
                            className="qf-logic"
                            value={r.logic}
                            options={[
                              { value: "and", label: "并且 and" },
                              { value: "or", label: "或者 or" },
                            ]}
                            onChange={(v) =>
                              patchQFilter(r.id, { logic: v as "and" | "or" })
                            }
                          />
                        ) : (
                          <span className="qf-where">WHERE</span>
                        )}
                        <FilterSelect
                          className="qf-col"
                          value={r.col}
                          placeholder="字段"
                          emptyText="没有字段"
                          options={qfColumnOptions}
                          onChange={(v) => {
                            const nf = familyOf(colTypeOf(v));
                            patchQFilter(r.id, { col: v, op: Q_OPS[nf][0].v });
                          }}
                        />
                        <FilterSelect
                          className="qf-op"
                          value={r.op}
                          placeholder="运算符"
                          options={Q_OPS[fam].map((o) => ({
                            value: o.v,
                            label: o.label,
                          }))}
                          onChange={(v) => patchQFilter(r.id, { op: v })}
                        />
                        {!meta?.noValue && (
                          <input
                            className="input qf-val"
                            placeholder={
                              r.op === "in" ? "a, b, c" : "值"
                            }
                            value={r.val}
                            onChange={(e) =>
                              patchQFilter(r.id, { val: e.target.value })
                            }
                          />
                        )}
                        {meta?.twoValues && (
                          <input
                            className="input qf-val"
                            placeholder="到"
                            value={r.val2}
                            onChange={(e) =>
                              patchQFilter(r.id, { val2: e.target.value })
                            }
                          />
                        )}
                        <button
                          className="btn btn-ghost btn-icon"
                          title="删除该条件"
                          onClick={() => removeQFilter(r.id)}
                        >
                          <X size={13} />
                        </button>
                      </div>
                    );
                  })}

                  <div className="qf-head" style={{ marginTop: 4 }}>
                    <span className="section-title">排序方式</span>
                    <button
                      className="btn btn-ghost btn-icon"
                      title="添加排序准则"
                      onClick={addQSort}
                    >
                      <Plus size={13} />
                    </button>
                    <span className="qf-tip">
                      {qSorts.length ? `按加入顺序生效` : "点 + 添加，或直接点表头"}
                    </span>
                  </div>

                  {qSorts.map((s) => (
                    <div key={s.id} className="qf-row">
                      <FilterSelect
                        className="qf-col"
                        value={s.col}
                        placeholder="字段"
                        emptyText="没有字段"
                        options={qfColumnOptions}
                        onChange={(v) => patchQSort(s.id, { col: v })}
                      />
                      <FilterSelect
                        className="qf-dir"
                        value={s.dir}
                        options={[
                          { value: "asc", label: "升序 A→Z" },
                          { value: "desc", label: "降序 Z→A" },
                        ]}
                        onChange={(v) =>
                          patchQSort(s.id, { dir: v as "asc" | "desc" })
                        }
                      />
                      <button
                        className="btn btn-ghost btn-icon"
                        title="删除该排序"
                        onClick={() => removeQSort(s.id)}
                      >
                        <X size={13} />
                      </button>
                    </div>
                  ))}

                  <div className="qf-foot">
                    <button
                      className="btn btn-primary"
                      onClick={applyQuery}
                      disabled={loading}
                    >
                      <Play size={13} /> 应用筛选 排序
                    </button>
                    <button
                      className="btn"
                      onClick={clearQuery}
                      disabled={
                        !qApplied.rows.length && !qApplied.sorts.length && !qfRows.length
                      }
                    >
                      清除
                    </button>
                    <button className="btn" onClick={() => setQfOpen(false)}>
                      收起
                    </button>
                    <span className="qf-tip">
                      条件与排序只改查询语句，不写数据；换表后自动清空
                    </span>
                  </div>
                </div>
              )}
              <div className="flex-1 overflow-auto">
                {tablesLoading && !activeTable ? (
                  loadingPane
                ) : loading && !data ? (
                  <div className="p-4 space-y-2">
                    {Array.from({ length: 10 }).map((_, i) => (
                      <div key={i} className="skeleton h-7" style={{ opacity: 1 - i * 0.08 }} />
                    ))}
                  </div>
                ) : data && data.columns.length > 0 ? (
                  <table
                    key={`${database}-${activeSchema}-${activeTable}-${page}`}
                    className="data-table animate-pane-in"
                  >
                    <thead>
                      <tr>
                        <th className="col-num" style={{ width: 52 }}>#</th>
                        {data.columns.map((c) => {
                          const s = qApplied.sorts.find((x) => x.col === c);
                          return (
                            <th
                              key={c}
                              className={isSqlKind ? "qf-th" : undefined}
                              title={
                                isSqlKind
                                  ? "点击切换：升序 → 降序 → 取消"
                                  : undefined
                              }
                              onClick={() => toggleHeaderSort(c)}
                            >
                              {c}
                              {s && (
                                <span className="qf-mark">
                                  {s.dir === "asc" ? "▲" : "▼"}
                                </span>
                              )}
                            </th>
                          );
                        })}
                        <th style={{ width: 92 }}>操作</th>
                      </tr>
                    </thead>
                    <tbody>
                      {data.rows.map((row, ri) => (
                        <tr
                          key={ri}
                          className="group"
                          onContextMenu={(e) => {
                            e.preventDefault();
                            e.stopPropagation();
                            setCtxMenu({
                              x: e.clientX,
                              y: e.clientY,
                              items: [
                                {
                                  type: "item",
                                  label: "编辑该行",
                                  onClick: () => {
                                    const rec = rowToRecord(row);
                                    setEditPk(extractPk(rec));
                                    setEditRow({ ...rec });
                                    void loadSchema();
                                  },
                                },
                                { type: "sep" },
                                {
                                  type: "item",
                                  label: "删除该行",
                                  danger: true,
                                  onClick: () => doDelete(row),
                                },
                              ] as ContextMenuItem[],
                            });
                          }}
                        >
                          <td className="col-num" style={{ color: "var(--text-subtle)", fontSize: 11 }}>
                            {(page - 1) * pageSize + ri + 1}
                          </td>
                          {row.map((cell, ci) => (
                            <td
                              key={ci}
                              className={cell === null ? "null" : "mono"}
                              title={cell === null ? "NULL" : typeof cell === "object" ? JSON.stringify(cell) : String(cell)}
                            >
                              {cell === null
                                ? "NULL"
                                : typeof cell === "object"
                                ? JSON.stringify(cell)
                                : String(cell)}
                            </td>
                          ))}
                          <td>
                            <div className="row-actions">
                              <button
                                title="编辑"
                                onClick={() => {
                                  const rec = rowToRecord(row);
                                  setEditPk(extractPk(rec));
                                  setEditRow({ ...rec });
                                  loadSchema();
                                }}
                              >
                                <Pencil size={13} />
                              </button>
                              <button
                                className="danger"
                                title="删除"
                                onClick={() => doDelete(row)}
                              >
                                <Trash2 size={13} />
                              </button>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : (
                  <div className="h-full flex items-center justify-center">
                    <div className="empty-state">
                      <div className="empty-icon">
                        {activeTable ? <Inbox size={22} /> : <Table2 size={22} />}
                      </div>
                      <div className="text-sm font-medium" style={{ color: "var(--text-muted)" }}>
                        {activeTable ? "该表暂无数据" : "未选择表"}
                      </div>
                      <div className="text-xs">
                        {activeTable ? "可以点击右上角「插入」添加一行" : "从左侧列表中选择一张表查看数据"}
                      </div>
                    </div>
                  </div>
                )}
              </div>
              {data && (
                <div
                  className="flex items-center gap-3 px-3 h-9 border-t text-xs flex-none"
                  style={{ borderColor: "var(--border)", color: "var(--text-muted)", background: "var(--bg-panel)" }}
                >
                  <span className="flex items-center gap-1.5">
                    <span className="status-dot status-ok" />
                    共 <b style={{ color: "var(--text)" }}>{data.total}</b> 行
                  </span>
                  <span style={{ color: "var(--text-subtle)" }}>·</span>
                  <span className="mono">{data.elapsed_ms}ms</span>
                  {data.message && (
                    <>
                      <span style={{ color: "var(--text-subtle)" }}>·</span>
                      <span>{data.message}</span>
                    </>
                  )}
                  <div className="flex-1" />
                  <span style={{ color: "var(--text-subtle)" }}>每页</span>
                  <FilterSelect
                    style={{
                      width: 72,
                      padding: "2px 6px",
                      fontSize: 11.5,
                      minHeight: 24,
                    }}
                    title="每页行数"
                    placeholder="每页"
                    value={String(pageSize)}
                    options={[20, 50, 100, 200].map((n) => ({
                      value: String(n),
                      label: String(n),
                    }))}
                    onChange={(v) => {
                      setPageSize(Number(v) || 50);
                      setPage(1);
                    }}
                  />
                  <div className="split-tabs" style={{ padding: 2 }}>
                    <button disabled={page <= 1} onClick={() => setPage((p) => p - 1)} style={{ padding: "3px 8px" }}>
                      <ChevronLeft size={13} />
                    </button>
                    <span className="px-1 text-[11.5px] self-center mono" style={{ color: "var(--text-muted)" }}>
                      {page}/{totalPages}
                    </span>
                    <input
                      className="input mono"
                      style={{ width: 52, padding: "2px 4px", fontSize: 11 }}
                      title="跳转页码"
                      value={pageJump}
                      onChange={(e) => setPageJump(e.target.value.replace(/\D/g, ""))}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          const n = Number(pageJump);
                          if (n >= 1 && n <= totalPages) setPage(n);
                        }
                      }}
                    />
                    <button
                      className="btn btn-ghost"
                      style={{ padding: "3px 8px", fontSize: 11 }}
                      onClick={() => {
                        const n = Number(pageJump);
                        if (n >= 1 && n <= totalPages) setPage(n);
                      }}
                    >
                      跳转
                    </button>
                    <button disabled={page >= totalPages} onClick={() => setPage((p) => p + 1)} style={{ padding: "3px 8px" }}>
                      <ChevronRight size={13} />
                    </button>
                  </div>
                </div>
              )}
            </>
          )}

          {tab === "schema" && tablesLoading && !activeTable && (
            <div className="flex-1 overflow-auto">{loadingPane}</div>
          )}

          {tab === "schema" && !(tablesLoading && !activeTable) && (
            <div className="flex-1 overflow-auto p-4 space-y-4 stack-in" style={{ background: "var(--bg)" }}>
              {(connection.kind === "postgres" || connection.kind === "pgvector") && (
                <section className="schema-card animate-fade-up">
                  <header>
                    <Database size={14} style={{ color: "var(--accent)" }} />
                    表空间
                    <span className="tag">{tablespaces.length}</span>
                    <button
                      className="btn btn-ghost"
                      style={{ marginLeft: "auto", fontSize: 11, padding: "2px 8px" }}
                      onClick={() => void loadTablespaces()}
                    >
                      刷新
                    </button>
                  </header>
                  {tablespaces.length === 0 ? (
                    <div className="p-3 text-xs" style={{ color: "var(--text-subtle)" }}>
                      暂无表空间数据（可能查询失败或无权限）
                    </div>
                  ) : (
                    <table className="data-table">
                      <thead>
                        <tr>
                          <th>名称</th>
                          <th>属主</th>
                          <th>位置</th>
                          <th>大小</th>
                          <th>注释</th>
                        </tr>
                      </thead>
                      <tbody>
                        {tablespaces.map((ts) => (
                          <tr key={ts.name}>
                            <td className="font-medium mono">{ts.name}</td>
                            <td>{ts.owner ?? ""}</td>
                            <td className="mono" style={{ color: "var(--text-muted)" }}>
                              {ts.location || "(默认/集群目录)"}
                            </td>
                            <td>{ts.size ?? ""}</td>
                            <td style={{ color: "var(--text-muted)" }}>{ts.comment ?? ""}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                </section>
              )}
              {(tableExtra?.schema || tableExtra?.tablespace || tableExtra?.size) && (
                <section className="schema-card animate-fade-up">
                  <header>
                    <Database size={14} style={{ color: "var(--accent)" }} />
                    表属性
                  </header>
                  <div className="flex flex-wrap gap-2 p-3">
                    {tableExtra?.schema && (
                      <span className="tag">schema: {tableExtra.schema}</span>
                    )}
                    {tableExtra?.tablespace && (
                      <span className="tag tag-accent">
                        tablespace: {tableExtra.tablespace}
                      </span>
                    )}
                    {tableExtra?.size && (
                      <span className="tag">size: {tableExtra.size}</span>
                    )}
                  </div>
                </section>
              )}
              {!activeTable && (
                <div className="text-xs" style={{ color: "var(--text-subtle)" }}>
                  {isSqlKind
                    ? "选择左侧表后可查看字段、索引与外键；上方为当前库连接的表空间列表。"
                    : "选择左侧条目后可查看该引擎给出的字段信息。"}
                </div>
              )}
              <section className="schema-card animate-fade-up">
                <header>
                  <Columns3 size={14} style={{ color: "var(--accent)" }} />
                  字段
                  <span className="tag">{columns.length}</span>
                  {kind === "mongodb" && (
                    <span className="tag" title="按最近 50 篇文档采样出的字段">
                      按文档采样
                    </span>
                  )}
                  {kind === "elasticsearch" && (
                    <span className="tag" title="来自索引 _mapping">
                      来自 mapping
                    </span>
                  )}
                  {activeTable && kind === "sqlite" && (
                    <span className="tag" title="改类型 / 约束需要重建整表，这里只支持重命名列">
                      SQLite 仅支持重命名列
                    </span>
                  )}
                  {activeTable && !isSqlKind && kind !== "sqlite" && (
                    <span className="tag" title="该引擎没有列级 DDL，字段信息只读">
                      只读
                    </span>
                  )}
                  {activeTable && (
                    <span className="flex-1" />
                  )}
                </header>
                {activeTable && (isSqlKind || kind === "mongodb" || kind === "elasticsearch") && (
                  <div
                    className="flex flex-wrap items-center gap-2 p-3 border-b"
                    style={{ borderColor: "var(--border)" }}
                  >
                    <input
                      className="input"
                      style={{ width: 150 }}
                      placeholder="新字段名"
                      value={schemaNewCol.name}
                      onChange={(e) => setSchemaNewCol({ ...schemaNewCol, name: e.target.value })}
                    />
                    <TypePicker
                      kind={kind}
                      value={schemaNewCol.type}
                      onChange={(v) => setSchemaNewCol({ ...schemaNewCol, type: v })}
                    />
                    {(isSqlKind || kind === "mongodb") && (
                      <input
                        className="input mono"
                        style={{ width: 120 }}
                        placeholder={kind === "mongodb" ? "初始值" : "默认值"}
                        title={
                          kind === "mongodb"
                            ? "数组/对象请填 JSON"
                            : "留空表示不设默认值"
                        }
                        value={schemaNewCol.default}
                        onChange={(e) =>
                          setSchemaNewCol({ ...schemaNewCol, default: e.target.value })
                        }
                      />
                    )}
                    {isSqlKind && (
                      <label
                        className="flex items-center gap-1.5 text-xs"
                        style={{ color: "var(--text-muted)" }}
                      >
                        <input
                          type="checkbox"
                          checked={schemaNewCol.notNull}
                          onChange={(e) =>
                            setSchemaNewCol({ ...schemaNewCol, notNull: e.target.checked })
                          }
                        />
                        非空
                      </label>
                    )}
                    <button
                      className="btn btn-primary"
                      disabled={ddlBusy}
                      onClick={submitNewColumn}
                    >
                      {ddlBusy ? "执行中…" : "添加字段"}
                    </button>
                    {kind === "elasticsearch" && (
                      <span className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
                        ES 只能新增映射字段，已有字段的类型需要重建索引
                      </span>
                    )}
                    {kind === "mongodb" && (
                      <span className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
                        Mongo 无固定模式：添加会给集合内全部文档写入该字段
                      </span>
                    )}
                  </div>
                )}
                <table className="data-table">
                  <thead>
                    <tr>
                      <th>名称</th>
                      <th>类型</th>
                      {isSqlKind && <th>可空</th>}
                      {(isSqlKind || kind === "mongodb") && <th>主键</th>}
                      {isSqlKind && <th>默认值</th>}
                      {isSqlKind && <th>注释</th>}
                      <th style={{ width: 96 }}></th>
                    </tr>
                  </thead>
                  <tbody>
                    {columns.map((c) => {
                      const editing = colEdit?.origin === c.name;
                      const draft = colEdit?.draft;
                      const cellInput = {
                        padding: "3px 7px",
                        fontSize: 12,
                      };
                      const canEdit = isSqlKind || kind === "mongodb";
                      return (
                      <tr key={c.name}>
                        <td className="font-medium mono">
                          {editing && draft ? (
                            <input
                              className="input mono"
                              style={cellInput}
                              value={draft.name}
                              onChange={(e) => patchDraft({ name: e.target.value })}
                            />
                          ) : (
                            c.name
                          )}
                        </td>
                        <td>
                          {editing && draft ? (
                            kind === "mongodb" ? (
                              <span className="tag">{draft.data_type}</span>
                            ) : (
                              <TypePicker
                                kind={kind}
                                value={draft.data_type}
                                onChange={(v) => patchDraft({ data_type: v })}
                              />
                            )
                          ) : (
                            <span className="tag">{c.data_type}</span>
                          )}
                        </td>
                        {isSqlKind && (
                          <td>
                            {editing && draft ? (
                              <input
                                type="checkbox"
                                checked={draft.nullable}
                                onChange={(e) => patchDraft({ nullable: e.target.checked })}
                              />
                            ) : (
                              <span
                                style={{
                                  color: c.nullable ? "var(--text-muted)" : "var(--warn)",
                                }}
                              >
                                {c.nullable ? "YES" : "NO"}
                              </span>
                            )}
                          </td>
                        )}
                        {(isSqlKind || kind === "mongodb") && (
                          <td>
                            {c.primary_key && (
                              <span className="tag tag-accent">
                                <KeyRound size={10} /> PK
                              </span>
                            )}
                          </td>
                        )}
                        {isSqlKind && (
                          <td className="mono" style={{ color: "var(--text-muted)" }}>
                            {editing && draft ? (
                              <input
                                className="input mono"
                                style={cellInput}
                                placeholder="无"
                                value={draft.default}
                                onChange={(e) => patchDraft({ default: e.target.value })}
                              />
                            ) : (
                              c.default ?? ""
                            )}
                          </td>
                        )}
                        {isSqlKind && (
                          <td style={{ color: "var(--text-muted)" }}>
                            {editing && draft ? (
                              <input
                                className="input"
                                style={cellInput}
                                value={draft.comment}
                                onChange={(e) => patchDraft({ comment: e.target.value })}
                              />
                            ) : (
                              c.comment ?? ""
                            )}
                          </td>
                        )}
                        <td>
                          <div
                            className="row-actions"
                            style={{ opacity: 1, justifyContent: "flex-end" }}
                          >
                            {editing ? (
                              <>
                                <button
                                  className="btn btn-primary"
                                  style={{ padding: "2px 8px", fontSize: 11 }}
                                  disabled={colSaving}
                                  onClick={saveColumn}
                                >
                                  {colSaving ? "保存中" : "保存"}
                                </button>
                                <button
                                  className="btn btn-ghost"
                                  style={{ padding: "2px 8px", fontSize: 11 }}
                                  onClick={() => setColEdit(null)}
                                >
                                  取消
                                </button>
                              </>
                            ) : (
                              <>
                                {activeTable && canEdit && (
                                  <button
                                    title={
                                      kind === "mongodb"
                                        ? "重命名字段（作用于全部文档）"
                                        : "编辑字段"
                                    }
                                    onClick={() => startColEdit(c)}
                                  >
                                    <Pencil size={12} />
                                  </button>
                                )}
                                {activeTable && canEdit && !c.primary_key && (
                                  <button
                                    className="danger"
                                    title={
                                      kind === "mongodb"
                                        ? "从全部文档移除该字段"
                                        : "删除字段"
                                    }
                                    onClick={() => void dropField(c)}
                                  >
                                    <Trash2 size={12} />
                                  </button>
                                )}
                              </>
                            )}
                          </div>
                        </td>
                      </tr>
                      );
                    })}
                  </tbody>
                </table>
              </section>
              {(isSqlKind || kind === "mongodb") && (
              <section className="schema-card animate-fade-up">
                <header>
                  <KeyRound size={14} style={{ color: "var(--accent)" }} />
                  索引
                  <span className="tag">{indexes.length}</span>
                  {activeTable && <span className="flex-1" />}
                </header>
                {activeTable && (
                  <div
                    className="flex flex-wrap items-center gap-2 p-3 border-b"
                    style={{ borderColor: "var(--border)" }}
                  >
                    <input
                      className="input"
                      style={{ width: 160 }}
                      placeholder="索引名（留空自动生成）"
                      value={newIdx.name}
                      onChange={(e) => setNewIdx({ ...newIdx, name: e.target.value })}
                    />
                    <div className="flex items-center gap-1 flex-wrap min-w-0">
                      {idxCols.map((cn, i) => (
                        <button
                          key={cn}
                          className="tag tag-accent"
                          title="点击移除"
                          onClick={() =>
                            setNewIdx({
                              ...newIdx,
                              columns: idxCols.filter((x) => x !== cn).join(", "),
                            })
                          }
                        >
                          {i + 1}. {cn} ×
                        </button>
                      ))}
                      <FilterSelect
                        className="mono"
                        style={{ width: 172 }}
                        title="加入索引字段（按选择顺序组成联合索引）"
                        placeholder={
                          idxCols.length ? "继续添加字段…" : "选择索引字段…"
                        }
                        emptyText="字段已全部加入"
                        value=""
                        options={columns
                          .filter((c) => !idxCols.includes(c.name))
                          .map((c) => ({
                            value: c.name,
                            label: c.name,
                            hint: c.data_type,
                          }))}
                        onChange={(v) => {
                          if (!v || idxCols.includes(v)) return;
                          setNewIdx({
                            ...newIdx,
                            columns: [...idxCols, v].join(", "),
                          });
                        }}
                      />
                    </div>
                    <label
                      className="flex items-center gap-1.5 text-xs"
                      style={{ color: "var(--text-muted)" }}
                    >
                      <input
                        type="checkbox"
                        checked={newIdx.unique}
                        onChange={(e) => setNewIdx({ ...newIdx, unique: e.target.checked })}
                      />
                      唯一
                    </label>
                    <button
                      className="btn btn-primary"
                      disabled={ddlBusy}
                      onClick={submitIndex}
                    >
                      {ddlBusy ? "执行中…" : "添加索引"}
                    </button>
                  </div>
                )}
                {indexes.length === 0 ? (
                  <div className="p-4 text-xs" style={{ color: "var(--text-subtle)" }}>无索引</div>
                ) : (
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th>名称</th>
                        <th>字段</th>
                        <th>唯一</th>
                        <th>主键</th>
                        <th style={{ width: 48 }}></th>
                      </tr>
                    </thead>
                    <tbody>
                      {indexes.map((i) => (
                        <tr key={i.name}>
                          <td className="font-medium mono">{i.name}</td>
                          <td className="mono">{i.columns.join(", ")}</td>
                          <td>{i.unique ? "YES" : "NO"}</td>
                          <td>{i.primary ? "YES" : "NO"}</td>
                          <td>
                            {activeTable && !i.primary && i.name !== "_id_" && (
                              <div className="row-actions" style={{ opacity: 1, justifyContent: "flex-end" }}>
                                <button
                                  className="danger"
                                  title="删除索引"
                                  onClick={() => {
                                    if (!connection) return;
                                    if (!confirm(`删除索引 ${i.name}？`)) return;
                                    const stmt =
                                      connection.kind === "mongodb"
                                        ? mongoIndexDrop(database, activeTable, i.name)
                                        : dropIndexSql(
                                            connection.kind,
                                            activeSchema,
                                            activeTable,
                                            i.name
                                          );
                                    runDdl(stmt, `已删除索引 ${i.name}`);
                                  }}
                                >
                                  <Trash2 size={12} />
                                </button>
                              </div>
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </section>
              )}
              {isSqlKind && (
              <section className="schema-card animate-fade-up">
                <header>
                  <Link2 size={14} style={{ color: "var(--accent)" }} />
                  外键 / 关系
                  <span className="tag">{fks.length}</span>
                </header>
                {activeTable &&
                  (kind === "sqlite" ? (
                    <div
                      className="p-3 text-xs border-b"
                      style={{ borderColor: "var(--border)", color: "var(--text-subtle)" }}
                    >
                      SQLite 不支持用 ALTER TABLE 添加外键（需要重建整张表），请建表时用
                      FOREIGN KEY 声明。
                    </div>
                  ) : (
                    <div
                      className="flex flex-wrap items-center gap-2 p-3 border-b"
                      style={{ borderColor: "var(--border)" }}
                    >
                      <input
                        className="input"
                        style={{ width: 150 }}
                        placeholder="约束名（留空自动）"
                        value={newFk.name}
                        onChange={(e) => setNewFk({ ...newFk, name: e.target.value })}
                      />
                      <FilterSelect
                        className="mono"
                        style={{ width: 152 }}
                        title="本表字段"
                        placeholder="本表字段…"
                        emptyText="暂无字段"
                        value={newFk.column}
                        options={columns.map((c) => ({
                          value: c.name,
                          label: c.name,
                          hint: c.data_type,
                        }))}
                        onChange={(v) => setNewFk({ ...newFk, column: v })}
                      />
                      <FilterSelect
                        className="mono"
                        style={{ width: 172 }}
                        title="引用表"
                        placeholder="引用表…"
                        emptyText="暂无其他表"
                        value={newFk.refTable}
                        options={tables.map((t) => ({
                          value: t.name,
                          label: t.name,
                          hint: t.comment || undefined,
                        }))}
                        onChange={(v) => {
                          setNewFk({ ...newFk, refTable: v, refColumn: "" });
                          void loadRefColumns(v);
                        }}
                      />
                      <FilterSelect
                        className="mono"
                        style={{ width: 152 }}
                        title="引用字段"
                        placeholder={
                          refColsLoading ? "读取字段…" : "引用字段…"
                        }
                        emptyText={
                          newFk.refTable
                            ? "该表暂无字段"
                            : "先选引用表，再选字段"
                        }
                        value={newFk.refColumn}
                        options={fsOptions(refCols)}
                        onChange={(v) => setNewFk({ ...newFk, refColumn: v })}
                      />
                      {REF_ACTIONS.map((a) => (
                        <FilterSelect
                          key={a.field}
                          style={{ width: 138 }}
                          title={a.label}
                          placeholder={`${a.label}（默认）`}
                          value={
                            a.field === "onDelete" ? newFk.onDelete : newFk.onUpdate
                          }
                          options={[
                            { value: "", label: `${a.label}（默认）` },
                            ...(kind === "mysql"
                              ? REF_ACTIONS_MYSQL
                              : REF_ACTIONS_PG
                            ).map((v) => ({ value: v, label: v })),
                          ]}
                          onChange={(v) =>
                            setNewFk(
                              { ...newFk, [a.field]: v } as FkDraft
                            )
                          }
                        />
                      ))}
                      <button className="btn btn-primary" disabled={ddlBusy} onClick={submitFk}>
                        {ddlBusy ? "执行中…" : "添加外键"}
                      </button>
                    </div>
                  ))}
                {fks.length === 0 ? (
                  <div className="p-4 text-xs" style={{ color: "var(--text-subtle)" }}>无外键</div>
                ) : (
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th>名称</th>
                        <th>字段</th>
                        <th>引用表</th>
                        <th>引用字段</th>
                        <th style={{ width: 48 }}></th>
                      </tr>
                    </thead>
                    <tbody>
                      {fks.map((f) => (
                        <tr key={f.name + f.column}>
                          <td className="font-medium mono">{f.name}</td>
                          <td className="mono">{f.column}</td>
                          <td className="mono">{f.ref_table}</td>
                          <td className="mono">{f.ref_column}</td>
                          <td>
                            {activeTable && (
                              <div className="row-actions" style={{ opacity: 1, justifyContent: "flex-end" }}>
                                <button
                                  className="danger"
                                  title="删除外键"
                                  onClick={() => {
                                    if (!connection || !activeTable) return;
                                    if (!confirm(`删除外键 ${f.name}？`)) return;
                                    runDdl(
                                      dropFkSql(
                                        connection.kind,
                                        activeSchema,
                                        activeTable,
                                        f.name
                                      ),
                                      `已删除外键 ${f.name}`
                                    );
                                  }}
                                >
                                  <Trash2 size={12} />
                                </button>
                              </div>
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </section>
              )}
            </div>
          )}

          {tab === "query" && (
            <div className="flex-1 flex flex-col min-h-0 animate-pane-in">
              <div className="p-3 border-b flex-none" style={{ borderColor: "var(--border)", background: "var(--bg-panel)" }}>
                <textarea
                  ref={sqlRef}
                  className="sql-editor"
                  rows={6}
                  value={sql}
                  onChange={(e) => setSql(e.target.value)}
                  onKeyDown={(e) => {
                    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                      e.preventDefault();
                      runSql();
                    }
                  }}
                  placeholder={kindMeta.placeholder}
                  spellCheck={false}
                />
                <div className="text-[11px] mt-1.5" style={{ color: "var(--text-subtle)" }}>
                  {kindMeta.hint}
                </div>
                <div className="flex items-center gap-2.5 mt-2">
                  <button className="btn btn-primary" onClick={runSql} disabled={loading || !sql.trim()}>
                    <Play size={13} /> 执行
                  </button>
                  <span className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
                    Ctrl + Enter 快捷执行
                  </span>
                  {execMsg && (
                    <span className="tag tag-accent animate-fade-in">{execMsg}</span>
                  )}
                  <div className="flex-1" />
                  {sqlHistory.length > 0 && (
                    <button className="btn btn-ghost" style={{ fontSize: 11 }} onClick={clearSqlHistory}>
                      清空历史
                    </button>
                  )}
                </div>
                {presetCount > 0 && (
                  <div className="preset-box">
                    <button
                      type="button"
                      className="preset-head"
                      onClick={() => setPresetsOpen((v) => !v)}
                    >
                      {presetsOpen ? (
                        <ChevronDown size={13} />
                      ) : (
                        <ChevronRight size={13} />
                      )}
                      <Zap size={13} style={{ color: "var(--accent)" }} />
                      <span className="text-xs font-semibold">常用操作</span>
                      <span className="tag">{presetCount}</span>
                      <span
                        className="text-[10px]"
                        style={{ color: "var(--text-subtle)" }}
                      >
                        {activeTable
                          ? `已代入 ${activeTable}`
                          : "选中表/索引后自动代入名称"}
                      </span>
                    </button>
                    {presetsOpen && (
                      <div className="preset-body">
                        {presets.map((g) => (
                          <div key={g.group} className="preset-group">
                            <span className="preset-group-label">{g.group}</span>
                            <div className="flex flex-wrap gap-1">
                              {g.items.map((it) => (
                                <button
                                  key={it.text}
                                  type="button"
                                  className={`preset-chip${it.danger ? " is-danger" : ""}`}
                                  title={it.text}
                                  onClick={() => applyPreset(it.text)}
                                >
                                  {it.label}
                                </button>
                              ))}
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}

                {sqlHistory.length > 0 && (
                  <div className="mt-2 max-h-28 overflow-auto space-y-0.5">
                    <div className="text-[10px] mb-1" style={{ color: "var(--text-subtle)" }}>
                      执行记录 · {database || "-"}
                    </div>
                    {sqlHistory.map((h, i) => (
                      <div key={`${i}-${h.slice(0, 24)}`} className="flex items-center gap-1 text-[11px]">
                        <button
                          className="flex-1 text-left truncate mono px-1.5 py-0.5 rounded"
                          style={{ background: "var(--bg-elevated)" }}
                          title={h}
                          onClick={() => setSql(h)}
                        >
                          {h}
                        </button>
                        <button
                          className="btn btn-ghost"
                          style={{ padding: "1px 4px" }}
                          title="删除该条"
                          onClick={() => removeSqlHistory(i)}
                        >
                          <X size={11} />
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </div>
              <div className="flex-1 overflow-auto">
                {loading ? (
                  <div className="p-4 space-y-2">
                    {Array.from({ length: 6 }).map((_, i) => (
                      <div key={i} className="skeleton h-7" style={{ opacity: 1 - i * 0.12 }} />
                    ))}
                  </div>
                ) : execOut ? (
                  <pre className="rest-out animate-fade-in">{execOut}</pre>
                ) : sqlResult ? (
                  <table className="data-table animate-fade-in">
                    <thead>
                      <tr>
                        {sqlResult.columns.map((c) => (
                          <th key={c}>{c}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {sqlResult.rows.map((row, i) => (
                        <tr key={i}>
                          {row.map((cell, j) => (
                            <td key={j} className={cell === null ? "null" : "mono"}>
                              {cell === null ? "NULL" : typeof cell === "object" ? JSON.stringify(cell) : String(cell)}
                            </td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : (
                  <div className="h-full flex items-center justify-center">
                    <div className="empty-state">
                      <div className="empty-icon">
                        <TerminalSquare size={22} />
                      </div>
                      <div className="text-xs">执行查询后，结果将显示在这里</div>
                    </div>
                  </div>
                )}
              </div>
              {sqlResult && (
                <div
                  className="flex items-center gap-2 px-3 h-9 border-t text-xs flex-none"
                  style={{ borderColor: "var(--border)", color: "var(--text-muted)", background: "var(--bg-panel)" }}
                >
                  <span className="status-dot status-ok" />
                  返回 <b style={{ color: "var(--text)" }}>{sqlResult.rows.length}</b> 行
                  <span style={{ color: "var(--text-subtle)" }}>·</span>
                  <span className="mono">{sqlResult.elapsed_ms}ms</span>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* 插入弹窗 */}
      {insertOpen && (
        <div className="modal-backdrop" onClick={() => setInsertOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center gap-2 mb-4">
              <span className="brand-mark" style={{ width: 24, height: 24, borderRadius: 7 }}>
                <Plus size={13} />
              </span>
              <h3 className="font-semibold text-sm flex-1">
                插入到 <span className="mono">{activeTable}</span>
              </h3>
              <button className="btn btn-ghost btn-icon" onClick={() => setInsertOpen(false)}>
                <X size={15} />
              </button>
            </div>
            <div className="space-y-3 max-h-96 overflow-auto pr-1">
              {columns.map((c) => (
                <label key={c.name} className="block">
                  <span className="field-label">
                    <span className="mono">{c.name}</span>{" "}
                    <span style={{ color: "var(--text-subtle)" }}>({c.data_type})</span>
                    {c.primary_key && <span className="tag tag-accent ml-1">PK</span>}
                  </span>
                  <input
                    className="input mono"
                    value={insertData[c.name] ?? ""}
                    onChange={(e) =>
                      setInsertData((d) => ({ ...d, [c.name]: e.target.value }))
                    }
                    placeholder={c.nullable ? "可空，填 null" : ""}
                  />
                </label>
              ))}
              {columns.length === 0 && (
                <div className="space-y-2">
                  {Array.from({ length: 4 }).map((_, i) => (
                    <div key={i} className="skeleton h-8" />
                  ))}
                </div>
              )}
            </div>
            <div className="flex justify-end gap-2 mt-5">
              <button className="btn" onClick={() => setInsertOpen(false)}>
                取消
              </button>
              <button className="btn btn-primary" onClick={doInsert}>
                <Plus size={14} /> 插入
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 编辑弹窗 */}
      {editRow && (
        <div className="modal-backdrop" onClick={() => setEditRow(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center gap-2 mb-4">
              <span className="brand-mark" style={{ width: 24, height: 24, borderRadius: 7 }}>
                <Pencil size={12} />
              </span>
              <h3 className="font-semibold text-sm flex-1">
                编辑 <span className="mono">{activeTable}</span> 的行
              </h3>
              <button className="btn btn-ghost btn-icon" onClick={() => setEditRow(null)}>
                <X size={15} />
              </button>
            </div>
            <div className="space-y-3 max-h-96 overflow-auto pr-1">
              {Object.entries(editRow).map(([k, v]) => (
                <label key={k} className="block">
                  <span className="field-label">
                    <span className="mono">{k}</span>
                    {pkCols.includes(k) && <span className="tag tag-accent ml-1">PK</span>}
                    {isEsMeta(k) && (
                      <span
                        className="tag ml-1"
                        style={{ color: "var(--text-subtle)" }}
                      >
                        元字段
                      </span>
                    )}
                  </span>
                  <input
                    className="input mono"
                    disabled={pkCols.includes(k) || isEsMeta(k)}
                    title={
                      isEsMeta(k)
                        ? "ES 元字段只用于定位文档，不会写进 _source"
                        : pkCols.includes(k)
                          ? "主键不可修改"
                          : undefined
                    }
                    value={v === null ? "null" : typeof v === "object" ? JSON.stringify(v) : String(v)}
                    onChange={(e) =>
                      setEditRow((r) => ({ ...r!, [k]: toCellValue(k, e.target.value) }))
                    }
                  />
                </label>
              ))}
              {kind === "elasticsearch" && (
                <div className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
                  改文档 id 请用「另存为新文档」：写 _id 会被 ES 拒绝（元字段不能进 _source）。
                </div>
              )}
            </div>
            <div className="flex justify-end gap-2 mt-5">
              <button className="btn" onClick={() => setEditRow(null)}>
                取消
              </button>
              <button className="btn btn-primary" onClick={doUpdate}>
                保存
              </button>
            </div>
          </div>
        </div>
      )}

      {pendingDelete && (
        <ConfirmDialog
          title="删除行"
          message="确认删除该行？此操作不可恢复。"
          danger
          confirmText="删除"
          onCancel={() => setPendingDelete(null)}
          onConfirm={confirmDelete}
        />
      )}

      {ctxMenu && (
        <ContextMenu state={ctxMenu} onClose={() => setCtxMenu(null)} />
      )}

      {objectOpen && connection && (
        <TableDesigner
          connection={connection}
          database={database}
          onClose={() => setObjectOpen(false)}
          onCreated={(created, stmts) => {
            void loadTables();
            setExecMsg(`已创建 ${created} · 执行了 ${stmts.length} 条语句`);
            setError(null);
          }}
        />
      )}

      {docOpen && connection && activeTable && (
        <DocumentEditor
          connection={connection}
          database={database}
          collection={activeTable}
          columns={columns}
          onClose={() => setDocOpen(false)}
          onSaved={(msg) => {
            setExecMsg(msg);
            setError(null);
            loadPage();
          }}
        />
      )}

      {transferMode && connection && (
        <TransferDialog
          mode={transferMode}
          connection={connection}
          database={database}
          table={activeTable}
          sql={sql}
          connections={allConns}
          onClose={() => setTransferMode(null)}
          onMessage={(m) => {
            if (m.startsWith("已")) {
              setExecMsg(m);
              setError(null);
            } else {
              setError(m);
            }
          }}
        />
      )}
    </div>
  );
}
