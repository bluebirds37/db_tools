import { create } from "zustand";
import { api, type ConnectionConfig } from "../services/api";
import { reportUiError } from "../services/logger";
import { dropPref, readPref, writePref } from "../services/localPrefs";

interface AppState {
  theme: "light" | "dark";
  fontSize: number;
  fontFamily: string;
  connections: ConnectionConfig[];
  activeConnectionId: string | null;
  activeDatabase: string;
  activeTable: string | null;
  aiOpen: boolean;
  loading: boolean;
  error: string | null;
  setTheme: (t: "light" | "dark") => void;
  toggleTheme: () => void;
  setFontSize: (n: number) => void;
  setFontFamily: (f: string) => void;
  loadConnections: () => Promise<void>;
  saveConnection: (c: ConnectionConfig) => Promise<void>;
  deleteConnection: (id: string) => Promise<void>;
  setActiveConnection: (id: string | null) => void;
  setActiveDatabase: (db: string) => void;
  setActiveTable: (t: string | null) => void;
  setAiOpen: (v: boolean) => void;
  setError: (e: string | null) => void;
}

const getInitialTheme = (): "light" | "dark" => {
  const saved = readPref("theme");
  if (saved === "light" || saved === "dark") return saved;
  return "dark";
};

export const UI_FONT_FALLBACK =
  '"Inter", "Segoe UI", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif';

const getNum = (key: string, fallback: number) => {
  const v = readPref(key);
  const n = v ? Number(v) : NaN;
  return Number.isFinite(n) && n > 0 ? n : fallback;
};

const getInitialFontSize = () => {
  const saved = readPref("font_size");
  if (saved) {
    const n = Number(saved);
    if (Number.isFinite(n) && n >= 11 && n <= 24) return n;
  }
  // 旧版分开的界面/代码字号，现在只留一个
  const legacy = getNum("ui_font", 0);
  dropPref("ui_font");
  dropPref("code_font");
  return legacy > 0 ? legacy : 13;
};

/// 字体名来自用户输入，去掉能破坏 CSS 声明的字符
const sanitizeFamily = (raw: string) =>
  raw.replace(/["'`;\\(){}]/g, "").replace(/[\r\n]+/g, " ").trim().slice(0, 64);

/// 字号/字体只有一个来源：界面和代码都读这两个变量
export function applyFontVars(size: number, family: string) {
  const style = document.documentElement.style;
  style.setProperty("--app-font-size", `${size}px`);
  const clean = sanitizeFamily(family);
  style.setProperty(
    "--app-font",
    clean ? `"${clean}", ${UI_FONT_FALLBACK}` : UI_FONT_FALLBACK
  );
}

export const useAppStore = create<AppState>((set, get) => ({
  theme: getInitialTheme(),
  fontSize: getInitialFontSize(),
  fontFamily: readPref("font_family") || "",
  connections: [],
  activeConnectionId: null,
  activeDatabase: "",
  activeTable: null,
  aiOpen: true,
  loading: false,
  error: null,
  setTheme: (t) => {
    writePref("theme", t);
    const apply = () => {
      document.documentElement.classList.toggle("dark", t === "dark");
      set({ theme: t });
    };
    const doc = document as Document & {
      startViewTransition?: (cb: () => void) => void;
    };
    // 主题切换用一次快照交叉淡化，不支持时直接改类名
    if (doc.startViewTransition) doc.startViewTransition(apply);
    else apply();
  },
  toggleTheme: () => {
    const next = get().theme === "dark" ? "light" : "dark";
    get().setTheme(next);
  },
  setFontSize: (n) => {
    const size = Math.trunc(Number(n));
    const safe = Number.isFinite(size) ? Math.min(24, Math.max(11, size)) : 13;
    writePref("font_size", String(safe));
    applyFontVars(safe, get().fontFamily);
    set({ fontSize: safe });
  },
  setFontFamily: (f) => {
    const clean = sanitizeFamily(f);
    writePref("font_family", clean);
    applyFontVars(get().fontSize, clean);
    set({ fontFamily: clean });
  },
  loadConnections: async () => {
    try {
      const connections = await api.listConnections();
      set({ connections });
    } catch (e) {
      set({ error: String(e) });
    }
  },
  saveConnection: async (c) => {
    await api.saveConnection(c);
    await get().loadConnections();
  },
  deleteConnection: async (id) => {
    await api.deleteConnection(id);
    const s = get();
    if (s.activeConnectionId === id) {
      set({ activeConnectionId: null, activeDatabase: "", activeTable: null });
    }
    await s.loadConnections();
  },
  setActiveConnection: (id) =>
    set({ activeConnectionId: id, activeDatabase: "", activeTable: null }),
  setActiveDatabase: (db) => set({ activeDatabase: db, activeTable: null }),
  setActiveTable: (t) => set({ activeTable: t }),
  setAiOpen: (v) => set({ aiOpen: v }),
  setError: (e) => {
    if (e) void reportUiError("appStore", e);
    set({ error: e });
  },
}));

// 首屏就按保存好的字号/字体渲染，不等 React 挂载后再改
applyFontVars(
  useAppStore.getState().fontSize,
  useAppStore.getState().fontFamily
);
