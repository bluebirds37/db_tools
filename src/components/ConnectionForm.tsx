import { useMemo, useState } from "react";
import { backdropProps } from "./Modal";
import {
  X,
  Plug,
  Check,
  Database,
  FileJson,
  Layers,
  Search,
  Boxes,
  LayoutGrid,
  List,
  ArrowLeft,
  ArrowRight,
} from "lucide-react";
import { useAppStore } from "../stores/appStore";
import { api, type ConnectionConfig, type DbKind } from "../services/api";
import { KindBadge, KIND_META } from "./ConnectionSidebar";
import { FilterSelect } from "./FilterSelect";
import { readPref, writePref } from "../services/localPrefs";

type CatId = "rel" | "doc" | "kv" | "search" | "vector";

const CATEGORIES: { id: CatId; label: string; icon: typeof Database }[] = [
  { id: "rel", label: "关系型", icon: Database },
  { id: "doc", label: "文档型", icon: FileJson },
  { id: "kv", label: "键值 / 缓存", icon: Layers },
  { id: "search", label: "搜索引擎", icon: Search },
  { id: "vector", label: "向量库", icon: Boxes },
];

const KINDS: { value: DbKind; label: string; cat: CatId; desc: string }[] = [
  { value: "mysql", label: "MySQL", cat: "rel", desc: "最常用的开源关系库" },
  { value: "postgres", label: "PostgreSQL", cat: "rel", desc: "功能完整的对象关系库" },
  { value: "sqlite", label: "SQLite", cat: "rel", desc: "单文件本地库，无需服务" },
  { value: "mongodb", label: "MongoDB", cat: "doc", desc: "文档集合，模式自由" },
  { value: "redis", label: "Redis", cat: "kv", desc: "键值与数据结构服务" },
  { value: "elasticsearch", label: "Elasticsearch", cat: "search", desc: "全文检索与分析引擎" },
  { value: "pgvector", label: "PgVector", cat: "vector", desc: "带向量列的 PostgreSQL" },
  { value: "qdrant", label: "Qdrant", cat: "vector", desc: "向量相似度检索服务" },
];

const RECENT_KEY = "recent_kinds";

const readRecent = (): DbKind[] => {
  try {
    const raw = readPref(RECENT_KEY);
    const list = raw ? (JSON.parse(raw) as DbKind[]) : [];
    return Array.isArray(list)
      ? list.filter((k) => KINDS.some((x) => x.value === k)).slice(0, 4)
      : [];
  } catch {
    return [];
  }
};

const pushRecent = (kind: DbKind) => {
  try {
    const next = [kind, ...readRecent().filter((k) => k !== kind)].slice(0, 4);
    writePref(RECENT_KEY, JSON.stringify(next));
  } catch {
    /* 存不下就丢掉最近使用，不影响主流程 */
  }
};

