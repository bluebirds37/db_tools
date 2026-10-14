import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportUiError } from "../services/logger";

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    void reportUiError("ErrorBoundary", error);
    console.error("UI error:", error, info);
  }

  render() {
    if (this.state.error) {
      return (
        <div
          className="flex flex-col items-center justify-center h-full gap-3 p-6"
          style={{ background: "var(--bg)", color: "var(--text)" }}
        >
          <div className="text-base font-semibold">界面出现异常</div>
          <div
            className="text-xs max-w-[520px] text-center"
            style={{ color: "var(--text-muted)" }}
          >
            {String(this.state.error?.message || this.state.error)}
          </div>
          <div
            className="text-[11px] max-w-[520px] text-center"
            style={{ color: "var(--text-subtle)" }}
          >
            错误已写入日志：~/.agentdb/logs/app.log
          </div>
          <button
            className="btn btn-primary"
            onClick={() => {
              this.setState({ error: null });
              window.location.reload();
            }}
          >
            重新加载
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
