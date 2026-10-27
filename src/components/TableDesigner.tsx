import { useEffect, useMemo, useState, type ReactNode } from "react";
import { backdropProps } from "./Modal";
import { X, Plus, Trash2, Check, Loader2 } from "lucide-react";
import {
  api,
  type ConnectionConfig,
  type DbKind,
} from "../services/api";
import { TypePicker, defaultTypeOf } from "./fieldTypes";
import { FilterSelect, fsOptions } from "./FilterSelect";

export type NewObject =
  | { kind: "table" }
  | { kind: "index" }
  | { kind: "collection" }
  | { kind: "points" };

type ColRow = {
  name: string;
  data_type: string;
  nullable: boolean;
  primary_key: boolean;
  auto_increment: boolean;
  default_value: string;
  comment: string;
};

const emptyRow = (kind: DbKind): ColRow => ({
  name: "",
  data_type: defaultTypeOf(kind),
  nullable: true,
  primary_key: false,
  auto_increment: false,
  default_value: "",
  comment: "",
});

const seedRows = (kind: DbKind): ColRow[] => [
  {
    name: "id",
    data_type: kind === "mysql" ? "BIGINT" : kind === "sqlite" ? "INTEGER" : "bigint",
    nullable: false,
    primary_key: true,
    auto_increment: true,
    default_value: "",
    comment: "",
  },
  {
    name: "created_at",
    data_type:
      kind === "mysql" ? "DATETIME" : kind === "sqlite" ? "TEXT" : "timestamp",
    nullable: false,
    primary_key: false,
    auto_increment: false,
    default_value: kind === "mysql" ? "CURRENT_TIMESTAMP" : "now()",
    comment: "",
  },
];

const Row = ({ label, children }: { label: string; children: ReactNode }) => (
  <div className="dd-row">
    <span className="dd-label">{label}</span>
    <div className="dd-value">{children}</div>
  </div>
);

/// 非关系引擎的对象创建语句在前端拼装，交给各自的 execute 通道
function nativePlan(
  kind: DbKind,
  database: string,
  name: string,
  opts: { shards: number; replicas: number; mapping: string; dim: number; distance: string }
): { stmt: string; preview: string[] } | { error: string } {
  const n = name.trim();
  if (!n) return { error: "请填写名称" };
  if (kind === "mongodb" && !database) return { error: "请先在左侧选择一个数据库" };
  if (kind === "mongodb") {
    const stmt = `db.${database}.${JSON.stringify({ create: n })}`;
    return { stmt, preview: [`${stmt}   -- 创建集合`] };
  }
  if (kind === "elasticsearch") {
    const body: Record<string, unknown> = {
      settings: {
        number_of_shards: opts.shards,
        number_of_replicas: opts.replicas,
      },
    };
    const raw = opts.mapping.trim();
    if (raw) {
      try {
        body.mappings = JSON.parse(raw);
      } catch (e) {
        return { error: `mapping JSON 解析失败：${String(e)}` };
      }
    }
    const stmt = `PUT /${n} ${JSON.stringify(body)}`;
    return {
      stmt,
      preview: [
        `PUT /${n}`,
        JSON.stringify(body, null, 2),
      ],
    };
  }
  if (kind === "qdrant") {
    const body = { vectors: { size: opts.dim, distance: opts.distance } };
    const stmt = `PUT /collections/${n} ${JSON.stringify(body)}`;
    return { stmt, preview: [`PUT /collections/${n}`, JSON.stringify(body, null, 2)] };
  }
  return { error: "该引擎不支持在这里创建对象" };
}