export function ConnectionForm({
  initial,
  onClose,
}: {
  initial: ConnectionConfig | null;
  onClose: () => void;
}) {
  const { saveConnection, setError } = useAppStore();
  const [step, setStep] = useState<1 | 2>(initial ? 2 : 1);
  const [cat, setCat] = useState<CatId | "all">("all");
  const [query, setQuery] = useState("");
  const [dense, setDense] = useState(false);
  const [recent, setRecent] = useState<DbKind[]>(readRecent);
  const [form, setForm] = useState<ConnectionConfig>(
    initial ?? {
      id: "",
      name: "",
      kind: "mysql",
      host: "localhost",
      port: 3306,
      username: "root",
      password: "",
      database: "",
      options: {},
      color: null,
    }
  );
  const [testing, setTesting] = useState(false);
  const [testOk, setTestOk] = useState<string | null>(null);

  const set = <K extends keyof ConnectionConfig>(k: K, v: ConnectionConfig[K]) =>
    setForm((f) => ({ ...f, [k]: v }));

  const onKindChange = (kind: DbKind) => {
    const defaults: Partial<ConnectionConfig> = { kind };
    switch (kind) {
      case "mysql":
        defaults.port = 3306;
        defaults.host = "localhost";
        break;
      case "postgres":
        defaults.port = 5432;
        break;
      case "pgvector":
        defaults.port = 5433;
        break;
      case "redis":
        defaults.port = 6379;
        defaults.username = "";
        break;
      case "mongodb":
        defaults.port = 27017;
        break;
      case "elasticsearch":
        defaults.port = 9200;
        defaults.username = "";
        break;
      case "qdrant":
        defaults.port = 6333;
        defaults.username = "";
        break;
      case "sqlite":
        defaults.host = "";
        defaults.port = null;
        defaults.username = "";
        defaults.password = "";
        defaults.options = { path: "" };
        break;
    }
    setForm((f) => ({ ...f, ...defaults, kind }));
  };

  const test = async () => {
    setTesting(true);
    setTestOk(null);
    try {
      await api.testConnection({ ...form, id: form.id || "tmp" });
      setTestOk("连接成功");
    } catch (e) {
      setError(String(e));
      setTestOk(null);
    } finally {
      setTesting(false);
    }
  };

  const save = async () => {
    if (!form.name.trim()) {
      setError("请填写连接名称");
      return;
    }
    pushRecent(form.kind);
    await saveConnection(form);
    onClose();
  };

  const isSqlite = form.kind === "sqlite";
  const chosen = KINDS.find((k) => k.value === form.kind) || KINDS[0];

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return KINDS.filter((k) => {
      if (cat !== "all" && k.cat !== cat) return false;
      if (!q) return true;
      const catLabel = CATEGORIES.find((c) => c.id === k.cat)?.label || "";
      return (
        k.label.toLowerCase().includes(q) ||
        k.value.includes(q) ||
        k.desc.toLowerCase().includes(q) ||
        catLabel.toLowerCase().includes(q)
      );
    });
  }, [cat, query]);

  const recentKinds = useMemo(
    () => recent.map((k) => KINDS.find((x) => x.value === k)).filter(Boolean) as typeof KINDS,
    [recent]
  );

  const card = (k: (typeof KINDS)[number], fromRecent = false) => {
    const active = form.kind === k.value;
    const meta = KIND_META[k.value];
    return (
      <button
        key={`${fromRecent ? "r" : "a"}-${k.value}`}
        type="button"
        onClick={() => onKindChange(k.value)}
        onDoubleClick={() => {
          onKindChange(k.value);
          setStep(2);
        }}
        className={`type-card ${dense ? "dense" : ""} ${active ? "active" : ""}`}
        title={`${k.label} · ${k.desc}`}
      >
        <span className="type-tile" style={{ background: meta?.color || "#6b7280" }}>
          {meta?.abbr || "Db"}
        </span>
        <span className="type-name">{k.label}</span>
        {!dense && <span className="type-desc">{k.desc}</span>}
        {active && (
          <span className="type-check">
            <Check size={11} />
          </span>
        )}
      </button>
    );
  };

  return (
    <div className="modal-backdrop" {...backdropProps(onClose)}>
      <div className={`modal ${step === 1 ? "wide" : ""}`} onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[15px] font-bold tracking-tight">
            {initial ? "编辑连接" : "新建连接"}
            <span className="conn-step">
              {step === 1 ? "· 选择连接类型" : `· ${chosen.label}`}
            </span>
          </h2>
          <button className="btn btn-ghost btn-icon" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        {step === 1 ? (
          <>
            <div className="conn-picker">
              <aside className="conn-cats">
                {[{ id: "all" as const, label: "全部", icon: LayoutGrid }, ...CATEGORIES].map(
                  (c) => {
                    const on = cat === c.id;
                    const n =
                      c.id === "all"
                        ? KINDS.length
                        : KINDS.filter((k) => k.cat === c.id).length;
                    return (
                      <button
                        key={c.id}
                        type="button"
                        className={`tree-item conn-cat ${on ? "active" : ""}`}
                        onClick={() => setCat(c.id as CatId | "all")}
                      >
                        <c.icon size={13} />
                        <span className="flex-1 truncate">{c.label}</span>
                        <span className="tag">{n}</span>
                      </button>
                    );
                  }
                )}
              </aside>

              <div className="conn-main">
                <div className="flex items-center gap-2 mb-2">
                  <div className="split-tabs" style={{ padding: 2 }}>
                    <button
                      className={!dense ? "active" : ""}
                      title="网格视图"
                      onClick={() => setDense(false)}
                    >
                      <LayoutGrid size={13} />
                    </button>
                    <button
                      className={dense ? "active" : ""}
                      title="列表视图"
                      onClick={() => setDense(true)}
                    >
                      <List size={13} />
                    </button>
                  </div>
                  <input
                    className="input"
                    style={{ maxWidth: 180, marginLeft: "auto" }}
                    placeholder="搜索类型…"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                  />
                </div>

                {recentKinds.length > 0 && !query.trim() && cat === "all" && (
                  <div className="conn-group">
                    <div className="section-title mb-1.5">最近使用过的</div>
                    <div className="type-grid stagger-in">
                      {recentKinds.map((k) => card(k, true))}
                    </div>
                  </div>
                )}

                <div className="conn-group">
                  <div className="section-title mb-1.5">
                    {cat === "all" ? "全部" : CATEGORIES.find((c) => c.id === cat)?.label}
                    <span className="tag" style={{ marginLeft: 6 }}>
                      {filtered.length}
                    </span>
                  </div>
                  {filtered.length === 0 ? (
                    <div className="empty-state" style={{ padding: "28px 12px" }}>
                      <div className="empty-icon" style={{ width: 44, height: 44, borderRadius: 14 }}>
                        <Search size={18} />
                      </div>
                      <div className="text-xs">没有匹配的连接类型</div>
                    </div>
                  ) : (
                    <div
                      className={dense ? "type-list stagger-in" : "type-grid stagger-in"}
                      key={`${cat}-${dense}`}
                    >
                      {filtered.map((k) => card(k))}
                    </div>
                  )}
                </div>
              </div>
            </div>

            <div
              className="flex items-center justify-between pt-3 mt-3 border-t"
              style={{ borderColor: "var(--border)" }}
            >
              <span className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
                已选：{chosen.label} · 双击卡片可直接进入下一步
              </span>
              <div className="flex gap-2">
                <button className="btn" onClick={onClose}>
                  取消
                </button>
                <button
                  className="btn btn-primary"
                  onClick={() => {
                    pushRecent(form.kind);
                    setRecent(readRecent());
                    setStep(2);
                  }}
                >
                  下一步 <ArrowRight size={14} />
                </button>
              </div>
            </div>
          </>
        ) : (
          <div className="space-y-4">
            <button
              type="button"
              className="conn-chosen"
              title="返回重新选择类型"
              onClick={() => setStep(1)}
            >
              <KindBadge kind={form.kind} color={KIND_META[form.kind]?.color} />
              <span className="flex-1 text-left">
                <span className="block text-xs font-semibold">{chosen.label}</span>
                <span className="block text-[10.5px]" style={{ color: "var(--text-subtle)" }}>
                  {chosen.desc} · 点击更换类型
                </span>
              </span>
              <ArrowLeft size={14} style={{ color: "var(--text-subtle)" }} />
            </button>

            <label className="block">
              <span className="field-label">名称</span>
              <input
                className="input"
                value={form.name}
                onChange={(e) => set("name", e.target.value)}
                placeholder="例如：本地 MySQL"
              />
            </label>

            {isSqlite ? (
              <label className="block">
                <span className="field-label">数据库文件路径</span>
                <input
                  className="input mono"
                  value={form.options?.path || ""}
                  onChange={(e) =>
                    set("options", { ...(form.options || {}), path: e.target.value })
                  }
                  placeholder="D:\data\app.db"
                />
              </label>
            ) : (
              <div className="grid grid-cols-[1fr_120px] gap-3">
                <label className="block">
                  <span className="field-label">主机</span>
                  <input
                    className="input mono"
                    value={form.host || ""}
                    onChange={(e) => set("host", e.target.value)}
                  />
                </label>
                <label className="block">
                  <span className="field-label">端口</span>
                  <input
                    className="input mono"
                    type="number"
                    value={form.port ?? ""}
                    onChange={(e) =>
                      set("port", e.target.value ? Number(e.target.value) : null)
                    }
                  />
                </label>
              </div>
            )}

            {!isSqlite && form.kind !== "elasticsearch" && form.kind !== "qdrant" && (
              <div className="grid grid-cols-2 gap-3">
                <label className="block">
                  <span className="field-label">用户名</span>
                  <input
                    className="input mono"
                    value={form.username || ""}
                    onChange={(e) => set("username", e.target.value)}
                  />
                </label>
                <label className="block">
                  <span className="field-label">密码</span>
                  <input
                    className="input mono"
                    type="password"
                    value={form.password || ""}
                    onChange={(e) => set("password", e.target.value)}
                  />
                </label>
              </div>
            )}

            <label className="block">
              <span className="field-label">
                {form.kind === "redis" ? "默认 DB 索引" : "默认数据库（可选）"}
              </span>
              {form.kind === "redis" ? (
                <FilterSelect
                  className="mono"
                  value={form.database || ""}
                  placeholder="连接配置里的默认库"
                  title="默认 DB 索引"
                  options={[
                    { value: "", label: "连接配置里的默认库" },
                    ...Array.from({ length: 16 }, (_, i) => ({
                      value: String(i),
                      label: `db${i}`,
                      hint: `SELECT ${i}`,
                    })),
                  ]}
                  onChange={(v) => set("database", v)}
                />
              ) : (
                <input
                  className="input mono"
                  value={form.database || ""}
                  onChange={(e) => set("database", e.target.value)}
                />
              )}
            </label>

            <div
              className="flex items-center justify-between pt-3 border-t"
              style={{ borderColor: "var(--border)" }}
            >
              <div
                className="text-xs flex items-center gap-1.5"
                style={{ color: "var(--ok)" }}
              >
                {testOk && (
                  <>
                    <Check size={13} /> {testOk}
                  </>
                )}
              </div>
              <div className="flex gap-2">
                {!initial && (
                  <button className="btn" onClick={() => setStep(1)}>
                    <ArrowLeft size={14} /> 上一步
                  </button>
                )}
                <button className="btn" onClick={test} disabled={testing}>
                  <Plug size={14} className={testing ? "animate-pulse" : ""} />
                  {testing ? "测试中…" : "测试连接"}
                </button>
                <button className="btn btn-primary" onClick={save}>
                  保存
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
