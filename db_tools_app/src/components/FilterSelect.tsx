import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as RKeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, Search, X } from "lucide-react";

export interface FsOption {
  value: string;
  label: string;
  /** 次要说明，参与筛选匹配 */
  hint?: string;
}

/** 子串优先，其次子序列：输入 uid 也能命中 user_id */
function fsMatch(opt: FsOption, q: string) {
  if (!q) return true;
  const hay = `${opt.label} ${opt.value} ${opt.hint ?? ""}`.toLowerCase();
  if (hay.includes(q)) return true;
  let i = 0;
  for (const ch of hay) {
    if (i < q.length && ch === q[i]) i++;
  }
  return i === q.length;
}

type Pos = { left: number; width: number; top?: number; bottom?: number };

export function FilterSelect({
  value,
  onChange,
  options,
  placeholder = "请选择",
  disabled = false,
  className = "",
  style,
  title,
  emptyText = "无可选项",
}: {
  value: string;
  onChange: (v: string) => void;
  options: FsOption[];
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  style?: CSSProperties;
  title?: string;
  emptyText?: string;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<Pos | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // 当前值不在列表里也要显示出来（外部数据未刷新、历史选择等）
  const opts = useMemo(() => {
    if (value && !options.some((o) => o.value === value)) {
      return [{ value, label: value }, ...options];
    }
    return options;
  }, [options, value]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? opts.filter((o) => fsMatch(o, q)) : opts;
  }, [opts, query]);

  const current = opts.find((o) => o.value === value);

  const place = useCallback(() => {
    const el = triggerRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const width = Math.min(Math.max(r.width, 190), window.innerWidth - 16);
    const left = Math.min(Math.max(8, r.left), window.innerWidth - width - 8);
    const expectH = 46 + Math.min(shown.length || 1, 10) * 28;
    const below = window.innerHeight - r.bottom;
    const flip = below < expectH + 10 && r.top > below;
    setPos({
      left,
      width,
      ...(flip
        ? { bottom: window.innerHeight - r.top + 5 }
        : { top: r.bottom + 5 }),
    });
  }, [shown.length]);

  const close = useCallback(() => {
    setOpen(false);
    setQuery("");
    setActive(0);
    setPos(null);
  }, []);

  useLayoutEffect(() => {
    if (open) place();
  }, [open, place]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (panelRef.current?.contains(t) || triggerRef.current?.contains(t)) return;
      close();
    };
    const reposition = () => place();
    document.addEventListener("mousedown", onDown, true);
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  }, [open, close, place]);

  useEffect(() => {
    if (!open) return;
    const node = listRef.current?.children[active] as HTMLElement | undefined;
    node?.scrollIntoView({ block: "nearest" });
  }, [active, open]);

  const pick = (v: string) => {
    onChange(v);
    close();
    triggerRef.current?.focus();
  };

  const onKeys = (e: RKeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(shown.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
    } else if (e.key === "Home") {
      e.preventDefault();
      setActive(0);
    } else if (e.key === "End") {
      e.preventDefault();
      setActive(Math.max(0, shown.length - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const hit = shown[active] ?? shown[0];
      if (hit) pick(hit.value);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
      triggerRef.current?.focus();
    } else if (e.key === "Tab") {
      close();
    }
  };

  return (
    <>
      <button
        type="button"
        ref={triggerRef}
        className={`fs-trigger input ${className}`.trim()}
        style={style}
        title={title}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => (open ? close() : setOpen(true))}
        onKeyDown={(e) => {
          if (open) return;
          if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        <span className={`fs-text${current ? "" : " fs-muted"}`}>
          {current ? current.label : placeholder}
        </span>
        <ChevronDown size={13} className={`fs-arrow${open ? " is-open" : ""}`} />
      </button>

      {open &&
        pos &&
        createPortal(
          <div
            className="fs-pop"
            ref={panelRef}
            role="listbox"
            aria-label={title || placeholder}
            style={{
              left: pos.left,
              width: pos.width,
              top: pos.top,
              bottom: pos.bottom,
            }}
            onKeyDown={onKeys}
          >
            <div className="fs-search">
              <Search size={12} className="fs-search-icon" />
              <input
                ref={inputRef}
                autoFocus
                className="fs-search-input"
                value={query}
                placeholder="输入筛选…"
                onChange={(e) => {
                  setQuery(e.target.value);
                  setActive(0);
                }}
              />
              {query && (
                <button
                  type="button"
                  className="fs-clear"
                  title="清空筛选"
                  onClick={() => {
                    setQuery("");
                    setActive(0);
                    inputRef.current?.focus();
                  }}
                >
                  <X size={11} />
                </button>
              )}
            </div>
            <div className="fs-list" ref={listRef}>
              {shown.length === 0 ? (
                <div className="fs-empty">
                  {query ? `没有匹配「${query}」的选项` : emptyText}
                </div>
              ) : (
                shown.map((o, i) => (
                  <button
                    type="button"
                    key={`${o.value}-${i}`}
                    role="option"
                    aria-selected={o.value === value}
                    className={`fs-opt${i === active ? " is-active" : ""}${
                      o.value === value ? " is-sel" : ""
                    }`}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => pick(o.value)}
                  >
                    <span className="fs-opt-label">{o.label}</span>
                    {o.hint && <span className="fs-opt-hint">{o.hint}</span>}
                    {o.value === value && (
                      <Check size={12} className="fs-opt-check" />
                    )}
                  </button>
                ))
              )}
            </div>
            <div className="fs-foot">
              {shown.length} / {opts.length} 项 · ↑↓ 选择，Enter 确认
            </div>
          </div>,
          document.body
        )}
    </>
  );
}

/** 方便直接传字符串数组的调用点 */
export const fsOptions = (list: (string | null | undefined)[]): FsOption[] =>
  list
    .filter((s): s is string => typeof s === "string" && s.length > 0)
    .map((s) => ({ value: s, label: s }));
