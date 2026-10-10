import { invoke } from "@tauri-apps/api/core";

export type DbKind =
  | "mysql"
  | "postgres"
  | "sqlite"
  | "redis"
  | "mongodb"
  | "elasticsearch"
  | "qdrant"
  | "pgvector";

export interface ConnectionConfig {
  id: string;
  name: string;
  kind: DbKind;
  host?: string | null;
  port?: number | null;
  username?: string | null;
  password?: string | null;
  database?: string | null;
  options: Record<string, string>;
  color?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
}

export interface TableInfo {
  name: string;
  schema?: string | null;
  table_type?: string | null;
  comment?: string | null;
  row_estimate?: number | null;
  tablespace?: string | null;
}

export interface ColumnInfo {
  name: string;
  data_type: string;
  nullable: boolean;
  primary_key: boolean;
  default?: string | null;
  comment?: string | null;
  extra?: string | null;
  max_length?: number | null;
}

export interface IndexInfo {
  name: string;
  columns: string[];
  unique: boolean;
  primary: boolean;
}

export interface ForeignKeyInfo {
  name: string;
  column: string;
  ref_table: string;
  ref_column: string;
}

export interface TablespaceInfo {
  name: string;
  owner?: string | null;
  location?: string | null;
  size?: string | null;
  comment?: string | null;
}

export interface TableExtra {
  schema?: string | null;
  tablespace?: string | null;
  size?: string | null;
}

export interface QueryPage {
  columns: string[];
  rows: unknown[][];
  total: number;
  page: number;
  page_size: number;
  elapsed_ms: number;
  message?: string | null;
}

export interface ExecResult {
  affected_rows: number;
  last_insert_id?: number | null;
  message?: string | null;
}

export interface AiSettings {
  id?: string;
  name?: string;
  is_default?: boolean;
  provider: string;
  base_url: string;
  api_key: string;
  model: string;
  temperature: number;
  max_tokens: number;
  system_prompt: string;
  enable_tools: boolean;
  api_style?: string;
  response_mode?: string;
}

export interface SaveProfileResult {
  profile: AiSettings;
  profiles: AiSettings[];
}

export interface ChatMessage {
  id: string;
  role: string;
  content: string;
  created_at: string;
  tool_calls?: unknown;
}

export interface ChatSession {
  id: string;
  title: string;
  messages: ChatMessage[];
  created_at: string;
  updated_at: string;
}

export interface MemoryFact {
  id: string;
  content: string;
  created_at: string;
}

export interface DbCaps {
  owner: boolean;
  template: boolean;
  encoding: boolean;
  collate: boolean;
  ctype: boolean;
  tablespace: boolean;
  connLimit: boolean;
  allowConnect: boolean;
  isTemplate: boolean;
  comment: boolean;
  charset: boolean;
  extensions: boolean;
  immutableOnAlter: string[];
}

export interface DbSpec {
  name: string;
  newName?: string | null;
  owner?: string | null;
  template?: string | null;
  encoding?: string | null;
  collate?: string | null;
  ctype?: string | null;
  tablespace?: string | null;
  connLimit?: number | null;
  allowConnect?: boolean | null;
  isTemplate?: boolean | null;
  comment?: string | null;
  charset?: string | null;
  collation?: string | null;
}

export interface DbOptions {
  caps: DbCaps;
  owners: string[];
  templates: string[];
  encodings: string[];
  collations: string[];
  tablespaces: string[];
  charsets: string[];
  collationNames: string[];
  current: DbSpec | null;
}

export interface ExtInfo {
  name: string;
  defaultVersion: string;
  installedVersion: string | null;
  comment: string | null;
  schema: string | null;
}

export interface ColSpec {
  name: string;
  dataType?: string | null;
  nullable?: boolean | null;
  primaryKey?: boolean | null;
  autoIncrement?: boolean | null;
  defaultValue?: string | null;
  comment?: string | null;
}

export interface TableSpec {
  name: string;
  columns: ColSpec[];
  comment?: string | null;
  engine?: string | null;
  charset?: string | null;
}

export interface MigrateOptions {
  withStructure: boolean;
  withData: boolean;
  withIndexes: boolean;
  onExists: "skip" | "recreate" | "append";
  maxRowsPerTable: number;
}

export interface MigrateTableResult {
  table: string;
  status: "created" | "copied" | "skipped" | "failed";
  rows: number;
  detail: string;
}

