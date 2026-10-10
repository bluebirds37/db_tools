import { useEffect, useMemo, useState } from "react";
import {
  Database,
  Plus,
  Trash2,
  Plug,
  Pencil,
  RefreshCw,
  Search,
  ChevronRight,
  Loader2,
  X,
  Sparkles,
} from "lucide-react";
import { useAppStore } from "../stores/appStore";
import { api, type ConnectionConfig, type DbKind } from "../services/api";
import { ConnectionForm } from "./ConnectionForm";
import { ConfirmDialog } from "./Modal";
import { fetchLogTail, getLogDir } from "../services/logger";
import { ContextMenu, type ContextMenuState, type ContextMenuItem } from "./ContextMenu";
import { DatabaseDesigner } from "./DatabaseDesigner";

/// 支持库级 DDL 的引擎（SQLite 是单文件、Redis/Mongo/ES/Qdrant 没有库级设计器概念）
export const DB_DDL_KINDS = ["mysql", "postgres", "pgvector"];

export const KIND_META: Record<string, { color: string; abbr: string }> = {
  mysql: { color: "#00758F", abbr: "My" },
  postgres: { color: "#336791", abbr: "Pg" },
  pgvector: { color: "#4169E1", abbr: "Pv" },
  redis: { color: "#DC382D", abbr: "Re" },
  mongodb: { color: "#47A248", abbr: "Mo" },
  elasticsearch: { color: "#00A9A5", abbr: "Es" },
  sqlite: { color: "#0F80CC", abbr: "Sq" },
  qdrant: { color: "#DC244C", abbr: "Qd" },
};

export function KindBadge({ kind, color }: { kind: string; color?: string | null }) {
  const meta = KIND_META[kind] ?? { color: "#6b7280", abbr: "Db" };
  return (
    <span className="kind-badge" style={{ background: color || meta.color }}>
      {meta.abbr}
    </span>
  );
}

