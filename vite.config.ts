import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error injected by tauri
const host = process.env.TAURI_DEV_HOST;
// 端口由 scripts/tauri-dev.mjs 选好并通过环境变量传进来；被占时它会自动换一个
const port = Number(process.env.AGENTDB_DEV_PORT) || 44100;

export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: port + 1,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
