// 一键启动：挑一个没被占用的端口，让 vite 与 tauri 用同一个端口。
//
//   node scripts/tauri-dev.mjs              正常启动
//   node scripts/tauri-dev.mjs --port 45000 从指定端口开始找
//   node scripts/tauri-dev.mjs --dry-run    只打印选中的端口与补丁内容
//
// vite 侧读 AGENTDB_DEV_PORT（见 vite.config.ts），tauri 侧用 `--config` 合并一份
// 临时补丁覆盖 build.devUrl，两边端口永远一致。
import { createServer } from "node:net";
import { spawn } from "node:child_process";
import { randomInt } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const BASE_PORT = 44100;
const MAX_TRIES = 40;

const args = process.argv.slice(2);
const has = (name) => args.includes(name);
const opt = (name) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? Number(args[i + 1]) : undefined;
};

/** 记下第一个撞上的端口，好把「为什么换端口」说清楚 */
let blocked = null;

/**
 * 只能靠真实 bind 探测：Windows 上 Hyper-V / WSL 会保留整段端口，
 * 既没有监听者也不出现在 netsh 的排除列表里，netstat 看了也看不出来。
 */
function listenOnce(port) {
  return new Promise((ok, no) => {
    const s = createServer();
    s.once("error", no);
    s.listen({ port, host: "127.0.0.1" }, () => s.close(() => ok()));
  });
}

/** 端口和紧随其后的 HMR 端口都可用才算可用 */
async function isFree(port) {
  for (const p of [port, port + 1]) {
    try {
      await listenOnce(p);
    } catch {
      blocked = p;
      return false;
    }
  }
  return true;
}

/** 系统分配的临时端口一定可用；再给它后面的 HMR 端口留一位 */
async function ephemeralPort() {
  for (let i = 0; i < 5; i++) {
    const s = createServer();
    await new Promise((ok, no) => {
      s.once("error", no);
      s.listen(0, "127.0.0.1", ok);
    });
    const port = s.address().port;
    await new Promise((ok) => s.close(ok));
    if (port < 65530 && (await isFree(port))) return port;
  }
  throw new Error("找不到可用端口");
}

async function pickPort(start) {
  const base = start ?? BASE_PORT;
  if (await isFree(base)) return base;
  const reason = blocked;
  for (let i = 1; i <= MAX_TRIES; i++) {
    const candidate = base + i + randomInt(0, 64);
    if (candidate > 65534) break;
    if (await isFree(candidate)) {
      blocked = reason;
      return candidate;
    }
  }
  blocked = reason;
  return ephemeralPort();
}

const wanted = opt("--port");

// 给环境体检用：只回答「基准端口能不能用」，不产生补丁
if (has("--check-port")) {
  const base = wanted ?? BASE_PORT;
  if (await isFree(base)) {
    console.log(`${base} usable`);
    process.exit(0);
  }
  console.log(`${base} unavailable（被占用或系统保留），下一个可用：${await pickPort(base + 1)}`);
  process.exit(3);
}

const port = await pickPort(wanted);
const devUrl = `http://localhost:${port}`;
const dir = mkdtempSync(join(tmpdir(), "agentdb-dev-"));
const patchFile = join(dir, "tauri-dev-port.json");
writeFileSync(patchFile, JSON.stringify({ build: { devUrl } }, null, 2));

console.log(`[agentdb] 开发端口 ${devUrl}`);
if (port !== (wanted ?? BASE_PORT)) {
  console.log(`[agentdb] ${blocked} 不可用（被占用或被 Hyper-V / WSL 保留），已自动换到 ${port}`);
}

if (has("--dry-run")) {
  console.log(`[agentdb] 配置补丁 ${patchFile}:\n${readFileSync(patchFile, "utf8")}`);
  rmSync(dir, { recursive: true, force: true });
  process.exit(0);
}

const child = spawn("npx", ["tauri", "dev", "--config", patchFile], {
  cwd: ROOT,
  stdio: "inherit",
  env: { ...process.env, AGENTDB_DEV_PORT: String(port) },
  shell: process.platform === "win32",
});

const cleanup = () => rmSync(dir, { recursive: true, force: true });
child.on("exit", (code, signal) => {
  cleanup();
  process.exit(signal ? 1 : (code ?? 0));
});
for (const sig of ["SIGINT", "SIGTERM"]) {
  process.on(sig, () => {
    child.kill(sig);
    cleanup();
  });
}