export interface MigrateReport {
  tables: MigrateTableResult[];
  totalRows: number;
  created: number;
  copied: number;
  skipped: number;
  failed: number;
}

export interface SyncTableResult {
  table: string;
  status: "created" | "copied" | "failed";
  rows: number;
  failedRows: number;
  detail: string;
}

export interface SyncReport {
  tables: SyncTableResult[];
  totalRows: number;
  failedRows: number;
  created: number;
  copied: number;
  failed: number;
  message: string;
}

export type MaskMode =
  | "none"
  | "mask-all"
  | "keep-edges"
  | "email"
  | "phone"
  | "idcard"
  | "bankcard"
  | "name"
  | "hash"
  | "null"
  | "replace";

export interface MaskRule {
  column: string;
  mode: MaskMode;
  keepHead?: number | null;
  keepTail?: number | null;
  maskText?: string | null;
}

export interface ExportResult {
  content: string;
  rows: number;
  maskedColumns: string[];
  unmatchedColumns: string[];
}

export const api = {
  listConnections: () => invoke<ConnectionConfig[]>("list_connections"),
  saveConnection: (config: ConnectionConfig) =>
    invoke<ConnectionConfig>("save_connection", { config }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  testConnection: (config: ConnectionConfig) =>
    invoke<boolean>("test_connection", { config }),
  listDatabases: (connectionId: string) =>
    invoke<string[]>("list_databases", { connectionId }),
  listSchemas: (connectionId: string, database: string) =>
    invoke<string[]>("list_schemas", { connectionId, database }),
  listTables: (connectionId: string, database: string, schema?: string | null) =>
    invoke<TableInfo[]>("list_tables", {
      connectionId,
      database,
      schema: schema ?? null,
    }),
  listColumns: (
    connectionId: string,
    database: string,
    table: string,
    schema?: string | null
  ) =>
    invoke<ColumnInfo[]>("list_columns", {
      connectionId,
      database,
      table,
      schema: schema ?? null,
    }),
  listIndexes: (
    connectionId: string,
    database: string,
    table: string,
    schema?: string | null
  ) =>
    invoke<IndexInfo[]>("list_indexes", {
      connectionId,
      database,
      table,
      schema: schema ?? null,
    }),
  listForeignKeys: (
    connectionId: string,
    database: string,
    table: string,
    schema?: string | null
  ) =>
    invoke<ForeignKeyInfo[]>("list_foreign_keys", {
      connectionId,
      database,
      table,
      schema: schema ?? null,
    }),
  listTablespaces: (connectionId: string) =>
    invoke<TablespaceInfo[]>("list_tablespaces", { connectionId }),
  describeTableExtra: (
    connectionId: string,
    database: string,
    table: string,
    schema?: string | null
  ) =>
    invoke<TableExtra>("describe_table_extra", {
      connectionId,
      database,
      table,
      schema: schema ?? null,
    }),
  queryPage: (
    connectionId: string,
    database: string,
    sql: string,
    page: number,
    pageSize: number
  ) =>
    invoke<QueryPage>("query_page", {
      connectionId,
      database,
      sql,
      page,
      pageSize,
    }),
  executeSql: (connectionId: string, database: string, sql: string) =>
    invoke<ExecResult>("execute_sql", { connectionId, database, sql }),
  insertRow: (
    connectionId: string,
    database: string,
    table: string,
    data: Record<string, unknown>
  ) => invoke<ExecResult>("insert_row", { connectionId, database, table, data }),
  updateRow: (
    connectionId: string,
    database: string,
    table: string,
    pk: Record<string, unknown>,
    data: Record<string, unknown>
  ) => invoke<ExecResult>("update_row", { connectionId, database, table, pk, data }),
  deleteRow: (
    connectionId: string,
    database: string,
    table: string,
    pk: Record<string, unknown>
  ) => invoke<ExecResult>("delete_row", { connectionId, database, table, pk }),
  reconnect: (connectionId: string) =>
    invoke<void>("reconnect", { connectionId }),
  listAiProfiles: () => invoke<AiSettings[]>("list_ai_profiles"),
  saveAiProfile: (profile: AiSettings) =>
    invoke<SaveProfileResult>("save_ai_profile", { profile }),
  deleteAiProfile: (id: string) =>
    invoke<AiSettings[]>("delete_ai_profile", { id }),
  setDefaultAiProfile: (id: string) =>
    invoke<AiSettings[]>("set_default_ai_profile", { id }),
  listChatSessions: () => invoke<ChatSession[]>("list_chat_sessions"),
  createChatSession: (title?: string) =>
    invoke<ChatSession>("create_chat_session", { title }),
  deleteChatSession: (id: string) => invoke<void>("delete_chat_session", { id }),
  renameChatSession: (id: string, title: string) =>
    invoke<void>("rename_chat_session", { id, title }),
  listMemoryFacts: () => invoke<MemoryFact[]>("list_memory_facts"),
  addMemoryFact: (content: string) =>
    invoke<void>("add_memory_fact", { content }),
  clearMemoryFacts: () => invoke<void>("clear_memory_facts"),
  aiChat: (sessionId: string, userMessage: string, profileId?: string | null) =>
    invoke<ChatSession>("ai_chat", {
      sessionId,
      userMessage,
      profileId: profileId ?? null,
    }),
  testAiConnection: (settings: AiSettings) =>
    invoke<string>("test_ai_connection", { settings }),
  exportQuery: (
    connectionId: string,
    database: string,
    sql: string,
    format: string,
    maxRows?: number,
    masks?: MaskRule[]
  ) =>
    invoke<ExportResult>("export_query", {
      connectionId,
      database,
      sql,
      format,
      maxRows: maxRows ?? null,
      masks: masks && masks.length ? masks : null,
    }),
  dumpTable: (
    connectionId: string,
    database: string,
    table: string,
    includeStructure: boolean,
    includeData: boolean,
    maxRows?: number
  ) =>
    invoke<string>("dump_table", {
      connectionId,
      database,
      table,
      includeStructure,
      includeData,
      maxRows: maxRows ?? null,
    }),
  dumpDatabase: (
    connectionId: string,
    database: string,
    includeStructure: boolean,
    includeData: boolean,
    maxRows?: number
  ) =>
    invoke<string>("dump_database", {
      connectionId,
      database,
      includeStructure,
      includeData,
      maxRows: maxRows ?? null,
    }),
  syncTable: (args: {
    sourceConnectionId: string;
    sourceDatabase: string;
    sourceTable: string;
    targetConnectionId: string;
    targetDatabase: string;
    targetTable: string;
    createIfMissing: boolean;
    maxRows?: number;
  }) => invoke<SyncReport>("sync_table", args),
  syncDatabase: (args: {
    sourceConnectionId: string;
    sourceDatabase: string;
    targetConnectionId: string;
    targetDatabase: string;
    createIfMissing: boolean;
    maxRows?: number;
  }) => invoke<SyncReport>("sync_database", args),
  writeFile: (path: string, content: string) =>
    invoke<void>("write_text_file", { path, content }),
  readFile: (path: string) => invoke<string>("read_text_file", { path }),
  importTabularFile: (args: {
    connectionId: string;
    database: string;
    table: string;
    path: string;
    format: string;
    createIfMissing: boolean;
  }) => invoke<string>("import_tabular_file", args),
  runSqlScript: (args: {
    connectionId: string;
    database: string;
    path: string;
  }) => invoke<string>("run_sql_script", args),
  getDatabaseOptions: (args: { connectionId: string; database: string }) =>
    invoke<DbOptions>("get_database_options", args),
  planDatabase: (args: { connectionId: string; action: string; spec: DbSpec }) =>
    invoke<string[]>("plan_database", args),
  applyDatabase: (args: { connectionId: string; action: string; spec: DbSpec }) =>
    invoke<string[]>("apply_database", args),
  listExtensions: (args: { connectionId: string; database: string }) =>
    invoke<ExtInfo[]>("list_extensions", args),
  applyExtension: (args: {
    connectionId: string;
    database: string;
    action: string;
    name: string;
  }) => invoke<string[]>("apply_extension", args),
  planCreateTable: (args: { connectionId: string; spec: TableSpec }) =>
    invoke<string[]>("plan_create_table", args),
  applyCreateTable: (args: {
    connectionId: string;
    database: string;
    spec: TableSpec;
  }) => invoke<string[]>("apply_create_table", args),
  migrateDatabase: (args: {
    sourceConnectionId: string;
    sourceDatabase: string;
    targetConnectionId: string;
    targetDatabase: string;
    options: MigrateOptions;
  }) => invoke<MigrateReport>("migrate_database", args),
};
