import { useCallback, useEffect, useState } from "react";
import {
  X,
  FolderOpen,
  RefreshCw,
  Plus,
  Trash2,
  Wand2,
  CircleCheck,
  CircleX,
} from "lucide-react";
import { save, open } from "@tauri-apps/plugin-dialog";
import {
  api,
  type ConnectionConfig,
  type MaskMode,
  type MaskRule,
  type SyncReport,
} from "../services/api";
import { FilterSelect, fsOptions } from "./FilterSelect";
import { readPref, writePref } from "../services/localPrefs";

export type TransferMode =
  | "export"
  | "dump-table"
  | "dump-db"
  | "sync"
  | "sync-db"
  | "import"
  | "run-sql";

const MASK_MODES: {
  value: MaskMode;
  label: string;
  edges?: boolean;
  text?: boolean;
}[] = [
  { value: "mask-all", label: "全部掩码 ******" },
  { value: "keep-edges", label: "保留前后几位", edges: true },
  { value: "email", label: "邮箱 a***@qq.com" },
  { value: "phone", label: "手机号 138****5678" },
  { value: "idcard", label: "身份证 前6后4" },
  { value: "bankcard", label: "银行卡 只留后4位" },
  { value: "name", label: "姓名 只留首字" },
  { value: "hash", label: "哈希改写（同值同码）" },
  { value: "replace", label: "固定替换文本", text: true },
  { value: "null", label: "置为空" },
];

// 按列名猜常见敏感字段，命中后填规则（用户可再改）
const SENSITIVE: { re: RegExp; mode: MaskMode }[] = [
  { re: /(id_?card|identity|id_?no|身份证)/, mode: "idcard" },
  { re: /(bank_?card|card_?no|银行卡|卡号)/, mode: "bankcard" },
  { re: /(mobile|phone|tel|手机|电话|联系方式)/, mode: "phone" },
  { re: /(e-?mail|邮箱|邮件)/, mode: "email" },
  { re: /^(real_?name|full_?name|name|姓名)$/i, mode: "name" },
];

const MASK_STORE_KEY = "export_masks";

const SYNC_STATUS: Record<string, string> = {
  created: "已建目标",
  copied: "已写入",
  failed: "失败",
};

function loadStoredRules(): MaskRule[] {
  try {
    const raw = readPref(MASK_STORE_KEY);
    const list = raw ? JSON.parse(raw) : [];
    return Array.isArray(list) ? (list as MaskRule[]) : [];
  } catch {
    return [];
  }
}

const maskMeta = (mode: MaskMode) => MASK_MODES.find((m) => m.value === mode);