export function ConnectionSidebar({
  onOpenDb,
  onTransfer,
}: {
  onOpenDb: (conn: ConnectionConfig, db: string) => void;
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
    db: string
  ) => void;
}) {
  const {
    theme,
    connections,
    loadConnections,
    activeConnectionId,
    activeDatabase,
    setActiveConnection,
    setActiveDatabase,
    deleteConnection,
    setError,
  } = useAppStore();
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<ConnectionConfig | null>(null);
  const [expanded, setExpanded] = useState<Record<string, string[]>>({});
  const [testing, setTesting] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<ConnectionConfig | null>(null);
  const [showLogs, setShowLogs] = useState(false);
  const [logText, setLogText] = useState("");
  const [logPath, setLogPath] = useState("");
  const [ctxMenu, setCtxMenu] = useState<ContextMenuState | null>(null);
  const [loadingConn, setLoadingConn] = useState<string | null>(null);
  const [dbDesigner, setDbDesigner] = useState<{
    conn: ConnectionConfig;
    mode: "create" | "edit";
    database: string;
  } | null>(null);
  const [dbDrop, setDbDrop] = useState<{ conn: ConnectionConfig; db: string } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 6000);
    return () => clearTimeout(t);
  }, [notice]);

  useEffect(() => {
    loadConnections();
    document.documentElement.classList.toggle("dark", theme === "dark");
  }, []);

  const toggleExpand = async (conn: ConnectionConfig) => {
    const current = expanded[conn.id];
    // [] is truthy in JS — only treat non-empty as expanded
    if (current && current.length > 0) {
      setExpanded((e) => ({ ...e, [conn.id]: [] }));
      setActiveConnection(conn.id);
      return;
    }
    setActiveConnection(conn.id);
    const dbs = await refreshDatabases(conn);
    if (dbs[0]) onOpenDb(conn, dbs[0]);
  };

  const refreshDatabases = async (conn: ConnectionConfig) => {
    setLoadingConn(conn.id);
    try {
      const dbs = await api.listDatabases(conn.id);
      setExpanded((e) => ({ ...e, [conn.id]: dbs }));
      return dbs;
    } catch (err) {
      setError(String(err));
      setExpanded((e) => ({ ...e, [conn.id]: [] }));
      return [];
    } finally {
      setLoadingConn((cur) => (cur === conn.id ? null : cur));
    }
  };

  const dropDatabase = async (conn: ConnectionConfig, db: string) => {
    try {
      await api.applyDatabase({ connectionId: conn.id, action: "drop", spec: { name: db } });
      if (activeConnectionId === conn.id && activeDatabase === db) setActiveDatabase("");
      await refreshDatabases(conn);
      setNotice(`已删除数据库 ${db}`);
    } catch (e) {
      setError(String(e));
    }
  };

  const test = async (conn: ConnectionConfig) => {
    setTesting(conn.id);
    try {
      await api.reconnect(conn.id);
      await api.testConnection(conn);
      await toggleExpand(conn);
    } catch (e) {
      setError(String(e));
    } finally {
      setTesting(null);
    }
  };

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return connections;
    return connections.filter(
      (c) =>
        c.name.toLowerCase().includes(q) ||
        c.kind.toLowerCase().includes(q) ||
        (c.host || "").toLowerCase().includes(q)
    );
  }, [connections, filter]);

  const subtitle = (c: ConnectionConfig) =>
    c.kind === "sqlite"
      ? c.options?.path || "本地文件"
      : `${c.host || "localhost"}${c.port ? `:${c.port}` : ""}`;

  return (
    <aside
      className="flex flex-col h-full min-h-0 overflow-hidden border-r"
      style={{ background: "var(--sidebar)", borderColor: "var(--border)" }}
    >
      {/* 搜索 + 新建 */}
      <div className="px-3 pt-3.5 pb-2 space-y-2 flex-none">
        <div className="relative">
          <Search
            size={13}
            className="absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none"
            style={{ color: "var(--text-subtle)" }}
          />
          <input
            className="input"
            style={{ paddingLeft: 28, paddingTop: 5, paddingBottom: 5, fontSize: 12 }}
            placeholder="搜索连接…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>
        <div className="flex items-center justify-between px-1">
          <span className="section-title">
            连接 · {connections.length}
          </span>
          <button
            className="btn btn-ghost btn-icon"
            title="新建连接"
            style={{ color: "var(--accent)" }}
            onClick={() => {
              setEditing(null);
              setShowForm(true);
            }}
          >
            <Plus size={15} />
          </button>
        </div>
      </div>

      {/* 连接列表 */}
      <div className="flex-1 min-h-0 overflow-y-auto px-2.5 pb-3 space-y-0.5 stagger-in">
        {filtered.map((c) => {
          const dbs = expanded[c.id];
          const active = activeConnectionId === c.id;
          return (
            <div key={c.id}>
              <div
                className={`tree-item ${active ? "active" : ""}`}
                style={{ paddingTop: 6, paddingBottom: 6 }}
                onClick={() => toggleExpand(c)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  setCtxMenu({
                    x: e.clientX,
                    y: e.clientY,
                    items: [
                      { type: "item", label: "测试并连接", onClick: () => void test(c) },
                      { type: "item", label: dbs && dbs.length > 0 ? "收起" : "展开", onClick: () => void toggleExpand(c) },
                      { type: "sep" },
                      ...(DB_DDL_KINDS.includes(c.kind)
                        ? [
                            {
                              type: "item" as const,
                              label: "新建数据库…",
                              onClick: () =>
                                setDbDesigner({ conn: c, mode: "create" as const, database: "" }),
                            },
                          ]
                        : []),
                      { type: "item", label: "编辑连接", onClick: () => { setEditing(c); setShowForm(true); } },
                      { type: "item", label: "复制名称", onClick: () => void navigator.clipboard?.writeText(c.name) },
                      { type: "sep" },
                      { type: "item", label: "删除连接", danger: true, onClick: () => setDeleteTarget(c) },
                    ] as ContextMenuItem[],
                  });
                }}
              >
                <button
                  type="button"
                  title={dbs && dbs.length > 0 ? "收起" : "展开"}
                  className="flex-none p-0.5 rounded hover:bg-black/5 dark:hover:bg-white/10"
                  onClick={(e) => {
                    e.stopPropagation();
                    void toggleExpand(c);
                  }}
                >
                  {loadingConn === c.id ? (
                    <Loader2
                      size={12}
                      className="animate-spin"
                      style={{ color: "var(--accent)" }}
                    />
                  ) : (
                    <ChevronRight
                      size={12}
                      className="transition-transform duration-200"
                      style={{
                        color: "var(--text-subtle)",
                        transform: dbs && dbs.length > 0 ? "rotate(90deg)" : "none",
                      }}
                    />
                  )}
                </button>
                <KindBadge kind={c.kind} color={c.color} />
                <div className="flex-1 min-w-0 leading-tight">
                  <div className="truncate font-medium text-[12.5px]">{c.name}</div>
                  <div
                    className="truncate text-[10.5px]"
                    style={{ color: "var(--text-subtle)" }}
                  >
                    {subtitle(c)}
                  </div>
                </div>
                <div className="row-actions">
                  <button
                    title="测试并连接"
                    onClick={(e) => {
                      e.stopPropagation();
                      test(c);
                    }}
                  >
                    {testing === c.id ? (
                      <RefreshCw size={13} className="animate-spin" />
                    ) : (
                      <Plug size={13} />
                    )}
                  </button>
                  <button
                    title="编辑"
                    onClick={(e) => {
                      e.stopPropagation();
                      setEditing(c);
                      setShowForm(true);
                    }}
                  >
                    <Pencil size={13} />
                  </button>
                  <button
                    className="danger"
                    title="删除"
                    onClick={(e) => {
                      e.stopPropagation();
                      setDeleteTarget(c);
                    }}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
              {dbs && dbs.length > 0 && (
                <div className="ml-[26px] mt-0.5 mb-1 space-y-0.5 animate-fade-in stagger-in">
                  {dbs.map((db) => {
                    const isActive =
                      activeConnectionId === c.id && activeDatabase === db;
                    return (
                      <div
                        key={db}
                        className={`tree-item text-xs ${isActive ? "active" : ""}`}
                        style={{
                          paddingTop: 4,
                          paddingBottom: 4,
                          fontWeight: isActive ? 600 : undefined,
                        }}
                        onClick={() => onOpenDb(c, db)}
                        onContextMenu={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          setCtxMenu({
                            x: e.clientX,
                            y: e.clientY,
                            items: [
                              { type: "item", label: "打开数据库", onClick: () => onOpenDb(c, db) },
                              { type: "item", label: "复制库名", onClick: () => void navigator.clipboard?.writeText(db) },
                              ...(DB_DDL_KINDS.includes(c.kind)
                                ? [
                                    { type: "sep" as const },
                                    {
                                      type: "item" as const,
                                      label: "编辑数据库…",
                                      onClick: () =>
                                        setDbDesigner({ conn: c, mode: "edit" as const, database: db }),
                                    },
                                    {
                                      type: "item" as const,
                                      label: "删除数据库",
                                      danger: true,
                                      onClick: () => setDbDrop({ conn: c, db }),
                                    },
                                  ]
                                : []),
                              { type: "sep" },
                              { type: "item", label: "转储数据库 (.sql)", onClick: () => onTransfer?.("dump-db", c, db) },
                              { type: "item", label: "导出查询结果", onClick: () => onTransfer?.("export", c, db) },
                              { type: "item", label: "同步整库到其他库…", onClick: () => onTransfer?.("sync-db", c, db) },
                              { type: "item", label: "运行 SQL 脚本文件…", onClick: () => onTransfer?.("run-sql", c, db) },
                              { type: "item", label: "导入 CSV / Excel…", onClick: () => onTransfer?.("import", c, db) },
                              { type: "sep" },
                              { type: "item", label: "刷新库列表", onClick: () => void refreshDatabases(c) },
                            ] as ContextMenuItem[],
                          });
                        }}
                      >
                        <Database
                          size={12}
                          style={{
                            color: isActive ? "var(--accent)" : "var(--text-subtle)",
                          }}
                        />
                        <span className="truncate">{db}</span>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}

        {filtered.length === 0 && (
          <div className="empty-state" style={{ padding: "32px 12px" }}>
            <div className="empty-icon">
              <Sparkles size={22} />
            </div>
            <div className="text-xs">
              {filter ? "没有匹配的连接" : "还没有连接"}
            </div>
            {!filter && (
              <button
                className="btn btn-primary"
                onClick={() => {
                  setEditing(null);
                  setShowForm(true);
                }}
              >
                <Plus size={14} /> 新建连接
              </button>
            )}
          </div>
        )}
      </div>

      {/* 底部状态 */}
      <div
        className="px-4 py-2.5 border-t flex items-center gap-2 text-[10.5px] flex-none"
        style={{ borderColor: "var(--border)", color: "var(--text-subtle)" }}
      >
        <span className="status-dot status-ok" />
        <span className="flex-1">本地服务已就绪</span>
        <button
          className="btn btn-ghost"
          style={{ fontSize: 11, padding: "2px 8px" }}
          title="查看错误日志"
          onClick={() => setShowLogs(true)}
        >
          日志
        </button>
      </div>

      {showForm && (
        <ConnectionForm
          initial={editing}
          onClose={() => {
            setShowForm(false);
            setEditing(null);
          }}
        />
      )}

      {showLogs && (
        <div className="modal-backdrop" onClick={() => setShowLogs(false)}>
          <div className="modal wide" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center justify-between mb-3">
              <h3 className="font-semibold text-sm">应用日志（最近 200 行）</h3>
              <div className="flex gap-2">
                <button
                  className="btn"
                  onClick={async () => {
                    const dir = await getLogDir();
                    if (dir) setLogPath(dir);
                    setLogText(await fetchLogTail(200));
                  }}
                >
                  刷新
                </button>
                <button className="btn btn-ghost" onClick={() => setShowLogs(false)}>
                  关闭
                </button>
              </div>
            </div>
            <div className="text-[11px] mb-2" style={{ color: "var(--text-subtle)" }}>
              {logPath || "日志目录: ~/.db_tools/logs/app.log"}
            </div>
            <pre
              className="mono"
              style={{
                maxHeight: 420,
                overflow: "auto",
                background: "var(--bg-elevated)",
                border: "1px solid var(--border)",
                borderRadius: 8,
                padding: 10,
                whiteSpace: "pre-wrap",
                wordBreak: "break-all",
              }}
            >
              {logText || "（暂无日志）"}
            </pre>
          </div>
        </div>
      )}

      {deleteTarget && (
        <ConfirmDialog
          title="删除连接"
          message={"确认删除连接「" + deleteTarget.name + "」？此操作不可恢复。"}
          danger
          confirmText="删除"
          onCancel={() => setDeleteTarget(null)}
          onConfirm={async () => {
            const id = deleteTarget.id;
            setDeleteTarget(null);
            await deleteConnection(id);
          }}
        />
      )}

      {dbDesigner && (
        <DatabaseDesigner
          connection={dbDesigner.conn}
          mode={dbDesigner.mode}
          database={dbDesigner.database}
          onClose={() => setDbDesigner(null)}
          onApplied={(stmts, m) => {
            void refreshDatabases(dbDesigner.conn);
            setNotice(
              `${m === "create" ? "已创建" : "已更新"}数据库 · 执行了 ${stmts.length} 条语句`
            );
          }}
        />
      )}

      {dbDrop && (
        <ConfirmDialog
          title={`删除数据库 ${dbDrop.db}`}
          message={`该操作会永久删除「${dbDrop.db}」及其全部表和数据，不可恢复。确认继续？`}
          danger
          confirmText="删除数据库"
          onCancel={() => setDbDrop(null)}
          onConfirm={async () => {
            const { conn, db } = dbDrop;
            setDbDrop(null);
            await dropDatabase(conn, db);
          }}
        />
      )}

      {notice && (
        <div className="notice-bar" style={{ borderRight: "none" }}>
          <span className="break-all flex-1">{notice}</span>
          <button title="关闭" onClick={() => setNotice(null)}>
            <X size={14} />
          </button>
        </div>
      )}

      {ctxMenu && (
        <ContextMenu state={ctxMenu} onClose={() => setCtxMenu(null)} />
      )}
    </aside>
  );
}

export const kindLabel = (k: DbKind) => k;
