import { useEffect, useRef, useState } from "react";
import { Copy, RotateCcw, Star, Trash2, X } from "lucide-react";
import { api, type AiSettings } from "../services/api";
import { FilterSelect, type FsOption } from "./FilterSelect";
import { useAppStore } from "../stores/appStore";

// 一个字体设置同时管界面和代码，所以等宽字体也列在这里
const FONTS: FsOption[] = [
  { value: "", label: "默认（跟随系统）", hint: "界面优先" },
  { value: "Segoe UI", label: "Segoe UI", hint: "Windows 界面" },
  { value: "Microsoft YaHei UI", label: "微软雅黑", hint: "中文界面" },
  { value: "PingFang SC", label: "苹方", hint: "macOS 中文" },
  { value: "HarmonyOS Sans SC", label: "鸿蒙黑体", hint: "中文界面" },
  { value: "Source Han Sans SC", label: "思源黑体", hint: "中文界面" },
  { value: "Sarasa Gothic SC", label: "更纱黑体 SC", hint: "中文界面" },
  { value: "Inter", label: "Inter", hint: "西文界面" },
  { value: "JetBrains Mono", label: "JetBrains Mono", hint: "等宽" },
  { value: "Cascadia Code", label: "Cascadia Code", hint: "等宽" },
  { value: "Cascadia Mono", label: "Cascadia Mono", hint: "等宽" },
  { value: "Consolas", label: "Consolas", hint: "等宽" },
  { value: "Noto Sans Mono CJK SC", label: "思源等宽", hint: "等宽中文" },
  { value: "SF Mono", label: "SF Mono", hint: "等宽" },
  { value: "Menlo", label: "Menlo", hint: "等宽" },
];

const CUSTOM_FONT = "__custom__";

