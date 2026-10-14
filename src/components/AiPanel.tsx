import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  Bot,
  Send,
  Plus,
  Trash2,
  X,
  Brain,
  MessageSquare,
  User,
  Sparkles,
  Search,
  ChevronDown,
  ChevronRight,
  History,
  RefreshCw,
  Database,
  ImagePlus,
  Crosshair,
} from "lucide-react";
import { api, type AiSettings, type ChatSession } from "../services/api";
import { useAppStore } from "../stores/appStore";
import { FilterSelect } from "./FilterSelect";

const AI_PROFILE_KEY = "db_tools_ai_profile";

const MAX_IMAGES = 4;
const MAX_IMAGE_BYTES = 8 * 1024 * 1024;

type PendingImage = { id: string; dataUrl: string; name: string; size: number };

const MB = (n: number) => `${(n / 1024 / 1024).toFixed(1)}MB`;

const newId = () =>
  typeof crypto?.randomUUID === "function"
    ? crypto.randomUUID()
    : `i${Date.now()}-${Math.random().toString(16).slice(2)}`;

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const fr = new FileReader();
    fr.onload = () => resolve(String(fr.result || ""));
    fr.onerror = () => reject(new Error(`读取「${file.name}」失败`));
    fr.readAsDataURL(file);
  });
}

/** 历史图片的 data URL 缓存：同一张图切换会话不必重复解码 */
const uriCache = new Map<string, string>();

/** 从后端按文件名取图片，加载中与丢失都要看得见 */
function MsgImage({
  name,
  onZoom,
}: {
  name: string;
  onZoom: (uri: string) => void;
}) {
  const [uri, setUri] = useState<string | null>(uriCache.get(name) ?? null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (uriCache.has(name)) {
      setUri(uriCache.get(name)!);
      return;
    }
    let alive = true;
    api
      .aiImageDataUri(name)
      .then((u) => {
        uriCache.set(name, u);
        if (alive) setUri(u);
      })
      .catch(() => alive && setFailed(true));
    return () => {
      alive = false;
    };
  }, [name]);

  if (failed)
    return (
      <span className="ai-img-missing" title={name}>
        图片已丢失
      </span>
    );
  if (!uri) return <span className="ai-img ai-img-loading" />;
  return (
    <img
      className="ai-img"
      src={uri}
      alt={name}
      loading="lazy"
      title="点击放大"
      onClick={() => onZoom(uri)}
    />
  );
}

const SUGGESTIONS = [
  "看下当前范围里有哪些表",
  "查询 users 表前 10 条",
  "解释一下我粘进来的这张报错截图",
  "记住我偏好中文回复",
];

