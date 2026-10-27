import { MouseEvent, ReactNode } from "react";
import { X } from "lucide-react";

/**
 * 遮罩「点空白关闭」不能只看 click：在弹窗里拖选文字、把鼠标拖到遮罩上松手时，
 * 浏览器会把 click 派发到两者的共同祖先（也就是遮罩本身），弹窗就被误关了。
 * 所以按下必须在遮罩上才算一次真正的「点空白」。同一时刻只会有一层遮罩，
 * 这个标记放模块级即可，组件里可以直接 {...backdropProps(onClose)}。
 */
let pressedOnDialog = false;

export function backdropProps(onClose: () => void) {
  return {
    onMouseDown: (e: MouseEvent<HTMLDivElement>) => {
      pressedOnDialog = e.target !== e.currentTarget;
    },
    onClick: (e: MouseEvent<HTMLDivElement>) => {
      const pressedOnBackdrop = e.target === e.currentTarget && !pressedOnDialog;
      pressedOnDialog = false;
      if (pressedOnBackdrop) onClose();
    },
  };
}

export function Modal({
  title,
  children,
  onClose,
  wide,
  footer,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
  footer?: ReactNode;
}) {
  return (
    <div className="modal-backdrop" {...backdropProps(onClose)}>
      <div
        className={`modal ${wide ? "wide" : ""}`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-3">
          <h3 className="font-semibold">{title}</h3>
          <button className="btn btn-ghost" onClick={onClose} type="button">
            <X size={16} />
          </button>
        </div>
        {children}
        {footer && <div className="flex justify-end gap-2 mt-4">{footer}</div>}
      </div>
    </div>
  );
}

export function ConfirmDialog({
  title,
  message,
  confirmText = "确认",
  danger,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  confirmText?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal
      title={title}
      onClose={onCancel}
      footer={
        <>
          <button className="btn" onClick={onCancel} type="button">
            取消
          </button>
          <button
            className={`btn ${danger ? "btn-danger" : "btn-primary"}`}
            onClick={() => {
              onConfirm();
            }}
            type="button"
          >
            {confirmText}
          </button>
        </>
      }
    >
      <p className="text-sm" style={{ color: "var(--text-muted)" }}>
        {message}
      </p>
    </Modal>
  );
}
