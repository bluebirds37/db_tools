import { useCallback, useEffect, useState } from "react";
import { backdropProps } from "./Modal";
import {
  X,
  ArrowRight,
  RefreshCw,
  Loader2,
  Play,
  CircleCheck,
  CircleX,
  MinusCircle,
} from "lucide-react";
import {
  api,
  type ConnectionConfig,
  type MigrateOptions,
  type MigrateReport,
} from "../services/api";
import { FilterSelect, fsOptions } from "./FilterSelect";

const SQL_KINDS = ["mysql", "postgres", "pgvector", "sqlite"];

const statusIcon = (s: string) =>
  s === "failed" ? (
    <CircleX size={12} style={{ color: "var(--danger)" }} />
  ) : s === "skipped" ? (
    <MinusCircle size={12} style={{ color: "var(--text-subtle)" }} />
  ) : (
    <CircleCheck size={12} style={{ color: "var(--ok)" }} />
  );

const statusLabel: Record<string, string> = {
  created: "已建结构",
  copied: "已迁移",
  skipped: "跳过",
  failed: "失败",
};

/// 库下拉：选项来自该连接的 listDatabases，失败时显示原始响应并可重试
function DbSelect({
  connectionId,
  value,
  placeholder,
  onChange,
}: {
  connectionId: string;
  value: string;
  placeholder: string;
  onChange: (v: string) => void;
}) {
  const [list, setList] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    if (!connectionId) return;
    setLoading(true);
    setError("");
    try {
      const dbs = await api.listDatabases(connectionId);
      setList(dbs);
      onChange(dbs.includes(value) ? value : dbs[0] || "");
    } catch (e) {
      setList([]);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [connectionId]);

  useEffect(() => {
    void load();
  }, [connectionId]);

  return (
    <>
      <div className="flex gap-1">
        <FilterSelect
          className="mono flex-1 min-w-0"
          value={value}
          disabled={loading}
          placeholder={loading ? "读取中…" : placeholder}
          emptyText={error ? "读取失败，点右侧重试" : "该连接暂无可选库"}
          options={fsOptions(list)}
          onChange={onChange}
        />
        <button
          className="btn"
          title="重新读取库列表"
          onClick={() => void load()}
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
        </button>
      </div>
      {error && (
        <div className="text-[10.5px] mt-1 break-all" style={{ color: "var(--danger)" }}>
          {error}
        </div>
      )}
    </>
  );
}

