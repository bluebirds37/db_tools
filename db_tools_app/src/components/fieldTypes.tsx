import type { DbKind } from "../services/api";
import { FilterSelect } from "./FilterSelect";

/// 类型候选按引擎家族给，长度/精度单独一格，避免手敲整串
export const TYPE_OPTIONS: Record<string, string[]> = {
  mysql: [
    "VARCHAR", "CHAR", "TINYINT", "SMALLINT", "INT", "BIGINT", "DECIMAL", "FLOAT",
    "DOUBLE", "TINYINT(1)", "DATE", "DATETIME", "TIMESTAMP", "TIME", "YEAR", "TEXT",
    "MEDIUMTEXT", "LONGTEXT", "JSON", "BLOB", "VARBINARY", "ENUM", "SET",
  ],
  postgres: [
    "character varying", "character", "text", "smallint", "integer", "bigint",
    "numeric", "real", "double precision", "boolean", "date", "timestamp",
    "time", "interval", "json", "jsonb", "bytea", "uuid", "inet", "cidr", "money",
  ],
  sqlite: ["INTEGER", "REAL", "TEXT", "BLOB", "NUMERIC"],
  mongodb: ["string", "int", "long", "double", "decimal", "bool", "date", "objectId", "array", "object", "null"],
  elasticsearch: [
    "keyword", "text", "constant_keyword", "wildcard", "long", "integer", "short",
    "byte", "double", "float", "half_float", "scaled_float", "date", "boolean",
    "binary", "ip", "geo_point", "nested", "object", "percolator",
  ],
};

export const typeFamily = (kind: DbKind | string) =>
  kind === "mysql"
    ? "mysql"
    : kind === "sqlite"
      ? "sqlite"
      : kind === "mongodb"
        ? "mongodb"
        : kind === "elasticsearch"
          ? "elasticsearch"
          : "postgres";

export const splitType = (t: string) => {
  const i = t.indexOf("(");
  return i < 0
    ? { base: t.trim(), args: "" }
    : { base: t.slice(0, i).trim(), args: t.slice(i + 1).replace(/\)\s*$/, "").trim() };
};

export const joinType = (base: string, args: string) =>
  args.trim() ? `${base}(${args.trim()})` : base;

export const defaultTypeOf = (kind: DbKind | string) => {
  switch (typeFamily(kind)) {
    case "mysql":
      return "VARCHAR";
    case "sqlite":
      return "TEXT";
    case "mongodb":
      return "string";
    case "elasticsearch":
      return "keyword";
    default:
      return "text";
  }
};

/// MySQL 里这些类型必须带长度，选中时先给个常用值，避免用户面对 1064
const SUGGESTED_ARGS: Record<string, string> = {
  VARCHAR: "255",
  CHAR: "1",
  BIT: "1",
  BINARY: "64",
  VARBINARY: "255",
};

export function TypePicker({
  kind,
  value,
  onChange,
}: {
  kind: DbKind | string;
  value: string;
  onChange: (v: string) => void;
}) {
  const fam = typeFamily(kind);
  const opts = TYPE_OPTIONS[fam] || [];
  const { base, args } = splitType(value);
  const freeText = fam === "mongodb" || fam === "elasticsearch";
  return (
    <div className="flex gap-1">
      <FilterSelect
        className="mono"
        style={{ width: freeText ? 158 : 118 }}
        value={base}
        title="字段类型"
        placeholder="选择类型"
        options={opts.map((o) => ({ value: o, label: o }))}
        onChange={(next) => {
          const suggested = SUGGESTED_ARGS[next.toUpperCase()] || "";
          onChange(joinType(next, freeText ? "" : args || suggested));
        }}
      />
      {!freeText && (
        <input
          className="input mono"
          style={{ width: 76 }}
          placeholder="长度"
          title="长度或精度，如 64 或 10,2"
          value={args}
          onChange={(e) => onChange(joinType(base, e.target.value))}
        />
      )}
    </div>
  );
}
