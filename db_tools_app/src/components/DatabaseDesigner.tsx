import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  X,
  SlidersHorizontal,
  Boxes,
  MessageSquare,
  ScrollText,
  Search,
  Check,
  Loader2,
} from "lucide-react";
import {
  api,
  type ConnectionConfig,
  type DbOptions,
  type DbSpec,
  type ExtInfo,
} from "../services/api";
import { FilterSelect } from "./FilterSelect";

type Tab = "general" | "ext" | "comment" | "sql";

const EMPTY: DbSpec = {
  name: "",
  connLimit: -1,
  allowConnect: true,
  isTemplate: false,
};

const Row = ({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) => (
  <div className="dd-row">
    <span className="dd-label" title={hint}>
      {label}
      {hint && <span className="dd-hint">?</span>}
    </span>
    <div className="dd-value">{children}</div>
  </div>
);

const Pick = ({
  value,
  options,
  placeholder,
  disabled,
  title,
  onChange,
}: {
  value?: string | null;
  options: string[];
  placeholder?: string;
  disabled?: boolean;
  title?: string;
  onChange: (v: string) => void;
}) => {
  const v = value ?? "";
  return (
    <FilterSelect
      className="mono"
      value={v}
      disabled={disabled}
      title={title}
      placeholder={placeholder || "（不设置）"}
      emptyText="暂无候选值"
      options={[
        { value: "", label: placeholder || "（不设置）" },
        ...options.map((o) => ({ value: o, label: o })),
      ]}
      onChange={onChange}
    />
  );
};

export function DatabaseDesigner({
  connection,
  mode,
  database,
  onClose,
  onApplied,
}: {
  connection: ConnectionConfig;
  mode: "create" | "edit";
  database: string;
  onClose: () => void;
  onApplied: (stmts: string[], mode: "create" | "edit") => void;
}) {
  const editing = mode === "edit";
  const [opts, setOpts] = useState<DbOptions | null>(null);
  const [spec, setSpec] = useState<DbSpec>(editing ? { ...EMPTY, name: database, newName: database } : EMPTY);
  const [tab, setTab] = useState<Tab>("general");
  const [preview, setPreview] = useState<string[]>([]);
  const [planErr, setPlanErr] = useState<string | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [exts, setExts] = useState<ExtInfo[]>([]);
  const [extQuery, setExtQuery] = useState("");
  const [extBusy, setExtBusy] = useState<string | null>(null);
  const [extMsg, setExtMsg] = useState<string | null>(null);

  const set = <K extends keyof DbSpec>(k: K, v: DbSpec[K]) =>
    setSpec((s) => ({ ...s, [k]: v }));

  useEffect(() => {
    let alive = true;
    api
      .getDatabaseOptions({ connectionId: connection.id, database })
      .then((o) => {
        if (!alive) return;
        setOpts(o);
        const cur = o.current;
        if (cur) {
          setSpec({ ...cur, newName: cur.name });
        } else if (!editing) {
          // 新建时给个合理起点，避免用户面对一堆空下拉
          const enc = o.encodings.find((e) => e.toUpperCase() === "UTF8") || o.encodings[0] || "";
          const cs = o.charsets.find((c) => c.toLowerCase() === "utf8mb4") || o.charsets[0] || "";
          setSpec({ ...EMPTY, encoding: enc || null, charset: cs || null });
        }
      })
      .catch((e) => alive && setLoadErr(String(e)));
    return () => {
      alive = false;
    };
  }, [connection.id, database, editing]);

  const loadExts = () => {
    api
      .listExtensions({ connectionId: connection.id, database: spec.name || database })
      .then(setExts)
      .catch((e) => setExtMsg(String(e)));
  };

  useEffect(() => {
    if (tab !== "ext" || exts.length > 0 || !editing) return;
    loadExts();
  }, [tab, exts.length]);

  useEffect(() => {
    if (!opts) return;
    let alive = true;
    api
      .planDatabase({
        connectionId: connection.id,
        action: editing ? "alter" : "create",
        spec,
      })
      .then((s) => {
        if (!alive) return;
        setPreview(s);
        setPlanErr(s.length ? null : "没有可执行的变更");
      })
      .catch((e) => {
        if (!alive) return;
        setPreview([]);
        setPlanErr(String(e));
      });
    return () => {
      alive = false;
    };
  }, [opts, spec, editing, connection.id]);

  const caps = opts?.caps;
  const immutable = (field: string) => editing && !!caps?.immutableOnAlter?.includes(field);
  const targetName = (editing ? spec.newName || spec.name : spec.name).trim();

  const mysqlCollations = useMemo(() => {
    const all = opts?.collationNames || [];
    const cs = (spec.charset || "").trim();
    if (!cs) return all.slice(0, 200);
    return all.filter((c) => c.toLowerCase().startsWith(cs.toLowerCase())).slice(0, 200);
  }, [opts, spec.charset]);

  const apply = async () => {
    if (!targetName) {
      setErr("请填写数据库名称");
      setTab("general");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      const stmts = await api.applyDatabase({
        connectionId: connection.id,
        action: editing ? "alter" : "create",
        spec: { ...spec, name: spec.name, newName: editing ? targetName : null },
      });
      onApplied(stmts, mode);
      onClose();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleExtension = async (e: ExtInfo) => {
    const action = e.installedVersion ? "drop" : "create";
    setExtBusy(e.name);
    setExtMsg(null);
    try {
      await api.applyExtension({
        connectionId: connection.id,
        database: spec.name || database,
        action,
        name: e.name,
      });
      setExtMsg(
        `${action === "create" ? "已安装" : "已卸载"} ${e.name}`
      );
      loadExts();
    } catch (errText) {
      setExtMsg(String(errText));
    } finally {
      setExtBusy(null);
    }
  };

  const installed = exts.filter((e) => e.installedVersion);
  const available = exts.filter((e) => !e.installedVersion);
  const q = extQuery.trim().toLowerCase();
  const match = (e: ExtInfo) =>
    !q || e.name.toLowerCase().includes(q) || (e.comment || "").toLowerCase().includes(q);

  const tabs: { id: Tab; label: string; icon: typeof Search; show: boolean }[] = [
    { id: "general", label: "常规", icon: SlidersHorizontal, show: true },
    { id: "ext", label: "扩展", icon: Boxes, show: !!caps?.extensions },
    { id: "comment", label: "注释", icon: MessageSquare, show: !!caps?.comment },
    { id: "sql", label: "SQL 预览", icon: ScrollText, show: true },
  ];

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-[15px] font-bold tracking-tight">
            {editing ? `设计数据库 · ${database}` : "新建数据库"}
            <span className="conn-step">{connection.name}</span>
          </h2>
          <button className="btn btn-ghost btn-icon" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        {loadErr ? (
          <div className="error-banner">
            <span className="break-all flex-1">{loadErr}</span>
            <button title="关闭" onClick={onClose}>
              <X size={14} />
            </button>
          </div>
        ) : !opts ? (
          <div className="p-4 space-y-2">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="skeleton h-8" style={{ opacity: 1 - i * 0.1 }} />
            ))}
          </div>
        ) : (
          <>
            <div className="split-tabs mb-3" style={{ alignSelf: "flex-start", width: "fit-content" }}>
              {tabs
                .filter((t) => t.show)
                .map((t) => (
                  <button
                    key={t.id}
                    className={tab === t.id ? "active" : ""}
                    onClick={() => setTab(t.id)}
                  >
                    <t.icon size={13} /> {t.label}
                  </button>
                ))}
            </div>

            <div className="dd-body animate-fade-in" key={tab}>
              {tab === "general" && (
                <div className="dd-form stagger-in">
                  <Row label={editing ? "重命名" : "数据库名称"}>
                    <input
                      className="input mono"
                      value={editing ? spec.newName ?? "" : spec.name}
                      onChange={(e) =>
                        editing ? set("newName", e.target.value) : set("name", e.target.value)
                      }
                      placeholder={editing ? database : "例如 app_users"}
                    />
                  </Row>

                  {caps?.owner && (
                    <Row label="所有者">
                      <Pick
                        value={spec.owner}
                        options={opts.owners}
                        placeholder="（当前用户）"
                        onChange={(v) => set("owner", v)}
                      />
                    </Row>
                  )}

                  {caps?.template && (
                    <Row label="模板" hint="决定新库继承哪套系统目录与编码">
                      <Pick
                        value={spec.template}
                        options={opts.templates}
                        disabled={immutable("template")}
                        title={immutable("template") ? "建库后不可修改" : undefined}
                        onChange={(v) => set("template", v)}
                      />
                    </Row>
                  )}

                  {caps?.charset && (
                    <Row label="字符集">
                      <Pick
                        value={spec.charset}
                        options={opts.charsets}
                        onChange={(v) => {
                          set("charset", v);
                          set("collation", "");
                        }}
                      />
                    </Row>
                  )}

                  {caps?.charset && (
                    <Row label="排序规则">
                      <Pick
                        value={spec.collation}
                        options={mysqlCollations}
                        onChange={(v) => set("collation", v)}
                      />
                    </Row>
                  )}

                  {caps?.encoding && (
                    <Row label="编码">
                      <Pick
                        value={spec.encoding}
                        options={opts.encodings}
                        disabled={immutable("encoding")}
                        title={immutable("encoding") ? "建库后不可修改" : undefined}
                        onChange={(v) => set("encoding", v)}
                      />
                    </Row>
                  )}

                  {caps?.collate && (
                    <Row
                      label="排序规则 (LC_COLLATE)"
                      hint="PG 要求与模板库一致；要换排序规则请把模板选为 template0，否则服务端会判定不兼容"
                    >
                      <Pick
                        value={spec.collate}
                        options={opts.collations}
                        disabled={immutable("collate")}
                        title={immutable("collate") ? "建库后不可修改" : undefined}
                        onChange={(v) => set("collate", v)}
                      />
                    </Row>
                  )}

                  {caps?.ctype && (
                    <Row
                      label="字符分类 (LC_CTYPE)"
                      hint="同上：与模板库不一致时需要改用 template0 作为模板"
                    >
                      <Pick
                        value={spec.ctype}
                        options={opts.collations}
                        disabled={immutable("ctype")}
                        title={immutable("ctype") ? "建库后不可修改" : undefined}
                        onChange={(v) => set("ctype", v)}
                      />
                    </Row>
                  )}

                  {caps?.tablespace && (
                    <Row label="表空间">
                      <Pick
                        value={spec.tablespace}
                        options={opts.tablespaces}
                        disabled={immutable("tablespace")}
                        title={immutable("tablespace") ? "建库后不可修改" : undefined}
                        onChange={(v) => set("tablespace", v)}
                      />
                    </Row>
                  )}

                  {caps?.connLimit && (
                    <Row label="连接限制" hint="-1 表示不限制">
                      <input
                        className="input mono"
                        type="number"
                        value={spec.connLimit ?? -1}
                        onChange={(e) =>
                          set("connLimit", e.target.value === "" ? null : Number(e.target.value))
                        }
                      />
                    </Row>
                  )}

                  {caps?.allowConnect && (
                    <Row label="允许连接">
                      <input
                        type="checkbox"
                        checked={spec.allowConnect !== false}
                        onChange={(e) => set("allowConnect", e.target.checked)}
                      />
                    </Row>
                  )}

                  {caps?.isTemplate && (
                    <Row label="作为模板库">
                      <input
                        type="checkbox"
                        checked={!!spec.isTemplate}
                        onChange={(e) => set("isTemplate", e.target.checked)}
                      />
                    </Row>
                  )}
                </div>
              )}

              {tab === "ext" && !editing && (
                <div className="text-xs p-3" style={{ color: "var(--text-subtle)" }}>
                  先创建数据库，再对该库安装或卸载扩展
                </div>
              )}

              {tab === "ext" && editing && (
                <div>
                  <div className="flex items-center gap-2 mb-2">
                    <div className="relative flex-1">
                      <Search
                        size={12}
                        className="absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none"
                        style={{ color: "var(--text-subtle)" }}
                      />
                      <input
                        className="input"
                        style={{ paddingLeft: 26, fontSize: 12 }}
                        placeholder="搜索扩展…"
                        value={extQuery}
                        onChange={(e) => setExtQuery(e.target.value)}
                      />
                    </div>
                    {extMsg && (
                      <span className="tag tag-accent animate-fade-in">{extMsg}</span>
                    )}
                  </div>
                  <div className="ext-list">
                    {installed.filter(match).map((e) => (
                      <div className="ext-row" key={e.name}>
                        <span className="tag tag-accent flex-none">已安装</span>
                        <span className="mono text-xs font-semibold">{e.name}</span>
                        <span className="mono text-[11px]" style={{ color: "var(--text-subtle)" }}>
                          v{e.installedVersion}
                          {e.schema ? ` · ${e.schema}` : ""}
                        </span>
                        <span className="ext-desc">{e.comment || e.defaultVersion}</span>
                        <button
                          className="btn btn-ghost danger"
                          style={{ padding: "2px 8px", fontSize: 11 }}
                          disabled={extBusy === e.name}
                          onClick={() => toggleExtension(e)}
                        >
                          {extBusy === e.name ? "处理中" : "卸载"}
                        </button>
                      </div>
                    ))}
                    <div className="section-title mt-3 mb-1">可安装</div>
                    {available.filter(match).map((e) => (
                      <div className="ext-row" key={e.name}>
                        <span className="tag flex-none">未安装</span>
                        <span className="mono text-xs font-semibold">{e.name}</span>
                        <span className="mono text-[11px]" style={{ color: "var(--text-subtle)" }}>
                          v{e.defaultVersion}
                        </span>
                        <span className="ext-desc">{e.comment || ""}</span>
                        <button
                          className="btn"
                          style={{ padding: "2px 8px", fontSize: 11 }}
                          disabled={extBusy === e.name}
                          onClick={() => toggleExtension(e)}
                        >
                          {extBusy === e.name ? (
                            <Loader2 size={11} className="animate-spin" />
                          ) : (
                            "安装"
                          )}
                        </button>
                      </div>
                    ))}
                    {installed.filter(match).length === 0 &&
                      available.filter(match).length === 0 && (
                        <div className="text-xs p-3" style={{ color: "var(--text-subtle)" }}>
                          没有匹配的扩展
                        </div>
                      )}
                  </div>
                </div>
              )}

              {tab === "comment" && (
                <div>
                  <span className="field-label">数据库注释</span>
                  <textarea
                    className="sql-editor"
                    rows={6}
                    value={spec.comment || ""}
                    placeholder="留空会清除已有注释（COMMENT ON DATABASE … IS NULL）"
                    onChange={(e) => set("comment", e.target.value)}
                  />
                </div>
              )}

              {tab === "sql" && (
                <div>
                  {planErr && (
                    <div className="text-xs mb-2" style={{ color: "var(--warn)" }}>
                      {planErr}
                    </div>
                  )}
                  {preview.length === 0 && !planErr ? (
                    <div className="text-xs" style={{ color: "var(--text-subtle)" }}>
                      填写常规页后这里会显示将要执行的语句
                    </div>
                  ) : (
                    <pre className="rest-out" style={{ margin: 0, maxHeight: 300 }}>
                      {preview.join(";\n") + (preview.length ? ";" : "")}
                    </pre>
                  )}
                  <div className="text-[11px] mt-2" style={{ color: "var(--text-subtle)" }}>
                    预览与实际执行使用同一份语句，不会二次拼装
                  </div>
                </div>
              )}
            </div>

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
                将执行 {preview.length} 条语句
              </span>
              <div className="flex gap-2">
                <button className="btn" onClick={onClose}>
                  取消
                </button>
                <button className="btn btn-primary" disabled={busy} onClick={apply}>
                  {busy ? (
                    <Loader2 size={13} className="animate-spin" />
                  ) : (
                    <Check size={13} />
                  )}
                  {editing ? "保存" : "创建数据库"}
                </button>
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