export function SettingsModal({ onClose }: { onClose: () => void }) {
  const { theme, setTheme, fontSize, setFontSize, fontFamily, setFontFamily } =
    useAppStore();
  const [tab, setTab] = useState<"general" | "ai">("general");
  const [profiles, setProfiles] = useState<AiSettings[]>([]);
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [loadErr, setLoadErr] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [testMsg, setTestMsg] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [customFont, setCustomFont] = useState(
    () => fontFamily !== "" && !FONTS.some((f) => f.value === fontFamily)
  );
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    api
      .listAiProfiles()
      .then((list) => {
        if (!mounted.current) return;
        setProfiles(list);
        const def = list.find((p) => p.is_default) || list[0] || null;
        setSettings(def);
      })
      .catch((e) => {
        if (mounted.current) setLoadErr(String(e));
      });
    return () => {
      mounted.current = false;
    };
  }, []);

  const patch = (p: Partial<AiSettings>) =>
    setSettings((s) => (s ? { ...s, ...p } : s));

  const profileLabel = (p: AiSettings) =>
    (p.name || "").trim() || `${p.model || "未填模型"}（${p.provider === "anthropic" ? "Anthropic" : "OpenAI 兼容"}）`;

  const isDirty = (p: AiSettings) =>
    JSON.stringify({ ...p, id: "", is_default: false }) !==
    JSON.stringify({ ...settings, id: "", is_default: false });

  const switchProfile = (id: string) => {
    const hit = profiles.find((p) => p.id === id);
    if (!hit) return;
    if (settings && isDirty(hit)) {
      setTestMsg(`已切换到「${profileLabel(hit)}」，之前的未保存修改已丢弃`);
    } else {
      setTestMsg(null);
    }
    setConfirmDelete(false);
    setSettings(hit);
  };

  const duplicateAsNew = () => {
    const base = settings ?? profiles[0];
    const fresh: AiSettings = {
      ...(base ?? ({} as AiSettings)),
      id: "",
      name: "",
      is_default: false,
    };
    setConfirmDelete(false);
    setSettings(fresh);
    setTestMsg("以当前配置为模板新建，请填名称与 Key 后保存");
  };

  const applyList = (list: AiSettings[], preferId?: string) => {
    setProfiles(list);
    const keep = preferId ? list.find((p) => p.id === preferId) : undefined;
    const next = keep || list.find((p) => p.is_default) || list[0] || null;
    setSettings(next);
  };

  const saveAi = async () => {
    if (!settings) return;
    setSaving(true);
    try {
      const wasNew = !settings.id;
      const res = await api.saveAiProfile(settings);
      setProfiles(res.profiles);
      setSettings(res.profile);
      setTestMsg(
        `${wasNew ? "已新增" : "已保存"}「${profileLabel(res.profile)}」${
          res.profile.is_default ? "（默认）" : ""
        }`
      );
    } catch (e) {
      setTestMsg(String(e));
    } finally {
      setSaving(false);
    }
  };

  const removeAi = async () => {
    if (!settings?.id) {
      setSettings(null);
      return;
    }
    try {
      const list = await api.deleteAiProfile(settings.id);
      setConfirmDelete(false);
      applyList(list);
      setTestMsg("已删除该配置");
    } catch (e) {
      setTestMsg(String(e));
    }
  };

  const makeDefault = async () => {
    if (!settings?.id) return;
    try {
      const list = await api.setDefaultAiProfile(settings.id);
      applyList(list, settings.id);
      setTestMsg(`已将「${profileLabel(settings)}」设为默认`);
    } catch (e) {
      setTestMsg(String(e));
    }
  };

  const testAi = async () => {
    if (!settings) return;
    setTesting(true);
    setTestMsg(null);
    try {
      const r = await api.testAiConnection(settings);
      setTestMsg(`测试成功：${r.slice(0, 80)}`);
    } catch (e) {
      setTestMsg(String(e));
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-[15px] font-bold tracking-tight">设置</h3>
          <button className="btn btn-ghost btn-icon" onClick={onClose}>
            <X size={15} />
          </button>
        </div>

        <div className="split-tabs mb-4" style={{ width: "fit-content" }}>
          <button
            type="button"
            className={tab === "general" ? "active" : ""}
            onClick={() => setTab("general")}
          >
            外观与字体
          </button>
          <button
            type="button"
            className={tab === "ai" ? "active" : ""}
            onClick={() => setTab("ai")}
          >
            大模型
          </button>
        </div>

        {tab === "general" && (
          <div className="space-y-4">
            <div>
              <span className="field-label">主题</span>
              <div className="flex gap-2 mt-1.5">
                {(["light", "dark"] as const).map((t) => (
                  <button
                    key={t}
                    type="button"
                    className={`btn ${theme === t ? "btn-primary" : ""}`}
                    onClick={() => setTheme(t)}
                  >
                    {t === "light" ? "浅色" : "深色"}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <span className="field-label">字体（界面与代码共用）</span>
              <div className="flex gap-2 mt-1">
                <FilterSelect
                  className="flex-1 min-w-0"
                  value={customFont ? CUSTOM_FONT : fontFamily}
                  placeholder="选择字体"
                  options={[
                    {
                      value: CUSTOM_FONT,
                      label: "手动输入字体名…",
                      hint: "本机已安装",
                    },
                    ...FONTS,
                  ]}
                  onChange={(v) => {
                    if (v === CUSTOM_FONT) {
                      setCustomFont(true);
                      if (!fontFamily) setFontFamily("Arial");
                      return;
                    }
                    setCustomFont(false);
                    setFontFamily(v);
                  }}
                />
                <button
                  className="btn"
                  title="恢复默认字体与字号"
                  onClick={() => {
                    setCustomFont(false);
                    setFontFamily("");
                    setFontSize(13);
                  }}
                >
                  <RotateCcw size={13} />
                </button>
              </div>
              {customFont && (
                <input
                  className="input mono mt-2"
                  placeholder="字体名，如 Sarasa Gothic SC"
                  value={fontFamily}
                  onChange={(e) => setFontFamily(e.target.value)}
                />
              )}
            </div>

            <div>
              <span className="field-label">
                字号 <span className="mono">{fontSize}px</span>
                <span style={{ color: "var(--text-subtle)" }}>
                  {" "}
                  · 界面与代码共用
                </span>
              </span>
              <div className="flex items-center gap-2 mt-1">
                <input
                  type="range"
                  min={11}
                  max={24}
                  step={1}
                  value={fontSize}
                  onChange={(e) => setFontSize(Number(e.target.value))}
                  style={{ flex: 1 }}
                />
                <input
                  className="input mono"
                  type="number"
                  min={11}
                  max={24}
                  step={1}
                  value={fontSize}
                  onChange={(e) => setFontSize(Number(e.target.value))}
                  style={{ width: 68 }}
                />
              </div>
            </div>

            <div className="font-preview">
              <div>数据库工具 · 表 users 的字段名</div>
              <div className="mono">SELECT id, name FROM users WHERE id = 1;</div>
            </div>

            <p className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
              字体和字号各只有一个设置，界面、表格、SQL
              编辑器一起生效；本机没装的字体自动回退到默认。改动立即生效并保存在本机。
            </p>
          </div>
        )}

        {tab === "ai" && (
          <div className="space-y-3">
            {loadErr && (
              <div className="text-[11px]" style={{ color: "var(--danger)" }}>
                读取模型配置失败：{loadErr}
              </div>
            )}

            <div className="ai-profile-bar">
              <div className="flex items-center gap-2 flex-wrap">
                <FilterSelect
                  className="flex-1 min-w-0"
                  value={settings?.id || ""}
                  placeholder="选择模型配置"
                  emptyText={loadErr ? "读取失败" : "还没有配置"}
                  options={profiles.map((p) => ({
                    value: p.id || "",
                    label: profileLabel(p),
                    hint: p.is_default ? `默认 · ${p.model}` : p.model,
                  }))}
                  onChange={switchProfile}
                />
                <button
                  className="btn"
                  onClick={duplicateAsNew}
                  title="以当前配置为模板新建一条"
                >
                  <Copy size={13} /> 新增
                </button>
                <button
                  className={`btn ${settings?.is_default ? "btn-primary" : ""}`}
                  onClick={makeDefault}
                  disabled={!settings?.id || settings?.is_default}
                  title="智能体和导出默认用这条配置"
                >
                  <Star size={13} /> 设为默认
                </button>
                <button
                  className={`btn ${confirmDelete ? "btn-danger" : ""}`}
                  onClick={() => (confirmDelete ? void removeAi() : setConfirmDelete(true))}
                  disabled={!settings}
                  title="删除这条模型配置"
                >
                  <Trash2 size={13} /> {confirmDelete ? "确认删除" : "删除"}
                </button>
              </div>
              <div className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
                可以保存多套配置，只有「默认」这条作为智能体的初始模型；对话时在右侧智能体面板随时切换。
              </div>
            </div>

            {!settings ? (
              <div className="text-xs p-3" style={{ color: "var(--text-subtle)" }}>
                还没有可用的模型配置，点「新增」创建一条。
              </div>
            ) : (
              <>
            <label className="block">
              <span className="field-label">配置名称（智能体切换列表里显示）</span>
              <input
                className="input mt-1"
                placeholder="例如：Doubao 编码版 / 本地 Ollama"
                value={settings.name || ""}
                onChange={(e) => patch({ name: e.target.value })}
              />
            </label>
            <label className="block">
              <span className="field-label">协议</span>
              <FilterSelect
                className="mt-1"
                value={settings.provider}
                placeholder="选择协议"
                options={[
                  {
                    value: "openai",
                    label: "OpenAI 兼容",
                    hint: "/chat/completions",
                  },
                  {
                    value: "anthropic",
                    label: "Anthropic",
                    hint: "/v1/messages",
                  },
                ]}
                onChange={(p) => {
                  setSettings({
                    ...settings,
                    provider: p,
                    base_url:
                      p === "anthropic"
                        ? "https://ark.cn-beijing.volces.com/api/coding"
                        : "https://ark.cn-beijing.volces.com/api/coding/v3",
                    model: settings.model || "doubao-seed-1.6",
                    api_style: "chat_completions",
                  });
                }}
              />
            </label>
            <label className="block">
              <span className="field-label">Base URL</span>
              <input
                className="input mt-1 mono"
                value={settings.base_url}
                onChange={(e) => setSettings({ ...settings, base_url: e.target.value })}
              />
            </label>
            <label className="block">
              <span className="field-label">API Key</span>
              <input
                className="input mt-1 mono"
                type="password"
                value={settings.api_key}
                onChange={(e) => setSettings({ ...settings, api_key: e.target.value })}
              />
            </label>
            <label className="block">
              <span className="field-label">模型</span>
              <input
                className="input mt-1 mono"
                value={settings.model}
                onChange={(e) => setSettings({ ...settings, model: e.target.value })}
              />
            </label>
            <div className="grid grid-cols-2 gap-3">
              <label className="block">
                <span className="field-label">温度 {settings.temperature}</span>
                <input
                  className="input mt-1"
                  type="number"
                  step={0.1}
                  min={0}
                  max={2}
                  value={settings.temperature}
                  onChange={(e) =>
                    setSettings({ ...settings, temperature: Number(e.target.value) })
                  }
                />
              </label>
              <label className="block">
                <span className="field-label">最大 tokens（发送时会限制在 32768 内）</span>
                <input
                  className="input mt-1"
                  type="number"
                  min={1}
                  max={32768}
                  value={settings.max_tokens}
                  onChange={(e) =>
                    setSettings({ ...settings, max_tokens: Number(e.target.value) })
                  }
                />
              </label>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <label className="block">
                <span className="field-label">接口模式</span>
                <FilterSelect
                  className="mt-1"
                  value={settings.api_style || "chat_completions"}
                  placeholder="接口模式"
                  options={[
                    {
                      value: "chat_completions",
                      label: "Chat Completions",
                      hint: "/chat/completions",
                    },
                    {
                      value: "responses",
                      label: "Responses API",
                      hint: "/responses",
                    },
                  ]}
                  onChange={(v) => setSettings({ ...settings, api_style: v })}
                />
              </label>
              <label className="block">
                <span className="field-label">响应模式</span>
                <FilterSelect
                  className="mt-1"
                  value={settings.response_mode || "auto"}
                  placeholder="响应模式"
                  options={[
                    {
                      value: "auto",
                      label: "自动 / 文本",
                      hint: "纯文本回复",
                    },
                    {
                      value: "json_object",
                      label: "JSON Object",
                      hint: "强制结构化输出",
                    },
                  ]}
                  onChange={(v) =>
                    setSettings({ ...settings, response_mode: v })
                  }
                />
              </label>
            </div>
            <label className="flex items-center gap-2 text-[12px]">
              <input
                type="checkbox"
                checked={settings.enable_tools}
                onChange={(e) =>
                  setSettings({ ...settings, enable_tools: e.target.checked })
                }
              />
              启用数据库工具（Function Calling / Tool Use）
            </label>
            <label className="block">
              <span className="field-label">系统提示词</span>
              <textarea
                className="input mt-1"
                rows={4}
                value={settings.system_prompt}
                onChange={(e) =>
                  setSettings({ ...settings, system_prompt: e.target.value })
                }
              />
            </label>
            {testMsg && (
              <div className="text-[11px]" style={{ color: "var(--text-muted)" }}>
                {testMsg}
              </div>
            )}
            <div className="flex justify-end gap-2 pt-1">
              <button className="btn" onClick={testAi} disabled={testing}>
                {testing ? "测试中…" : "测试 API"}
              </button>
              <button className="btn btn-primary" onClick={saveAi} disabled={saving}>
                {saving ? "保存中…" : settings.id ? "保存" : "保存并新增"}
              </button>
            </div>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