export function TableDesigner({
  connection,
  database,
  onClose,
  onCreated,
}: {
  connection: ConnectionConfig;
  database: string;
  onClose: () => void;
  onCreated: (name: string, stmts: string[]) => void;
}) {
  const kind = connection.kind;
  const isSql = ["mysql", "postgres", "pgvector", "sqlite"].includes(kind);
  const title =
    kind === "elasticsearch"
      ? "新建索引"
      : kind === "mongodb"
        ? "新建集合"
        : kind === "qdrant"
          ? "新建向量集合"
          : "新建表";

  const [name, setName] = useState("");
  const [comment, setComment] = useState("");
  const [engine, setEngine] = useState("InnoDB");
  const [charset, setCharset] = useState("utf8mb4");
  const [rows, setRows] = useState<ColRow[]>(isSql ? seedRows(kind) : []);
  const [shards, setShards] = useState(1);
  const [replicas, setReplicas] = useState(0);
  const [mapping, setMapping] = useState("");
  const [dim, setDim] = useState(768);
  const [distance, setDistance] = useState("Cosine");
  const [preview, setPreview] = useState<string[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const spec = useMemo(
    () => ({
      name: name.trim(),
      comment: comment.trim() || null,
      engine: kind === "mysql" ? engine : null,
      charset: kind === "mysql" ? charset : null,
      columns: rows
        .filter((r) => r.name.trim())
        .map((r) => ({
          name: r.name.trim(),
          dataType: r.data_type.trim() || null,
          nullable: r.nullable,
          primaryKey: r.primary_key,
          autoIncrement: r.auto_increment,
          defaultValue: r.default_value.trim() || null,
          comment: r.comment.trim() || null,
        })),
    }),
    [name, comment, rows, kind, engine, charset]
  );

  useEffect(() => {
    if (!isSql) {
      const r = nativePlan(kind, database, name, {
        shards,
        replicas,
        mapping,
        dim,
        distance,
      });
      if ("error" in r) {
        setPreview([]);
        setErr(name.trim() ? r.error : null);
      } else {
        setPreview(r.preview);
        setErr(null);
      }
      return;
    }
    if (!name.trim() || !spec.columns.length) {
      setPreview([]);
      return;
    }
    let alive = true;
    api
      .planCreateTable({ connectionId: connection.id, spec })
      .then((s) => alive && setPreview(s))
      .catch((e) => alive && setErr(String(e)));
    return () => {
      alive = false;
    };
  }, [isSql, kind, database, name, spec, shards, replicas, mapping, dim, distance, connection.id]);

  const patch = (i: number, p: Partial<ColRow>) =>
    setRows((rs) => rs.map((r, idx) => (idx === i ? { ...r, ...p } : r)));

  const create = async () => {
    setErr(null);
    if (!name.trim()) {
      setErr("请填写名称");
      return;
    }
    setBusy(true);
    try {
      if (isSql) {
        const stmts = await api.applyCreateTable({
          connectionId: connection.id,
          database,
          spec,
        });
        onCreated(name.trim(), stmts);
      } else {
        const r = nativePlan(kind, database, name, {
          shards,
          replicas,
          mapping,
          dim,
          distance,
        });
        if ("error" in r) {
          setErr(r.error);
          setBusy(false);
          return;
        }
        await api.executeSql(connection.id, database, r.stmt);
        onCreated(name.trim(), [r.stmt]);
      }
      onClose();
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" {...backdropProps(onClose)}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-[15px] font-bold tracking-tight">
            {title}
            <span className="conn-step">
              {connection.name} · {database || "-"}
            </span>
          </h2>
          <button className="btn btn-ghost btn-icon" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div className="dd-form stagger-in" style={{ maxWidth: "none" }}>
          <Row label={kind === "mysql" ? "表名称" : "名称"}>
            <input
              className="input mono"
              value={name}
              placeholder={kind === "elasticsearch" ? "my_index" : "my_table"}
              onChange={(e) => setName(e.target.value)}
            />
          </Row>

          {isSql && (
            <Row label="表注释">
              <input
                className="input"
                value={comment}
                placeholder={kind === "sqlite" ? "SQLite 不支持表注释" : "可选"}
                disabled={kind === "sqlite"}
                onChange={(e) => setComment(e.target.value)}
              />
            </Row>
          )}

          {kind === "mysql" && (
            <div className="grid grid-cols-2 gap-3">
              <Row label="存储引擎">
                <FilterSelect
                  className="mono"
                  value={engine}
                  title="存储引擎"
                  options={fsOptions([
                    "InnoDB",
                    "MyISAM",
                    "MEMORY",
                    "ARCHIVE",
                  ])}
                  onChange={setEngine}
                />
              </Row>
              <Row label="字符集">
                <FilterSelect
                  className="mono"
                  value={charset}
                  title="字符集"
                  options={fsOptions([
                    "utf8mb4",
                    "utf8",
                    "latin1",
                    "gbk",
                    "binary",
                  ])}
                  onChange={setCharset}
                />
              </Row>
            </div>
          )}

          {kind === "elasticsearch" && (
            <>
              <div className="grid grid-cols-2 gap-3">
                <Row label="主分片数">
                  <input
                    className="input mono"
                    type="number"
                    min={1}
                    value={shards}
                    onChange={(e) => setShards(Math.max(1, Number(e.target.value) || 1))}
                  />
                </Row>
                <Row label="副本数">
                  <input
                    className="input mono"
                    type="number"
                    min={0}
                    value={replicas}
                    onChange={(e) => setReplicas(Math.max(0, Number(e.target.value) || 0))}
                  />
                </Row>
              </div>
              <Row label="mapping">
                <textarea
                  className="sql-editor"
                  rows={6}
                  spellCheck={false}
                  placeholder={'留空则交给 ES 动态映射；或直接写：\n{\n  "properties": {\n    "title": { "type": "text" },\n    "tags": { "type": "keyword" }\n  }\n}'}
                  value={mapping}
                  onChange={(e) => setMapping(e.target.value)}
                />
              </Row>
            </>
          )}

          {kind === "qdrant" && (
            <div className="grid grid-cols-2 gap-3">
              <Row label="向量维度">
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  value={dim}
                  onChange={(e) => setDim(Math.max(1, Number(e.target.value) || 1))}
                />
              </Row>
              <Row label="距离函数">
                <FilterSelect
                  className="mono"
                  value={distance}
                  title="距离函数"
                  options={[
                    { value: "Cosine", label: "Cosine", hint: "余弦相似度" },
                    { value: "Euclid", label: "Euclid", hint: "欧氏距离" },
                    { value: "Dot", label: "Dot", hint: "点积" },
                    {
                      value: "Manhattan",
                      label: "Manhattan",
                      hint: "曼哈顿距离",
                    },
                  ]}
                  onChange={setDistance}
                />
              </Row>
            </div>
          )}
        </div>

        {isSql && (
          <div className="td-cols">
            <div className="flex items-center justify-between mb-1.5">
              <span className="section-title">
                字段 · {rows.filter((r) => r.name.trim()).length}
              </span>
              <button
                className="btn"
                style={{ padding: "2px 8px", fontSize: 11.5 }}
                onClick={() => setRows((rs) => [...rs, emptyRow(kind)])}
              >
                <Plus size={12} /> 添加字段
              </button>
            </div>
            <table className="data-table td-table">
              <thead>
                <tr>
                  <th style={{ width: 150 }}>名称</th>
                  <th style={{ width: 236 }}>类型</th>
                  <th style={{ width: 52 }} title="可空">
                    空
                  </th>
                  <th style={{ width: 52 }} title="主键">
                    PK
                  </th>
                  {kind !== "sqlite" && (
                    <th style={{ width: 52 }} title="自增">
                      AI
                    </th>
                  )}
                  <th style={{ width: 130 }}>默认值</th>
                  {kind !== "sqlite" && <th>注释</th>}
                  <th style={{ width: 40 }}></th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={i}>
                    <td>
                      <input
                        className="input mono"
                        style={{ padding: "3px 6px", fontSize: 12 }}
                        value={r.name}
                        placeholder="字段名"
                        onChange={(e) => patch(i, { name: e.target.value })}
                      />
                    </td>
                    <td>
                      <TypePicker
                        kind={kind}
                        value={r.data_type}
                        onChange={(v) => patch(i, { data_type: v })}
                      />
                    </td>
                    <td className="td-check">
                      <input
                        type="checkbox"
                        checked={r.nullable}
                        onChange={(e) => patch(i, { nullable: e.target.checked })}
                      />
                    </td>
                    <td className="td-check">
                      <input
                        type="checkbox"
                        checked={r.primary_key}
                        onChange={(e) =>
                          patch(i, {
                            primary_key: e.target.checked,
                            nullable: e.target.checked ? false : r.nullable,
                          })
                        }
                      />
                    </td>
                    {kind !== "sqlite" && (
                      <td className="td-check">
                        <input
                          type="checkbox"
                          checked={r.auto_increment}
                          onChange={(e) =>
                            patch(i, {
                              auto_increment: e.target.checked,
                              primary_key: e.target.checked ? true : r.primary_key,
                            })
                          }
                        />
                      </td>
                    )}
                    <td>
                      <input
                        className="input mono"
                        style={{ padding: "3px 6px", fontSize: 12 }}
                        value={r.default_value}
                        placeholder="留空不设"
                        onChange={(e) => patch(i, { default_value: e.target.value })}
                      />
                    </td>
                    {kind !== "sqlite" && (
                      <td>
                        <input
                          className="input"
                          style={{ padding: "3px 6px", fontSize: 12 }}
                          value={r.comment}
                          onChange={(e) => patch(i, { comment: e.target.value })}
                        />
                      </td>
                    )}
                    <td className="td-check">
                      <button
                        className="btn btn-ghost danger"
                        style={{ padding: "2px 4px" }}
                        title="移除该字段"
                        onClick={() => setRows((rs) => rs.filter((_, idx) => idx !== i))}
                      >
                        <Trash2 size={11} />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {preview.length > 0 && (
          <div className="mt-3">
            <span className="field-label">将执行的语句</span>
            <pre className="rest-out" style={{ margin: 0, maxHeight: 190 }}>
              {preview.join(";\n") + (isSql ? ";" : "")}
            </pre>
          </div>
        )}

        {err && (
          <div className="error-banner mt-3" style={{ borderBottom: "none" }}>
            <span className="break-all flex-1">{err}</span>
            <button title="关闭" onClick={() => setErr(null)}>
              <X size={14} />
            </button>
          </div>
        )}

        <div
          className="flex items-center justify-between pt-3 mt-3 border-t"
          style={{ borderColor: "var(--border)" }}
        >
          <span className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
            {isSql
              ? `${rows.filter((r) => r.name.trim()).length} 个字段 · 预览与实际执行同一份 DDL`
              : "语句按该引擎的原生接口下发"}
          </span>
          <div className="flex gap-2">
            <button className="btn" onClick={onClose}>
              取消
            </button>
            <button className="btn btn-primary" disabled={busy} onClick={create}>
              {busy ? <Loader2 size={13} className="animate-spin" /> : <Check size={13} />}
              {title}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
