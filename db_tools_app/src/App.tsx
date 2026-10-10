import { useEffect, useState } from "react";
import {
  ChevronRight,
  Unplug,
  Settings2,
  Moon,
  Sun,
  Bot,
  CheckCircle2,
  X,
  ArrowLeftRight,
  Shuffle,
  FileDown,
  HardDriveDownload,
} from "lucide-react";
import { ConnectionSidebar, KindBadge } from "./components/ConnectionSidebar";
import { BrowserPane } from "./components/BrowserPane";
import { AiPanel } from "./components/AiPanel";
import { SettingsModal } from "./components/SettingsModal";
import { TransferDialog, type TransferMode } from "./components/TransferDialog";
import { MigrationDialog } from "./components/MigrationDialog";
import { useAppStore } from "./stores/appStore";
import type { ConnectionConfig } from "./services/api";

export interface TransferRequest {
  mode: TransferMode;
  connection: ConnectionConfig;
  database: string;
  table?: string | null;
}

export default function App() {
  const {
    theme,
    toggleTheme,
    aiOpen,
    setAiOpen,
    activeConnectionId,
    activeDatabase,
    activeTable,
    connections,
    setActiveConnection,
    setActiveDatabase,
    setError,
  } = useAppStore();
  const [selected, setSelected] = useState<{
    conn: ConnectionConfig;
    db: string;
  } | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [transfer, setTransfer] = useState<TransferRequest | null>(null);
  const [migrateOpen, setMigrateOpen] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 8000);
    return () => clearTimeout(t);
  }, [notice]);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
  }, [theme]);

  useEffect(() => {
    const onCtx = (e: MouseEvent) => {
      e.preventDefault();
    };
    window.addEventListener("contextmenu", onCtx);
    return () => window.removeEventListener("contextmenu", onCtx);
  }, []);

  const connection =
    selected?.conn ||
    connections.find((c) => c.id === activeConnectionId) ||
    null;
  const database = selected?.db || activeDatabase;

  return (
    <div className={`app-shell ${aiOpen ? "" : "ai-collapsed"}`}>
      <ConnectionSidebar
        onOpenDb={(conn, db) => {
          setSelected({ conn, db });
          setActiveConnection(conn.id);
          setActiveDatabase(db);
        }}
        onTransfer={(mode, conn, db) =>
          setTransfer({ mode, connection: conn, database: db, table: null })
        }
      />
      <main className="flex flex-col min-w-0 h-full">
        <header className="topbar">
          {connection ? (
            <div className="flex items-center gap-2 min-w-0 animate-fade-in flex-1">
              <span key={connection.id} className="animate-pop-in flex-none">
                <KindBadge kind={connection.kind} color={connection.color} />
              </span>
              <span className="topbar-title">{connection.name}</span>
              {database && (
                <span
                  key={`${connection.id}::${database}`}
                  className="crumb-in flex items-center gap-2 min-w-0"
                >
                  <ChevronRight
                    size={13}
                    style={{ color: "var(--text-subtle)", flex: "none" }}
                  />
                  <span className="topbar-sub truncate">{database}</span>
                </span>
              )}
              <span className="tag tag-accent">{connection.kind}</span>
            </div>
          ) : (
            <div className="topbar-empty flex-1">
              <Unplug size={14} />
              未连接 — 请在左侧选择或新建一个数据库连接
            </div>
          )}

          <div className="flex items-center gap-1 flex-none">
            <div className="topbar-tools">
              {(
                [
                  {
                    key: "sync",
                    label: "数据库同步",
                    tip: database
                      ? "把当前库的全部表同步到另一个数据库（可跨引擎）"
                      : "请先在左侧选择一个数据库",
                    icon: <ArrowLeftRight size={13} />,
                    disabled: !connection || !database,
                    onClick: () =>
                      connection &&
                      setTransfer({
                        mode: "sync-db" as TransferMode,
                        connection,
                        database,
                        table: null,
                      }),
                  },
                  {
                    key: "migrate",
                    label: "数据库迁移",
                    tip: "结构 + 数据 + 索引整库迁移，支持 MySQL / PostgreSQL / SQLite 互转",
                    icon: <Shuffle size={13} />,
                    disabled: false,
                    onClick: () => setMigrateOpen(true),
                  },
                  {
                    key: "export",
                    label: "数据导出",
                    tip: activeTable
                      ? "把当前表导出为 CSV / Excel"
                      : "请先在左侧选择一张表",
                    icon: <FileDown size={13} />,
                    disabled: !connection || !activeTable,
                    onClick: () =>
                      connection &&
                      activeTable &&
                      setTransfer({
                        mode: "export" as TransferMode,
                        connection,
                        database,
                        table: activeTable,
                      }),
                  },
                  {
                    key: "backup",
                    label: "数据库备份",
                    tip: database
                      ? "把当前库导出为带时间戳的 .sql 备份文件"
                      : "请先在左侧选择一个数据库",
                    icon: <HardDriveDownload size={13} />,
                    disabled: !connection || !database,
                    onClick: () =>
                      connection &&
                      setTransfer({
                        mode: "dump-db" as TransferMode,
                        connection,
                        database,
                        table: null,
                      }),
                  },
                ] as const
              ).map((t) => (
                <button
                  key={t.key}
                  className="btn"
                  title={t.tip}
                  disabled={t.disabled}
                  onClick={t.onClick}
                >
                  {t.icon} <span className="tool-label">{t.label}</span>
                </button>
              ))}
            </div>
            <button
              className="btn btn-ghost btn-icon"
              title={aiOpen ? "收起 AI" : "展开 AI"}
              style={aiOpen ? { color: "var(--accent)" } : undefined}
              onClick={() => setAiOpen(!aiOpen)}
            >
              <Bot size={15} />
            </button>
            <button
              className="btn btn-ghost btn-icon"
              title={theme === "dark" ? "切换浅色" : "切换深色"}
              onClick={toggleTheme}
            >
              {theme === "dark" ? <Sun size={15} /> : <Moon size={15} />}
            </button>
            <button
              className="btn btn-ghost btn-icon"
              title="设置"
              onClick={() => setShowSettings(true)}
            >
              <Settings2 size={15} />
            </button>
          </div>
        </header>
        {notice && (
          <div className="notice-bar">
            <CheckCircle2 size={14} className="flex-none mt-0.5" />
            <span className="break-all">{notice}</span>
            <button title="关闭" onClick={() => setNotice(null)}>
              <X size={14} />
            </button>
          </div>
        )}
        <div className="flex-1 min-h-0">
          <BrowserPane
            connection={connection}
            database={database}
            onTransfer={(mode, conn, db, table) =>
              setTransfer({ mode, connection: conn, database: db, table })
            }
          />
        </div>
      </main>
      <AiPanel />

      {showSettings && <SettingsModal onClose={() => setShowSettings(false)} />}

      {migrateOpen && (
        <MigrationDialog
          connections={connections}
          source={connection}
          sourceDatabase={database}
          onClose={() => setMigrateOpen(false)}
          onDone={(msg) => {
            setNotice(msg);
            setError(null);
          }}
        />
      )}

      {transfer && (
        <TransferDialog
          mode={transfer.mode}
          connection={transfer.connection}
          database={transfer.database}
          table={transfer.table ?? null}
          sql=""
          connections={connections}
          onClose={() => setTransfer(null)}
          onMessage={(m) => {
            if (m.startsWith("已")) {
              setNotice(m);
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
