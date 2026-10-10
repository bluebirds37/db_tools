import { ReactNode } from "react";
import { X } from "lucide-react";

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
    <div className="modal-backdrop" onClick={onClose}>
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
