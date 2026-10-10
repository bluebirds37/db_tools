import { useMemo, useState } from "react";
import { X, Check, Loader2, Wand2 } from "lucide-react";
import {
  api,
  type ColumnInfo,
  type ConnectionConfig,
} from "../services/api";

/// 按字段类型给一个可用的样例值
function sample(type: string): unknown {
  const t = (type || "").toLowerCase();
  if (t.includes("bool")) return false;
  if (t.includes("int") || t.includes("long") || t.includes("double") || t.includes("float") || t.includes("numeric") || t === "number") return 0;
  if (t.includes("date") || t.includes("time")) return new Date().toISOString();
  if (t.includes("object") || t.includes("json") || t.includes("nested") || t.includes("array")) return {};
  if (t.includes("oid")) return "000000000000000000000000";
  if (t.includes("ip")) return "127.0.0.1";
  return "";
}

export function DocumentEditor({
  connection,
  database,
  collection,
  columns,
  onClose,
  onSaved,
}: {
  connection: ConnectionConfig;
  database: string;
  collection: string;
  columns: ColumnInfo[];
  onClose: () => void;
  onSaved: (msg: string) => void;
}) {
  const seed = useMemo(() => {
    const base: Record<string, unknown> = {};
    columns
      .filter((c) => c.name !== "_score")
      .slice(0, 16)
      .forEach((c) => {
        if (c.name === "_id") return;
        base[c.name] = sample(c.data_type);
      });
    return JSON.stringify(base, null, 2);
  }, [columns]);

  const [text, setText] = useState(seed);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const validate = (): Record<string, unknown> | null => {
    let v: unknown;
    try {
      v = JSON.parse(text);
    } catch (e) {
      setErr(`JSON 解析失败：${String(e)}`);
      return null;
    }
    if (v === null || typeof v !== "object" || Array.isArray(v)) {
      setErr("顶层必须是一个 JSON 对象");
      return null;
    }
    setErr(null);
    return v as Record<string, unknown>;
  };

  const submit = async () => {
    const obj = validate();
    if (!obj) return;
    setBusy(true);
    try {
      const res = await api.insertRow(connection.id, database, collection, obj);
      onSaved(res.message || "已写入 1 条");
      onClose();
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    }
  };

  const format = () => {
    const obj = validate();
    if (obj) setText(JSON.stringify(obj, null, 2));
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-[15px] font-bold tracking-tight">
            添加文档
            <span className="conn-step">
              {database || "-"} · {collection}
            </span>
          </h2>
          <button className="btn btn-ghost btn-icon" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <span className="field-label">JSON 文档</span>
        <textarea
          className="sql-editor"
          rows={14}
          spellCheck={false}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />

        <div className="flex items-center gap-2 mt-2">
          <button className="btn" onClick={format} disabled={busy}>
            <Wand2 size={13} /> 校验并格式化
          </button>
          <button className="btn" onClick={() => setText(seed)} disabled={busy}>
            按字段重置模板
          </button>
          <span className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
            字段模板来自{connection.kind === "elasticsearch" ? "索引 mapping" : "文档采样"}
          </span>
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
          className="flex items-center justify-end gap-2 pt-3 mt-3 border-t"
          style={{ borderColor: "var(--border)" }}
        >
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button className="btn btn-primary" disabled={busy} onClick={submit}>
            {busy ? <Loader2 size={13} className="animate-spin" /> : <Check size={13} />}
            写入
          </button>
        </div>
      </div>
    </div>
  );
}