export function MigrationDialog({
  connections,
  source,
  sourceDatabase,
  onClose,
  onDone,
}: {
  connections: ConnectionConfig[];
  source: ConnectionConfig | null;
  sourceDatabase: string;
  onClose: () => void;
  onDone: (msg: string) => void;
}) {
  const sqlConns = connections.filter((c) => SQL_KINDS.includes(c.kind));
  const [srcConn, setSrcConn] = useState(
    source && SQL_KINDS.includes(source.kind)
      ? source.id
      : sqlConns[0]?.id || ""
  );
  const [srcDb, setSrcDb] = useState(
    source && source.id === srcConn ? sourceDatabase : ""
  );
  const [dstConn, setDstConn] = useState(
    sqlConns.find((c) => c.id !== srcConn)?.id || sqlConns[0]?.id || ""
  );
  const [dstDb, setDstDb] = useState("");
  const [opts, setOpts] = useState<MigrateOptions>({
    withStructure: true,
    withData: true,
    withIndexes: true,
    onExists: "skip",
    maxRowsPerTable: 200000,
  });
  const [srcTables, setSrcTables] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<MigrateReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const connOptions = sqlConns.map((c) => ({
    value: c.id,
    label: c.name,
    hint: c.kind,
  }));

  useEffect(() => {
    if (!srcConn || !srcDb) {
      setSrcTables(null);
      return;
    }
    let alive = true;
    api
      .listTables(srcConn, srcDb, null)
      .then((t) => alive && setSrcTables(t.filter((x) => x.table_type !== "VIEW").length))
      .catch(() => alive && setSrcTables(null));
    return () => {
      alive = false;
    };
  }, [srcConn, srcDb]);

  const run = async () => {
    if (!srcConn || !srcDb || !dstConn || !dstDb) {
      setError("请选择源库与目标库");
      return;
    }
    if (srcConn === dstConn && srcDb === dstDb) {
      setError("源库与目标库不能相同");
      return;
    }
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      const r = await api.migrateDatabase({
        sourceConnectionId: srcConn,
        sourceDatabase: srcDb,
        targetConnectionId: dstConn,
        targetDatabase: dstDb,
        options: opts,
      });
      setReport(r);
      onDone(
        `已迁移 ${r.copied + r.created}/${r.tables.length} 张表，共 ${r.totalRows} 行${
          r.failed ? `，${r.failed} 张失败` : ""
        }`
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const srcKind = sqlConns.find((c) => c.id === srcConn)?.kind;
  const dstKind = sqlConns.find((c) => c.id === dstConn)?.kind;

  return (
    <div className="modal-backdrop" {...backdropProps(onClose)}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-[15px] font-bold tracking-tight">
            数据库迁移
            <span className="conn-step">结构 + 数据 + 索引，可跨引擎</span>
          </h2>
          <button className="btn btn-ghost btn-icon" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        {sqlConns.length < 2 ? (
          <div className="text-xs p-4" style={{ color: "var(--text-subtle)" }}>
            迁移需要至少两个 MySQL / PostgreSQL / PgVector / SQLite 连接，当前只有{" "}
            {sqlConns.length} 个。
          </div>
        ) : (
          <>
            <div className="mg-grid">
              <div className="mg-side">
                <span className="section-title">源</span>
                <label className="block mt-2">
                  <span className="field-label">连接</span>
                  <FilterSelect
                    value={srcConn}
                    placeholder="选择源连接"
                    options={connOptions}
                    onChange={setSrcConn}
                  />
                </label>
                <label className="block mt-2">
                  <span className="field-label">数据库</span>
                  <DbSelect
                    connectionId={srcConn}
                    value={srcDb}
                    placeholder="选择源库"
                    onChange={setSrcDb}
                  />
                </label>
                <div className="text-[11px] mt-2" style={{ color: "var(--text-subtle)" }}>
                  {srcKind} · {srcDb || "-"} ·{" "}
                  {srcTables === null ? "统计中…" : `${srcTables} 张基础表`}
                </div>
              </div>

              <div className="mg-arrow">
                <ArrowRight size={18} />
              </div>

              <div className="mg-side">
                <span className="section-title">目标</span>
                <label className="block mt-2">
                  <span className="field-label">连接</span>
                  <FilterSelect
                    value={dstConn}
                    placeholder="选择目标连接"
                    options={connOptions}
                    onChange={setDstConn}
                  />
                </label>
                <label className="block mt-2">
                  <span className="field-label">数据库（需已存在）</span>
                  <DbSelect
                    connectionId={dstConn}
                    value={dstDb}
                    placeholder="选择目标库"
                    onChange={setDstDb}
                  />
                </label>
                <div className="text-[11px] mt-2" style={{ color: "var(--text-subtle)" }}>
                  {dstKind} · {dstDb || "-"}
                </div>
              </div>
            </div>

            <div
              className="flex flex-wrap items-center gap-x-4 gap-y-2 mt-3 pt-3 border-t"
              style={{ borderColor: "var(--border)" }}
            >
              {(
                [
                  ["withStructure", "迁移表结构"],
                  ["withData", "迁移数据"],
                  ["withIndexes", "迁移二级索引"],
                ] as const
              ).map(([key, label]) => (
                <label
                  key={key}
                  className="flex items-center gap-1.5 text-xs"
                  style={{ color: "var(--text-muted)" }}
                >
                  <input
                    type="checkbox"
                    checked={opts[key]}
                    onChange={(e) => setOpts({ ...opts, [key]: e.target.checked })}
                  />
                  {label}
                </label>
              ))}
              <span
                className="flex items-center gap-1.5 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                目标已存在
                <FilterSelect
                  value={opts.onExists}
                  placeholder="已存在时"
                  style={{
                    width: 134,
                    padding: "3px 8px",
                    fontSize: 11.5,
                    minHeight: 26,
                  }}
                  options={[
                    { value: "skip", label: "跳过该表", hint: "保留目标数据" },
                    { value: "recreate", label: "清空重建", hint: "删表后重建" },
                    { value: "append", label: "追加数据", hint: "结构不变" },
                  ]}
                  onChange={(v) =>
                    setOpts({
                      ...opts,
                      onExists: v as MigrateOptions["onExists"],
                    })
                  }
                />
              </span>
              <span
                className="flex items-center gap-1.5 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                每表最多
                <input
                  className="input mono"
                  style={{ width: 96, padding: "3px 8px", fontSize: 11.5 }}
                  type="number"
                  min={-1}
                  step={1}
                  title="填 0 或 -1 表示不限制，迁移每张表的全部行"
                  value={opts.maxRowsPerTable}
                  onChange={(e) => {
                    const n = Math.trunc(Number(e.target.value));
                    setOpts({
                      ...opts,
                      maxRowsPerTable: Number.isFinite(n) ? n : 0,
                    });
                  }}
                />
                行
                <button
                  className={`btn ${opts.maxRowsPerTable <= 0 ? "btn-primary" : ""}`}
                  style={{ padding: "2px 8px", fontSize: 11 }}
                  title="不限制行数，迁移每张表的全部行"
                  onClick={() =>
                    setOpts({ ...opts, maxRowsPerTable: -1 })
                  }
                >
                  全部行
                </button>
              </span>
            </div>

            <div className="mt-3">
              {busy && (
                <div className="progress mb-2">
                  <span />
                </div>
              )}
              {error && (
                <div className="error-banner" style={{ borderBottom: "none" }}>
                  <span className="break-all flex-1">{error}</span>
                  <button title="关闭" onClick={() => setError(null)}>
                    <X size={14} />
                  </button>
                </div>
              )}
              {report && (
                <div className="mg-report animate-fade-in">
                  <div className="flex items-center gap-2 text-xs mb-1">
                    <span className="tag tag-accent">共 {report.tables.length} 张表</span>
                    <span className="tag">迁移 {report.copied}</span>
                    <span className="tag">仅结构 {report.created}</span>
                    <span className="tag">跳过 {report.skipped}</span>
                    <span className="tag" style={{ color: report.failed ? "var(--danger)" : undefined }}>
                      失败 {report.failed}
                    </span>
                    <span className="tag">{report.totalRows} 行</span>
                  </div>
                  <table className="data-table">
                    <thead>
                      <tr>
                        <th style={{ width: 96 }}>状态</th>
                        <th>表</th>
                        <th style={{ width: 72 }}>行数</th>
                        <th>说明</th>
                      </tr>
                    </thead>
                    <tbody>
                      {report.tables.map((t) => (
                        <tr key={t.table}>
                          <td>
                            <span className="flex items-center gap-1.5 text-xs">
                              {statusIcon(t.status)} {statusLabel[t.status] || t.status}
                            </span>
                          </td>
                          <td className="mono">{t.table}</td>
                          <td className="mono">{t.rows}</td>
                          <td className="mg-detail" title={t.detail}>
                            {t.detail}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>

            <div
              className="flex items-center justify-between pt-3 mt-3 border-t"
              style={{ borderColor: "var(--border)" }}
            >
              <span className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
                迁移按表逐张执行，单表失败不影响其他表
              </span>
              <div className="flex gap-2">
                <button className="btn" onClick={onClose}>
                  {report ? "完成" : "取消"}
                </button>
                <button
                  className="btn btn-primary"
                  disabled={busy || !srcDb || !dstDb}
                  onClick={run}
                >
                  {busy ? <Loader2 size={13} className="animate-spin" /> : <Play size={13} />}
                  {busy ? "迁移中…" : "开始迁移"}
                </button>
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