function formatTime(iso: string) {
  try {
    const d = new Date(iso);
    return d.toLocaleString("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

export function AiPanel() {
  const {
    aiOpen,
    setAiOpen,
    setError,
    connections,
    activeConnectionId,
    activeDatabase,
    activeTable,
  } = useAppStore();
  const [sessions, setSessions] = useState<ChatSession[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [facts, setFacts] = useState<{ id: string; content: string }[]>([]);
  const [historyOpen, setHistoryOpen] = useState(true);
  const [historyFilter, setHistoryFilter] = useState("");
  const [expandedIds, setExpandedIds] = useState<Record<string, boolean>>({});
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [profiles, setProfiles] = useState<AiSettings[]>([]);
  const [profilesLoading, setProfilesLoading] = useState(false);
  const [profilesError, setProfilesError] = useState("");
  const [profileId, setProfileId] = useState(
    () => localStorage.getItem(AI_PROFILE_KEY) || ""
  );
  const [databases, setDatabases] = useState<string[]>([]);
  const [dbLoading, setDbLoading] = useState(false);
  const [pending, setPending] = useState<PendingImage[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const [lightbox, setLightbox] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  // 还没建会话时先记在选择器上，新建对话后再落到会话
  const [draftScope, setDraftScope] = useState<{
    connection_id: string | null;
    database: string | null;
    table: string | null;
  }>({ connection_id: null, database: null, table: null });

  const active = sessions.find((s) => s.id === activeId) || null;
  const scope = active
    ? {
        connection_id: active.connection_id ?? null,
        database: active.database ?? null,
        table: active.table ?? null,
      }
    : draftScope;
  const activeProfile =
    profiles.find((p) => p.id === profileId) ||
    profiles.find((p) => p.is_default) ||
    profiles[0] ||
    null;

  const loadProfiles = async () => {
    setProfilesLoading(true);
    setProfilesError("");
    try {
      const list = await api.listAiProfiles();
      setProfiles(list);
      setProfileId((cur) => {
        const chosen = list.find((p) => p.id === cur);
        const next = chosen || list.find((p) => p.is_default) || list[0];
        const id = next?.id || "";
        if (id !== cur) localStorage.setItem(AI_PROFILE_KEY, id);
        return id;
      });
    } catch (e) {
      setProfiles([]);
      setProfilesError(String(e));
    } finally {
      setProfilesLoading(false);
    }
  };

  /** 改会话范围：没有会话时先记在草稿上，新建对话后再落库 */
  const setScope = async (
    patch: Partial<{
      connection_id: string | null;
      database: string | null;
      table: string | null;
    }>
  ) => {
    const next = { ...scope, ...patch };
    if (!activeId) {
      setDraftScope(next);
      return;
    }
    try {
      const updated = await api.setChatScope(
        activeId,
        next.connection_id,
        next.database,
        next.table
      );
      setSessions((l) => l.map((s) => (s.id === updated.id ? updated : s)));
    } catch (e) {
      setError(String(e));
    }
  };

  // 换连接就重取库列表；慢响应回来时若已换开就丢掉
  useEffect(() => {
    const cid = scope.connection_id;
    if (!cid) {
      setDatabases([]);
      return;
    }
    let alive = true;
    setDbLoading(true);
    api
      .listDatabases(cid)
      .then((list) => {
        if (alive) setDatabases(list);
      })
      .catch((e) => {
        if (!alive) return;
        setDatabases([]);
        setError(String(e));
      })
      .finally(() => {
        if (alive) setDbLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [scope.connection_id]);

  const followBrowser = () => {
    if (!activeConnectionId || !activeDatabase) {
      setError("左侧还没选中数据库：先在树上展开连接并点一个库");
      return;
    }
    void setScope({
      connection_id: activeConnectionId,
      database: activeDatabase,
      table: activeTable,
    });
  };

  const addImages = async (files: File[]) => {
    const imgs = files.filter((f) => f.type.startsWith("image/"));
    if (!imgs.length) {
      setError("只能添加图片文件（png / jpg / gif / webp / bmp）");
      return;
    }
    const room = MAX_IMAGES - pending.length;
    if (room <= 0) {
      setError(`一条消息最多 ${MAX_IMAGES} 张图片`);
      return;
    }
    const taken = imgs.slice(0, room);
    if (taken.length < imgs.length)
      setError(`一条消息最多 ${MAX_IMAGES} 张，已忽略多出的 ${imgs.length - taken.length} 张`);
    const next: PendingImage[] = [];
    for (const f of taken) {
      if (f.size > MAX_IMAGE_BYTES) {
        setError(`「${f.name}」${MB(f.size)}，超过单张 ${MB(MAX_IMAGE_BYTES)} 上限`);
        continue;
      }
      try {
        next.push({
          id: newId(),
          dataUrl: await readFileAsDataUrl(f),
          name: f.name,
          size: f.size,
        });
      } catch (e) {
        setError(String(e));
      }
    }
    if (next.length) setPending((p) => [...p, ...next]);
  };

  const filteredSessions = useMemo(() => {
    const q = historyFilter.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => {
      if (s.title.toLowerCase().includes(q)) return true;
      return s.messages.some(
        (m) =>
          m.content.toLowerCase().includes(q) ||
          m.role.toLowerCase().includes(q)
      );
    });
  }, [sessions, historyFilter]);

  const refresh = async () => {
    try {
      const [ss, fs] = await Promise.all([
        api.listChatSessions(),
        api.listMemoryFacts(),
      ]);
      setSessions(ss);
      setFacts(fs);
      if (!activeId && ss[0]) setActiveId(ss[0].id);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    if (aiOpen) {
      refresh();
      void loadProfiles();
    }
  }, [aiOpen]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [active?.messages.length, busy]);

  useEffect(() => {
    if (!lightbox) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setLightbox(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [lightbox]);

  const newSession = async () => {
    const s = await api.createChatSession();
    setSessions((list) => [s, ...list]);
    setActiveId(s.id);
  };

  const toggleExpand = (id: string) => {
    setExpandedIds((e) => ({ ...e, [id]: !e[id] }));
  };

  const removeSession = async (id: string) => {
    await api.deleteChatSession(id);
    setSessions((l) => l.filter((x) => x.id !== id));
    if (activeId === id) setActiveId(null);
    setConfirmDeleteId(null);
  };

  const send = async (preset?: string) => {
    const text = (preset ?? input).trim();
    const imgs = preset ? [] : pending;
    if ((!text && imgs.length === 0) || busy) return;
    setBusy(true);
    setInput("");
    setPending([]);
    try {
      let sid = activeId;
      if (!sid) {
        const s = await api.createChatSession(text.slice(0, 24) || "图片提问");
        setSessions((list) => [s, ...list]);
        sid = s.id;
        setActiveId(s.id);
        if (draftScope.connection_id || draftScope.database) {
          const scoped = await api.setChatScope(
            s.id,
            draftScope.connection_id,
            draftScope.database,
            draftScope.table
          );
          setSessions((list) => list.map((x) => (x.id === scoped.id ? scoped : x)));
        }
      }
      const updated = await api.aiChat(
        sid,
        text,
        activeProfile?.id || profileId || null,
        imgs.map((i) => i.dataUrl)
      );
      setSessions((list) => list.map((s) => (s.id === updated.id ? updated : s)));
    } catch (e) {
      // 消息没出去，附件留在输入区好重试
      setPending((cur) => (cur.length ? cur : imgs));
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside
      className={`ai-panel h-full border-l flex flex-col min-w-0 overflow-hidden ${
        aiOpen ? "animate-slide-in-right" : "ai-panel-collapsed"
      }`}
      aria-hidden={!aiOpen}
      inert={!aiOpen}
      style={{ background: "var(--bg-panel)", borderColor: "var(--border)" }}
    >
      <div
        className="flex items-center gap-2 px-3 h-12 border-b flex-none"
        style={{ borderColor: "var(--border)" }}
      >
        <span className="brand-mark" style={{ width: 24, height: 24, borderRadius: 8 }}>
          <Bot size={13} />
        </span>
        <div className="flex flex-col leading-tight flex-1">
          <span className="font-semibold text-[12.5px]">AI 智能体</span>
          <span className="text-[10px]" style={{ color: "var(--text-subtle)" }}>
            自然语言操作数据库
          </span>
        </div>
        <button className="btn btn-ghost btn-icon" title="新对话" onClick={newSession}>
          <Plus size={15} />
        </button>
        <button
          className="btn btn-ghost btn-icon"
          title="关闭"
          onClick={() => setAiOpen(false)}
        >
          <X size={15} />
        </button>
      </div>

      {/* 模型切换：多套配置随时换 */}
      <div
        className="flex items-center gap-2 px-3 py-2 border-b flex-none"
        style={{ borderColor: "var(--border)", background: "var(--bg-elevated)" }}
      >
        <span
          className="text-[11px] flex-none"
          style={{ color: "var(--text-subtle)" }}
        >
          模型
        </span>
        <FilterSelect
          className="ai-switch"
          value={activeProfile?.id || ""}
          disabled={profilesLoading}
          placeholder={
            profilesLoading ? "读取配置…" : profilesError ? "读取失败" : "选择模型"
          }
          emptyText={
            profilesError
              ? "读取配置失败，重新打开面板重试"
              : "还没有配置，去「设置 → 大模型」新增"
          }
          options={profiles.map((p) => ({
            value: p.id || "",
            label: (p.name || "").trim() || p.model,
            hint: `${p.provider === "anthropic" ? "Anthropic" : "OpenAI"}${
              p.is_default ? " · 默认" : ""
            }`,
          }))}
          onChange={(v) => {
            setProfileId(v);
            localStorage.setItem(AI_PROFILE_KEY, v);
          }}
        />
        {activeProfile && !activeProfile.api_key && (
          <span className="tag" style={{ color: "var(--warn)" }}>
            缺 API Key
          </span>
        )}
        <button
          className="btn btn-ghost btn-icon"
          title="重新读取模型配置"
          onClick={() => void loadProfiles()}
          disabled={profilesLoading}
        >
          <RefreshCw
            size={13}
            className={profilesLoading ? "animate-spin" : ""}
          />
        </button>
      </div>

      {/* 会话取数范围：选一次，工具调用就不必再翻连接列表 */}
      <div
        className="ai-scope"
        style={{ borderColor: "var(--border)", background: "var(--bg-elevated)" }}
      >
        <div className="ai-scope-row">
          <span className="ai-scope-label">
            <Database size={11} /> 范围
          </span>
          <FilterSelect
            className="ai-switch"
            value={scope.connection_id || ""}
            placeholder="连接"
            emptyText={
              connections.length ? "没有匹配的连接" : "还没有保存的连接"
            }
            options={connections.map((c) => ({
              value: c.id,
              label: c.name,
              hint: c.kind,
            }))}
            onChange={(v) =>
              void setScope({
                connection_id: v || null,
                database: null,
                table: null,
              })
            }
          />
          <FilterSelect
            className="ai-switch"
            value={scope.database || ""}
            disabled={!scope.connection_id || dbLoading}
            placeholder={
              dbLoading
                ? "读取库…"
                : scope.connection_id
                ? "数据库"
                : "先选连接"
            }
            emptyText={dbLoading ? "正在读取库列表…" : "这个连接没有可选库"}
            options={databases.map((d) => ({ value: d, label: d }))}
            onChange={(v) => void setScope({ database: v || null, table: null })}
          />
          <button
            className="btn btn-ghost btn-icon"
            title="跟随左侧树上选中的库与表"
            onClick={followBrowser}
          >
            <Crosshair size={13} />
          </button>
          <button
            className="btn btn-ghost btn-icon"
            title="清除范围，改由智能体自己问你要"
            onClick={() =>
              void setScope({
                connection_id: null,
                database: null,
                table: null,
              })
            }
            disabled={!scope.connection_id && !scope.database && !scope.table}
          >
            <X size={13} />
          </button>
        </div>
        {dbLoading && (
          <div className="ai-scope-note">
            <RefreshCw size={10} className="animate-spin" /> 正在读取该连接的库列表
          </div>
        )}
        {scope.connection_id && !dbLoading && databases.length === 0 && (
          <div className="ai-scope-note" style={{ color: "var(--warn)" }}>
            没读到库列表，可先手动输入 SQL 或换个连接
          </div>
        )}
        {scope.table && (
          <div className="ai-scope-row">
            <span className="tag">
              当前表 <b className="mono">{scope.table}</b>
            </span>
            <button
              className="btn btn-ghost"
              style={{ fontSize: 10, padding: "2px 6px" }}
              onClick={() => void setScope({ table: null })}
            >
              不带上表
            </button>
          </div>
        )}
      </div>

      {/* 历史对话：可折叠列表 + 搜索 */}
      <div className="border-b flex-none" style={{ borderColor: "var(--border)" }}>
        <button
          type="button"
          className="w-full flex items-center gap-1.5 px-3 py-2 text-left"
          onClick={() => setHistoryOpen((v) => !v)}
          style={{ color: "var(--text-muted)" }}
        >
          {historyOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
          <History size={13} />
          <span className="text-xs font-semibold flex-1">历史对话</span>
          <span className="tag">{sessions.length}</span>
        </button>

        {historyOpen && (
          <div className="px-2.5 pb-2 space-y-1.5 animate-fade-up stagger-in">
            <div className="relative">
              <Search
                size={12}
                className="absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none"
                style={{ color: "var(--text-subtle)" }}
              />
              <input
                className="input"
                style={{ paddingLeft: 26, paddingTop: 4, paddingBottom: 4, fontSize: 11.5 }}
                placeholder="搜索标题或消息内容…"
                value={historyFilter}
                onChange={(e) => setHistoryFilter(e.target.value)}
              />
            </div>

            <div className="max-h-44 overflow-y-auto space-y-0.5">
              {filteredSessions.length === 0 && (
                <div
                  className="text-[11px] px-1 py-2 text-center"
                  style={{ color: "var(--text-subtle)" }}
                >
                  {historyFilter ? "没有匹配的对话" : "暂无历史对话"}
                </div>
              )}
              {filteredSessions.map((s) => {
                const on = s.id === activeId;
                const expanded = !!expandedIds[s.id];
                const last = s.messages[s.messages.length - 1];
                const preview = last ? last.content.slice(0, 100) : "（空对话）";
                return (
                  <div
                    key={s.id}
                    className="rounded-lg"
                    style={{
                      border: "1px solid",
                      borderColor: on
                        ? "color-mix(in srgb, var(--accent) 45%, var(--border))"
                        : "var(--border)",
                      background: on ? "var(--bg-active)" : "transparent",
                    }}
                  >
                    <div className="flex items-center gap-1 px-2 py-1.5">
                      <button
                        type="button"
                        className="flex-none p-0.5"
                        title={expanded ? "收起预览" : "展开预览"}
                        onClick={() => toggleExpand(s.id)}
                      >
                        {expanded ? (
                          <ChevronDown size={12} style={{ color: "var(--text-subtle)" }} />
                        ) : (
                          <ChevronRight size={12} style={{ color: "var(--text-subtle)" }} />
                        )}
                      </button>
                      <button
                        type="button"
                        className="flex-1 min-w-0 text-left"
                        onClick={() => setActiveId(s.id)}
                        title={s.title}
                      >
                        <div className="flex items-center gap-1.5">
                          <MessageSquare
                            size={11}
                            style={{ color: on ? "var(--accent)" : "var(--text-subtle)" }}
                          />
                          <span
                            className="text-[11.5px] truncate font-medium"
                            style={{ color: on ? "var(--text)" : "var(--text-muted)" }}
                          >
                            {s.title}
                          </span>
                        </div>
                        <div
                          className="text-[10px] truncate mt-0.5"
                          style={{ color: "var(--text-subtle)" }}
                        >
                          {formatTime(s.updated_at || s.created_at)} · {s.messages.length} 条
                        </div>
                      </button>
                      <button
                        className="btn btn-ghost flex-none"
                        style={{ padding: "2px 4px" }}
                        title="删除对话"
                        onClick={() => setConfirmDeleteId(s.id)}
                      >
                        <Trash2 size={11} style={{ color: "var(--danger)" }} />
                      </button>
                    </div>
                    {expanded && (
                      <div
                        className="px-2.5 pb-2 text-[10.5px]"
                        style={{
                          color: "var(--text-muted)",
                          borderTop: "1px solid var(--border)",
                        }}
                      >
                        <div className="pt-1.5" style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
                          {preview}
                          {last && last.content.length > 100 ? "…" : ""}
                        </div>
                        <div className="mt-1 flex gap-1.5">
                          <button
                            className="btn btn-ghost"
                            style={{ fontSize: 10, padding: "2px 6px" }}
                            onClick={() => {
                              setActiveId(s.id);
                              setHistoryOpen(false);
                            }}
                          >
                            继续对话
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>

      {confirmDeleteId && (
        <div
          className="px-3 py-2 border-b text-[11px] flex items-center gap-2 flex-none"
          style={{ borderColor: "var(--border)", background: "var(--bg-elevated)" }}
        >
          <span style={{ color: "var(--text-muted)" }}>确认删除该对话？</span>
          <div className="flex-1" />
          <button className="btn" onClick={() => setConfirmDeleteId(null)}>
            取消
          </button>
          <button
            className="btn btn-danger"
            onClick={() => void removeSession(confirmDeleteId)}
          >
            删除
          </button>
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-3.5 space-y-4 min-h-0">
        {!active || active.messages.length === 0 ? (
          <div className="empty-state" style={{ paddingTop: 40 }}>
            <div className="empty-icon" style={{ color: "var(--accent)" }}>
              <Sparkles size={24} />
            </div>
            <div className="text-sm font-semibold" style={{ color: "var(--text)" }}>
              你好，我是数据库助手
            </div>
            <div className="text-xs max-w-[240px]">
              可以用自然语言连接库、查表、执行 SQL、导入导出
            </div>
            <div className="flex flex-wrap gap-1.5 justify-center mt-3">
              {SUGGESTIONS.map((s) => (
                <button
                  key={s}
                  className="tag cursor-pointer"
                  onClick={() => void send(s)}
                  disabled={busy}
                >
                  {s}
                </button>
              ))}
            </div>
          </div>
        ) : (
          active.messages.map((m) => (
            <div key={m.id} className="space-y-1.5 chat-msg group">
              <div className="flex items-center gap-1.5">
                <div
                  className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide"
                  style={{ color: "var(--text-subtle)" }}
                >
                  {m.role === "user" ? (
                    <>
                      <User size={11} />
                      你
                    </>
                  ) : (
                    <>
                      <Bot size={11} />
                      AI
                    </>
                  )}
                </div>
                <div className="flex-1" />
                <button
                  type="button"
                  className="btn btn-ghost copy-btn"
                  style={{ padding: "2px 6px", fontSize: 10 }}
                  title="复制消息"
                  onClick={async () => {
                    try {
                      await navigator.clipboard.writeText(m.content);
                    } catch {
                      // fallback
                      const ta = document.createElement("textarea");
                      ta.value = m.content;
                      document.body.appendChild(ta);
                      ta.select();
                      document.execCommand("copy");
                      document.body.removeChild(ta);
                    }
                  }}
                >
                  复制
                </button>
              </div>
              <div
                className={`chat-bubble text-xs ${
                  m.role === "user" ? "chat-user" : "chat-assistant"
                }`}
              >
                {m.content}
              </div>
              {m.images && m.images.length > 0 && (
                <div className="ai-msg-imgs">
                  {m.images.map((n) => (
                    <MsgImage key={n} name={n} onZoom={setLightbox} />
                  ))}
                </div>
              )}
            </div>
          ))
        )}
        {busy && (
          <div className="flex gap-2">
            <span
              className="flex-none grid place-items-center rounded-full"
              style={{
                width: 22,
                height: 22,
                background: "var(--accent-soft)",
                color: "var(--accent-strong)",
                border: "1px solid var(--border)",
              }}
            >
              <Bot size={12} />
            </span>
            <div className="chat-bubble chat-assistant text-xs flex items-center gap-1.5">
              <span className="typing-dot" />
              <span className="typing-dot" />
              <span className="typing-dot" />
            </div>
          </div>
        )}
        <div ref={bottomRef} />
      </div>

      <div
        className={`p-3 border-t flex-none ${dragOver ? "ai-dragover" : ""}`}
        style={{ borderColor: "var(--border)" }}
        onDragOver={(e) => {
          if (!e.dataTransfer?.types?.includes("Files")) return;
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(e) => {
          const files = Array.from(e.dataTransfer?.files || []);
          if (!files.length) return;
          e.preventDefault();
          setDragOver(false);
          void addImages(files);
        }}
      >
        <input
          ref={fileRef}
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp,image/bmp"
          multiple
          style={{ display: "none" }}
          onChange={(e) => {
            const files = Array.from(e.target.files || []);
            e.target.value = "";
            if (files.length) void addImages(files);
          }}
        />

        {pending.length > 0 && (
          <div className="ai-pending">
            {pending.map((p) => (
              <div
                key={p.id}
                className="ai-pending-item"
                title={`${p.name} · ${MB(p.size)}`}
              >
                <img src={p.dataUrl} alt={p.name} />
                <button
                  type="button"
                  className="ai-pending-x"
                  title="移除这张图"
                  onClick={() =>
                    setPending((list) => list.filter((x) => x.id !== p.id))
                  }
                >
                  <X size={10} />
                </button>
              </div>
            ))}
            <span className="ai-pending-tip">
              {pending.length}/{MAX_IMAGES} 张 · 单张≤{MB(MAX_IMAGE_BYTES)}
            </span>
          </div>
        )}

        <div
          className="flex items-end gap-2 rounded-xl p-1.5"
          style={{
            border: "1px solid var(--border)",
            background: "var(--bg-elevated)",
          }}
        >
          <button
            className="btn btn-ghost btn-icon"
            style={{ borderRadius: 10 }}
            title="添加图片（也可粘贴或拖进来）"
            onClick={() => fileRef.current?.click()}
            disabled={busy || pending.length >= MAX_IMAGES}
          >
            <ImagePlus size={14} />
          </button>
          <textarea
            className="flex-1 bg-transparent outline-none resize-none px-2 py-1 text-[12.5px]"
            rows={2}
            placeholder="描述你想做的数据库操作…  (Enter 发送，可直接粘贴图片)"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onPaste={(e) => {
              const files = Array.from(e.clipboardData?.files || []);
              if (!files.length) return;
              e.preventDefault();
              void addImages(files);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
          />
          <button
            className="btn btn-primary btn-icon"
            style={{ borderRadius: 10 }}
            onClick={() => void send()}
            disabled={busy || (!input.trim() && pending.length === 0)}
            title={pending.length ? "发送（含图片）" : "发送"}
          >
            <Send size={14} />
          </button>
        </div>
        {facts.length > 0 && (
          <div
            className="mt-2 flex items-center gap-1.5 text-[10px]"
            style={{ color: "var(--text-subtle)" }}
          >
            <Brain size={11} style={{ color: "var(--accent)" }} />
            已启用长期记忆 · {facts.length} 条
          </div>
        )}
      </div>

      {/* 放大看图：面板有 transform 动画，浮层必须挂到 body 上才不受裁剪 */}
      {lightbox &&
        createPortal(
          <div
            className="ai-lightbox"
            role="dialog"
            aria-label="图片预览"
            onClick={() => setLightbox(null)}
          >
            <img src={lightbox} alt="图片预览" onClick={(e) => e.stopPropagation()} />
            <button
              className="btn btn-ghost btn-icon ai-lightbox-x"
              title="关闭（Esc）"
              onClick={() => setLightbox(null)}
            >
              <X size={16} />
            </button>
          </div>,
          document.body
        )}
    </aside>
  );
}