export function TransferDialog({
  mode,
  connection,
  database,
  table,
  sql,
  connections,
  onClose,
  onMessage,
}: {
  mode: TransferMode;
  connection: ConnectionConfig;
  database: string;
  table: string | null;
  sql: string;
  connections: ConnectionConfig[];
  onClose: () => void;
  onMessage: (msg: string) => void;
}) {
  const [format, setFormat] = useState<"csv" | "excel">("csv");
  const [structure, setStructure] = useState(true);
  const [data, setData] = useState(true);
  const [maxRows, setMaxRows] = useState(10000);
  const [savePath, setSavePath] = useState("");
  const [openPath, setOpenPath] = useState("");
  const [dstConn, setDstConn] = useState(
    connections.find((c) => c.id !== connection.id)?.id || connection.id
  );
  const [dstDb, setDstDb] = useState(database);
  const [dstDbs, setDstDbs] = useState<string[]>([]);
  const [dstDbsLoading, setDstDbsLoading] = useState(false);
  const [dstDbsError, setDstDbsError] = useState("");
  const [dstTable, setDstTable] = useState(table || "");
  const [createIfMissing, setCreateIfMissing] = useState(true);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState("");
  const [syncReport, setSyncReport] = useState<SyncReport | null>(null);
  const [maskRules, setMaskRules] = useState<MaskRule[]>(() => loadStoredRules());
  const [maskCols, setMaskCols] = useState<string[]>([]);
  const [maskColsLoading, setMaskColsLoading] = useState(false);
  const [maskColsError, setMaskColsError] = useState("");

  const isSync = mode === "sync" || mode === "sync-db";

  useEffect(() => {
    if (table) setDstTable(table);
  }, [table]);

  const loadMaskColumns = useCallback(async () => {
    if (!table) {
      setMaskCols([]);
      setMaskColsError("");
      return;
    }
    setMaskColsLoading(true);
    setMaskColsError("");
    try {
      const cols = await api.listColumns(connection.id, database, table, null);
      setMaskCols(cols.map((c) => c.name));
    } catch (e) {
      setMaskCols([]);
      setMaskColsError(String(e));
    } finally {
      setMaskColsLoading(false);
    }
  }, [connection.id, database, table]);

  useEffect(() => {
    if (mode !== "export") return;
    void loadMaskColumns();
  }, [mode, loadMaskColumns]);

  useEffect(() => {
    try {
      writePref(MASK_STORE_KEY, JSON.stringify(maskRules));
    } catch {
      /* 存储被禁用时忽略 */
    }
  }, [maskRules]);

  const knownColsLower = maskCols.map((c) => c.toLowerCase());
  const isCustomCol = (col: string) =>
    !col.trim() || !knownColsLower.includes(col.trim().toLowerCase());

  const patchRule = (i: number, patch: Partial<MaskRule>) =>
    setMaskRules((list) =>
      list.map((r, idx) => (idx === i ? { ...r, ...patch } : r))
    );

  const addRule = () =>
    setMaskRules((list) => [
      ...list,
      {
        column: maskCols[0] || "",
        mode: "mask-all",
        keepHead: 3,
        keepTail: 4,
        maskText: "已脱敏",
      },
    ]);

  const removeRule = (i: number) =>
    setMaskRules((list) => list.filter((_, idx) => idx !== i));

  const detectRules = () => {
    const hits: MaskRule[] = [];
    for (const col of maskCols) {
      const rule = SENSITIVE.find((s) => s.re.test(col.toLowerCase()));
      if (!rule) continue;
      hits.push({
        column: col,
        mode: rule.mode,
        keepHead: 3,
        keepTail: 4,
        maskText: "已脱敏",
      });
    }
    if (hits.length === 0) {
      onMessage(
        maskCols.length
          ? `未在 ${maskCols.length} 个列名中识别出常见敏感列，可手动添加规则`
          : "未读取到列名，请先选择表或手动指定列名"
      );
      return;
    }
    setMaskRules((list) => {
      const kept = list.filter(
        (r) => !hits.some((h) => h.column.toLowerCase() === r.column.toLowerCase())
      );
      return [...kept, ...hits];
    });
    onMessage(`已识别 ${hits.length} 个敏感列：${hits.map((h) => h.column).join("、")}`);
  };

  const loadDstDatabases = useCallback(
    async (connId: string) => {
      setDstDbsLoading(true);
      setDstDbsError("");
      try {
        const list = await api.listDatabases(connId);
        setDstDbs(list);
        setDstDb((cur) =>
          cur && list.includes(cur)
            ? cur
            : list.includes(database)
              ? database
              : list[0] || cur
        );
      } catch (e) {
        setDstDbs([]);
        setDstDbsError(String(e));
      } finally {
        setDstDbsLoading(false);
      }
    },
    [database]
  );

  useEffect(() => {
    if (!isSync) return;
    void loadDstDatabases(dstConn);
  }, [isSync, dstConn, loadDstDatabases]);

  const defaultName = () => {
    if (mode === "export") {
      return `${database || "export"}_${table || "query"}.${format === "excel" ? "xls" : "csv"}`;
    }
    if (mode === "dump-table") return `${database}_${table || "table"}.sql`;
    // 备份带时间戳，避免反复覆盖同一个文件
    const stamp = new Date().toISOString().slice(0, 19).replace(/[T:]/g, "-");
    return `${database || "database"}_backup_${stamp}.sql`;
  };

  const pickSave = async () => {
    try {
      const filters =
        mode === "export"
          ? format === "excel"
            ? [{ name: "Excel", extensions: ["xls", "xml"] }]
            : [{ name: "CSV", extensions: ["csv"] }]
          : [{ name: "SQL", extensions: ["sql"] }];
      const path = await save({
        defaultPath: defaultName(),
        filters,
        title: mode === "export" ? "导出数据" : "保存 SQL 转储",
      });
      if (path) setSavePath(path);
    } catch (e) {
      onMessage(String(e));
    }
  };

  const pickOpen = async () => {
    try {
      const filters =
        mode === "import"
          ? [
              { name: "表格数据", extensions: ["csv", "xls", "xml"] },
              { name: "CSV", extensions: ["csv"] },
              { name: "Excel", extensions: ["xls", "xml"] },
            ]
          : [{ name: "SQL", extensions: ["sql", "txt"] }];
      const path = await open({
        multiple: false,
        filters,
        title: mode === "import" ? "选择导入文件" : "选择 SQL 脚本",
      });
      if (typeof path === "string") setOpenPath(path);
    } catch (e) {
      onMessage(String(e));
    }
  };

  const run = async () => {
    setBusy(true);
    setPreview("");
    setSyncReport(null);
    try {
      if (isSync && !dstDb.trim()) throw new Error("请先选择目标库");
      if (mode === "export") {
        const query =
          sql.trim() ||
          (connection.kind === "mysql"
            ? `SELECT * FROM \`${database}\`.\`${table}\``
            : connection.kind === "postgres" ||
                connection.kind === "pgvector" ||
                connection.kind === "sqlite"
              ? `SELECT * FROM "${table}"`
              : `${table || "*"}`);
        const active = maskRules.filter(
          (r) => r.column.trim() && r.mode !== "none"
        );
        const res = await api.exportQuery(
          connection.id,
          database,
          query,
          format,
          maxRows,
          active
        );
        let path = savePath.trim();
        if (!path) {
          const picked = await save({
            defaultPath: defaultName(),
            filters:
              format === "excel"
                ? [{ name: "Excel", extensions: ["xls"] }]
                : [{ name: "CSV", extensions: ["csv"] }],
          });
          if (!picked) {
            setBusy(false);
            return;
          }
          path = picked;
          setSavePath(picked);
        }
        await api.writeFile(path, res.content);
        const info = [`${res.rows} 行`, `${res.content.length} 字节`];
        if (res.maskedColumns.length)
          info.push(`脱敏列 ${res.maskedColumns.join("、")}`);
        let msg = `已导出到 ${path}（${info.join("，")}）`;
        if (res.unmatchedColumns.length)
          msg += `；这些列在结果集中不存在，未生效：${res.unmatchedColumns.join("、")}`;
        onMessage(msg);
      } else if (mode === "dump-table") {
        if (!table) throw new Error("未选择表");
        const content = await api.dumpTable(
          connection.id,
          database,
          table,
          structure,
          data,
          maxRows
        );
        let path = savePath.trim();
        if (!path) {
          const picked = await save({
            defaultPath: defaultName(),
            filters: [{ name: "SQL", extensions: ["sql"] }],
          });
          if (!picked) {
            setBusy(false);
            return;
          }
          path = picked;
          setSavePath(picked);
        }
        await api.writeFile(path, content);
        setPreview(content.slice(0, 1500));
        onMessage(`已转储表 ${table} → ${path}`);
      } else if (mode === "dump-db") {
        const content = await api.dumpDatabase(
          connection.id,
          database,
          structure,
          data,
          maxRows
        );
        let path = savePath.trim();
        if (!path) {
          const picked = await save({
            defaultPath: defaultName(),
            filters: [{ name: "SQL", extensions: ["sql"] }],
          });
          if (!picked) {
            setBusy(false);
            return;
          }
          path = picked;
          setSavePath(picked);
        }
        await api.writeFile(path, content);
        setPreview(content.slice(0, 1500));
        onMessage(`已转储库 ${database} → ${path}`);
      } else if (mode === "sync") {
        const rep = await api.syncTable({
          sourceConnectionId: connection.id,
          sourceDatabase: database,
          sourceTable: table || "",
          targetConnectionId: dstConn,
          targetDatabase: dstDb,
          targetTable: dstTable || table || "",
          createIfMissing,
          maxRows,
        });
        setSyncReport(rep);
        onMessage(rep.message);
      } else if (mode === "sync-db") {
        const rep = await api.syncDatabase({
          sourceConnectionId: connection.id,
          sourceDatabase: database,
          targetConnectionId: dstConn,
          targetDatabase: dstDb,
          createIfMissing,
          maxRows,
        });
        setSyncReport(rep);
        onMessage(rep.message);
      } else if (mode === "import") {
        let path = openPath.trim();
        if (!path) {
          path = (await open({
            multiple: false,
            filters: [
              { name: "表格数据", extensions: ["csv", "xls", "xml"] },
            ],
          })) as string;
          if (!path) {
            setBusy(false);
            return;
          }
          setOpenPath(path);
        }
        const importFmt = path.toLowerCase().endsWith(".csv") ? "csv" : "excel";
        const targetTable = dstTable || table || "";
        if (!targetTable) throw new Error("请填写目标表名");
        const msg = await api.importTabularFile({
          connectionId: connection.id,
          database,
          table: targetTable,
          path,
          format: importFmt,
          createIfMissing,
        });
        onMessage(msg);
      } else if (mode === "run-sql") {
        let path = openPath.trim();
        if (!path) {
          path = (await open({
            multiple: false,
            filters: [{ name: "SQL", extensions: ["sql", "txt"] }],
          })) as string;
          if (!path) {
            setBusy(false);
            return;
          }
          setOpenPath(path);
        }
        const msg = await api.runSqlScript({
          connectionId: connection.id,
          database,
          path,
        });
        onMessage(msg);
      }
      // 同步留着弹窗看逐表结果，其他动作完成即关
      if (!isSync) onClose();
    } catch (e) {
      onMessage(String(e));
    } finally {
      setBusy(false);
    }
  };

  const title =
    mode === "export"
      ? "导出数据"
      : mode === "dump-table"
        ? "转储表为 SQL"
        : mode === "dump-db"
          ? "转储数据库为 SQL"
          : mode === "import"
            ? "导入 CSV / Excel"
            : mode === "run-sql"
              ? "运行 SQL 脚本"
              : mode === "sync-db"
                ? "同步整库到其他连接"
                : "同步表到其他连接";

  const needSavePath =
    mode === "export" || mode === "dump-table" || mode === "dump-db";
  const needOpenPath = mode === "import" || mode === "run-sql";

  const dstConnField = (
    <label className="block">
      <span className="field-label">目标连接</span>
      <FilterSelect
        className="mt-1"
        value={dstConn}
        placeholder="选择目标连接"
        options={connections.map((c) => ({
          value: c.id,
          label: c.name,
          hint: c.kind,
        }))}
        onChange={setDstConn}
      />
    </label>
  );

  const dstDbField = (
    <div className="min-w-0">
      <span className="field-label">目标库</span>
      <div className="flex gap-2 mt-1">
        <FilterSelect
          className="flex-1 min-w-0"
          value={dstDb}
          disabled={dstDbsLoading}
          placeholder={
            dstDbsLoading ? "读取目标库…" : dstDb || "无可选库"
          }
          emptyText={
            dstDbsError ? "读取失败，点右侧重试" : "该连接暂无可选库"
          }
          options={fsOptions(dstDbs)}
          onChange={setDstDb}
        />
        <button
          className="btn"
          title="重新读取目标库列表"
          onClick={() => void loadDstDatabases(dstConn)}
        >
          <RefreshCw
            size={13}
            className={dstDbsLoading ? "animate-spin" : ""}
          />
        </button>
      </div>
      {dstDbsError && (
        <div
          className="text-[11px] mt-1 break-all"
          style={{ color: "var(--danger)" }}
        >
          {dstDbsError}
        </div>
      )}
    </div>
  );

  const dstKind = connections.find((c) => c.id === dstConn)?.kind;
  const redisHint =
    dstKind === "redis" ? (
      <div className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
        Redis 没有表：每行写成一个 string 键{' '}
        <span className="mono">
          {mode === "sync-db" ? "<表名>:<主键>" : `${dstTable || "表名"}:<主键>`}
        </span>
        ，值为整行 JSON。
      </div>
    ) : null;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-4">
          <h3 className="font-semibold">{title}</h3>
          <button className="btn btn-ghost" onClick={onClose}>
            <X size={15} />
          </button>
        </div>

        <div className="text-[11px] mb-3" style={{ color: "var(--text-subtle)" }}>
          源：{connection.name} / {database || "-"}
          {table ? ` / ${table}` : ""}
        </div>

        <div className="space-y-3">
          {needSavePath && (
            <div>
              <span className="field-label">保存到</span>
              <div className="flex gap-2 mt-1">
                <input
                  className="input mono flex-1"
                  placeholder="点击右侧选择文件…"
                  value={savePath}
                  onChange={(e) => setSavePath(e.target.value)}
                />
                <button className="btn" onClick={pickSave} title="选择保存路径">
                  <FolderOpen size={14} /> 选择
                </button>
              </div>
            </div>
          )}

          {needOpenPath && (
            <div>
              <span className="field-label">
                {mode === "import" ? "导入文件" : "SQL 脚本文件"}
              </span>
              <div className="flex gap-2 mt-1">
                <input
                  className="input mono flex-1"
                  placeholder="点击右侧选择文件…"
                  value={openPath}
                  onChange={(e) => setOpenPath(e.target.value)}
                />
                <button className="btn" onClick={pickOpen} title="选择文件">
                  <FolderOpen size={14} /> 选择
                </button>
              </div>
              {mode === "import" && (
                <div className="text-[11px] mt-1" style={{ color: "var(--text-subtle)" }}>
                  支持 CSV（首行为表头）或本工具导出的 Excel (.xls)
                </div>
              )}
            </div>
          )}

          {mode === "export" && (
            <div>
              <span className="field-label">格式</span>
              <div className="flex gap-2 mt-1">
                <button
                  className={`btn ${format === "csv" ? "btn-primary" : ""}`}
                  onClick={() => setFormat("csv")}
                >
                  CSV
                </button>
                <button
                  className={`btn ${format === "excel" ? "btn-primary" : ""}`}
                  onClick={() => setFormat("excel")}
                >
                  Excel
                </button>
              </div>
            </div>
          )}

          {mode === "export" && (
            <div className="mask-box">
              <div className="mask-head">
                <span className="field-label" style={{ margin: 0 }}>
                  脱敏规则（CSV / Excel 通用）
                </span>
                <div className="flex gap-1 items-center">
                  <button
                    className="btn"
                    onClick={() => void loadMaskColumns()}
                    disabled={maskColsLoading || !table}
                    title={table ? "重新读取列名" : "未选中表，无法读取列名"}
                  >
                    <RefreshCw
                      size={12}
                      className={maskColsLoading ? "animate-spin" : ""}
                    />
                  </button>
                  <button
                    className="btn"
                    onClick={detectRules}
                    disabled={maskColsLoading || maskCols.length === 0}
                    title="按列名识别手机号 / 邮箱 / 身份证等并生成规则"
                  >
                    <Wand2 size={12} /> 识别敏感列
                  </button>
                  <button className="btn" onClick={addRule}>
                    <Plus size={12} /> 添加规则
                  </button>
                </div>
              </div>

              {maskColsError && (
                <div className="mask-tip" style={{ color: "var(--danger)" }}>
                  列名读取失败：{maskColsError}（可手动输入列名）
                </div>
              )}

              {maskRules.length === 0 ? (
                <div className="mask-tip">
                  未配置规则，将导出原始值。选中表后可一键识别常见敏感列。
                </div>
              ) : (
                <div className="space-y-1.5">
                  {maskRules.map((r, i) => {
                    const meta = maskMeta(r.mode);
                    const custom = isCustomCol(r.column);
                    return (
                      <div key={i} className="mask-rule">
                        <FilterSelect
                          className="mask-col"
                          value={custom ? "__custom__" : r.column}
                          placeholder="选择列"
                          options={[
                            {
                              value: "__custom__",
                              label: maskColsLoading
                                ? "读取列名…"
                                : maskCols.length === 0
                                  ? "手动输入列名"
                                  : "手动输入列名…",
                              hint: "自己填写",
                            },
                            ...fsOptions(maskCols),
                          ]}
                          onChange={(v) =>
                            patchRule(
                              i,
                              v === "__custom__"
                                ? { column: custom ? r.column : "" }
                                : { column: v }
                            )
                          }
                        />
                        <FilterSelect
                          className="mask-mode"
                          value={r.mode}
                          placeholder="脱敏方式"
                          options={MASK_MODES.map((m) => ({
                            value: m.value,
                            label: m.label,
                          }))}
                          onChange={(v) =>
                            patchRule(i, { mode: v as MaskMode })
                          }
                        />
                        {meta?.edges && (
                          <div className="flex items-center gap-1">
                            <span className="mask-tip">留前</span>
                            <input
                              className="input mask-num"
                              type="number"
                              min={0}
                              max={32}
                              value={r.keepHead ?? 0}
                              onChange={(e) =>
                                patchRule(i, {
                                  keepHead: Math.max(
                                    0,
                                    Math.min(32, Number(e.target.value) || 0)
                                  ),
                                })
                              }
                            />
                            <span className="mask-tip">留后</span>
                            <input
                              className="input mask-num"
                              type="number"
                              min={0}
                              max={32}
                              value={r.keepTail ?? 0}
                              onChange={(e) =>
                                patchRule(i, {
                                  keepTail: Math.max(
                                    0,
                                    Math.min(32, Number(e.target.value) || 0)
                                  ),
                                })
                              }
                            />
                          </div>
                        )}
                        {meta?.text && (
                          <input
                            className="input mask-text"
                            placeholder="替换文本"
                            value={r.maskText ?? ""}
                            onChange={(e) =>
                              patchRule(i, { maskText: e.target.value })
                            }
                          />
                        )}
                        <button
                          className="btn btn-ghost"
                          title="删除该规则"
                          onClick={() => removeRule(i)}
                        >
                          <Trash2 size={12} />
                        </button>
                        {custom && (
                          <input
                            className="input mono mask-full"
                            placeholder="列名，如 phone / user.mobile"
                            value={r.column}
                            onChange={(e) =>
                              patchRule(i, { column: e.target.value })
                            }
                          />
                        )}
                      </div>
                    );
                  })}
                </div>
              )}

              <div className="mask-tip">
                列名不区分大小写，支持 <span className="mono">表.列</span> 写法；命中列的每个值都会先掩码再写入文件，未命中的列会在执行后提示。
              </div>
            </div>
          )}

          {(mode === "dump-table" || mode === "dump-db") && (
            <div className="space-y-2">
              <span className="field-label">转储内容</span>
              <label className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={structure}
                  onChange={(e) => setStructure(e.target.checked)}
                />
                表结构
              </label>
              <label className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={data}
                  onChange={(e) => setData(e.target.checked)}
                />
                数据
              </label>
            </div>
          )}

          {mode === "import" && (
            <>
              <label className="block">
                <span className="field-label">目标表名</span>
                <input
                  className="input mt-1 mono"
                  value={dstTable}
                  onChange={(e) => setDstTable(e.target.value)}
                  placeholder={table || "表名"}
                />
              </label>
              <label className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={createIfMissing}
                  onChange={(e) => setCreateIfMissing(e.target.checked)}
                />
                表不存在时自动创建（SQL 数据源）
              </label>
            </>
          )}

          {mode === "sync" && (
            <>
              {dstConnField}
              <div className="grid grid-cols-2 gap-3">
                <div>{dstDbField}</div>
                <label className="block min-w-0">
                  <span className="field-label">目标表</span>
                  <input
                    className="input mt-1"
                    value={dstTable}
                    onChange={(e) => setDstTable(e.target.value)}
                    placeholder={table || "表名"}
                  />
                </label>
              </div>
              {redisHint}
              <label className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={createIfMissing}
                  onChange={(e) => setCreateIfMissing(e.target.checked)}
                />
                目标表不存在时自动创建
              </label>
            </>
          )}

          {mode === "sync-db" && (
            <>
              {dstConnField}
              <div>{dstDbField}</div>
              {redisHint}
              <label className="flex items-center gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={createIfMissing}
                  onChange={(e) => setCreateIfMissing(e.target.checked)}
                />
                目标表不存在时自动创建
              </label>
              <div className="text-[11px]" style={{ color: "var(--text-subtle)" }}>
                将同步源库中全部基础表（跳过视图），表名同名落到目标库；每张表
                {maxRows <= 0 ? "同步全部行" : `最多 ${maxRows} 行`}。
              </div>
            </>
          )}

          {(mode === "export" ||
            mode === "dump-table" ||
            mode === "dump-db" ||
            mode === "sync" ||
            mode === "sync-db") && (
            <div className="block">
              <span className="field-label">
                最大行数
                {maxRows <= 0 && (
                  <span style={{ color: "var(--accent-strong)" }}>
                    {" "}
                    · 当前=全部行
                  </span>
                )}
              </span>
              <div className="flex gap-2">
                <input
                  className="input"
                  type="number"
                  min={-1}
                  step={1}
                  value={maxRows}
                  title="填 0 或 -1 表示不限制，导出/备份/同步全部行"
                  onChange={(e) => {
                    const n = Math.trunc(Number(e.target.value));
                    setMaxRows(Number.isFinite(n) ? n : 0);
                  }}
                />
                <button
                  className={`btn ${maxRows <= 0 ? "btn-primary" : ""}`}
                  title="不限制行数，处理全部行"
                  onClick={() => setMaxRows(-1)}
                >
                  全部行
                </button>
              </div>
              <div className="text-[11px] mt-1" style={{ color: "var(--text-subtle)" }}>
                0 或 -1 = 不限制（全部行）；整库操作时这是每张表的上限。
              </div>
            </div>
          )}

          {syncReport && (
            <div className="mg-report animate-fade-in">
              <div className="flex items-center gap-2 text-xs mb-1 flex-wrap">
                <span className="tag tag-accent">
                  共 {syncReport.tables.length} 张表
                </span>
                <span className="tag">已建目标 {syncReport.created}</span>
                <span className="tag">写入 {syncReport.copied}</span>
                <span
                  className="tag"
                  style={{
                    color: syncReport.failed ? "var(--danger)" : undefined,
                  }}
                >
                  失败 {syncReport.failed}
                </span>
                <span className="tag">{syncReport.totalRows} 行</span>
                {syncReport.failedRows > 0 && (
                  <span className="tag" style={{ color: "var(--danger)" }}>
                    {syncReport.failedRows} 行未写入
                  </span>
                )}
              </div>
              <table className="data-table">
                <thead>
                  <tr>
                    <th style={{ width: 96 }}>状态</th>
                    <th>目标表 / 键</th>
                    <th style={{ width: 72 }}>行数</th>
                    <th>说明</th>
                  </tr>
                </thead>
                <tbody>
                  {syncReport.tables.map((t, i) => (
                    <tr key={`${t.table}-${i}`}>
                      <td>
                        <span className="flex items-center gap-1.5 text-xs">
                          {t.status === "failed" ? (
                            <CircleX
                              size={12}
                              style={{ color: "var(--danger)" }}
                            />
                          ) : (
                            <CircleCheck
                              size={12}
                              style={{ color: "var(--ok)" }}
                            />
                          )}
                          {SYNC_STATUS[t.status] || t.status}
                        </span>
                      </td>
                      <td className="mono">{t.table}</td>
                      <td className="mono">
                        {t.rows}
                        {t.failedRows > 0 && (
                          <span style={{ color: "var(--danger)" }}>
                            {" "}
                            -{t.failedRows}
                          </span>
                        )}
                      </td>
                      <td className="mg-detail" title={t.detail}>
                        {t.detail}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {preview && (
            <pre
              className="text-[10px] mono"
              style={{
                maxHeight: 120,
                overflow: "auto",
                background: "var(--bg-elevated)",
                padding: 8,
                borderRadius: 8,
                border: "1px solid var(--border)",
              }}
            >
              {preview}…
            </pre>
          )}

          {busy && (
            <div className="progress" role="progressbar" aria-label="处理中">
              <span />
            </div>
          )}

          <div className="flex justify-end gap-2 pt-1">
            <button className="btn" onClick={onClose}>
              {isSync && syncReport ? "关闭" : "取消"}
            </button>
            <button className="btn btn-primary" onClick={run} disabled={busy}>
              {busy ? "处理中…" : "执行"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
