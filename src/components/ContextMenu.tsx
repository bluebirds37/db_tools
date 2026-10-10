import { useEffect, useRef } from "react";

export type ContextMenuItem =
  | {
      type: "item";
      label: string;
      danger?: boolean;
      disabled?: boolean;
      onClick: () => void;
    }
  | { type: "sep" };

export interface ContextMenuState {
  x: number;
  y: number;
  items: ContextMenuItem[];
}

export function ContextMenu({
  state,
  onClose,
}: {
  state: ContextMenuState;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", onClose);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onClose);
    };
  }, [onClose]);

  // keep menu inside viewport
  const margin = 8;
  const menuW = 180;
  const menuH = Math.min(320, 28 + state.items.length * 30);
  const x = Math.min(state.x, window.innerWidth - menuW - margin);
  const y = Math.min(state.y, window.innerHeight - menuH - margin);

  return (
    <div
      ref={ref}
      className="context-menu"
      style={{ left: x, top: y, width: menuW }}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {state.items.map((item, i) =>
        item.type === "sep" ? (
          <div key={`s${i}`} className="context-menu-sep" />
        ) : (
          <button
            key={`i${i}`}
            type="button"
            className={`context-menu-item ${item.danger ? "danger" : ""}`}
            disabled={item.disabled}
            onClick={() => {
              if (item.disabled) return;
              onClose();
              item.onClick();
            }}
          >
            {item.label}
          </button>
        )
      )}
    </div>
  );
}

export function useContextMenu() {
  const open = (
    e: React.MouseEvent,
    items: ContextMenuItem[]
  ): ContextMenuState | null => {
    e.preventDefault();
    e.stopPropagation();
    if (!items.length) return null;
    return { x: e.clientX, y: e.clientY, items };
  };
  return { open };
}
