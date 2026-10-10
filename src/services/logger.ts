import { invoke } from "@tauri-apps/api/core";

let installed = false;

export function installFrontendErrorCapture() {
  if (installed) return;
  installed = true;

  window.addEventListener("error", (ev) => {
    void report({
      message: ev.message || "window.error",
      stack: ev.error?.stack,
      source: `${ev.filename}:${ev.lineno}:${ev.colno}`,
    });
  });

  window.addEventListener("unhandledrejection", (ev) => {
    const reason = ev.reason;
    void report({
      message:
        typeof reason === "string"
          ? reason
          : reason?.message || "unhandledrejection",
      stack: reason?.stack,
      source: "unhandledrejection",
    });
  });
}

async function report(payload: {
  message: string;
  stack?: string;
  source?: string;
}) {
  try {
    await invoke("log_frontend_error", {
      message: payload.message,
      stack: payload.stack ?? null,
      source: payload.source ?? null,
    });
  } catch {
    // ignore - backend may not be ready
  }
  // also keep a console breadcrumb
  console.error("[db_tools]", payload.message, payload.stack || "");
}

export async function reportUiError(
  context: string,
  error: unknown
): Promise<void> {
  const message =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : String(error);
  await report({
    message: `[${context}] ${message}`,
    stack: error instanceof Error ? error.stack : undefined,
    source: context,
  });
}

export async function fetchLogTail(maxLines = 200): Promise<string> {
  try {
    return await invoke<string>("get_log_tail", { maxLines });
  } catch {
    return "";
  }
}

export async function getLogDir(): Promise<string> {
  try {
    return await invoke<string>("open_log_dir");
  } catch {
    return "";
  }
}
