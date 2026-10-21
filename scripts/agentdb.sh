#!/usr/bin/env bash
# AgentDB 一键脚本（Linux）
#
#   ./scripts/agentdb.sh check            只做环境体检
#   ./scripts/agentdb.sh dev              一键启动开发模式（自动装 npm 依赖）
#   ./scripts/agentdb.sh build            一键打包（.deb + .AppImage）
#
# 可选：
#   --with-deps    缺系统依赖时用 sudo apt-get 装上（仅限 Debian/Ubuntu）
#   --clean        打包前 cargo clean -p agentdb_app
#   --no-install   跳过 npm install
#   --bundles "deb appimage"   换成 rpm 等其它产物
#   --proxy URL    给子进程挂 http(s)_proxy（GitHub / crates 直连超时时用）
set -euo pipefail

usage() {
  cat <<'TXT'
用法：./scripts/agentdb.sh <命令> [选项]

  check   只做环境体检（默认）
  dev     一键启动开发模式（自动装 npm 依赖）
  build   一键打包（.deb + .AppImage）

选项：
  --with-deps              缺系统依赖时用 sudo apt-get 装上（仅 Debian/Ubuntu）
  --clean                  打包前 cargo clean -p agentdb_app
  --no-install             跳过 npm install
  --bundles "deb appimage" 换成 rpm 等其它产物
  --proxy URL              给子进程挂 http(s)_proxy（GitHub / crates 直连超时时用）
TXT
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
esac

CMD="${1:-check}"
[[ $# -gt 0 ]] && shift || true

WITH_DEPS=0 CLEAN=0 NO_INSTALL=0 BUNDLES="deb appimage" PROXY=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h | --help) usage; exit 0 ;;
    --with-deps) WITH_DEPS=1 ;;
    --clean) CLEAN=1 ;;
    --no-install) NO_INSTALL=1 ;;
    --bundles) shift; BUNDLES="${1:-deb appimage}" ;;
    --proxy) shift; PROXY="${1:-}" ;;
    *)
      echo "未知参数：$1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

FAILED=0
ok() { printf '    ok   %s\n' "$1"; }
warn() { printf '    WARN %s\n' "$1"; }
bad() {
  printf '    FAIL %s\n' "$1" >&2
  FAILED=$((FAILED + 1))
}
step() { printf '\n==> %s\n' "$1"; }

have() { command -v "$1" >/dev/null 2>&1; }

run() {
  if [[ -n "$PROXY" ]]; then
    export http_proxy="$PROXY" https_proxy="$PROXY" HTTP_PROXY="$PROXY" HTTPS_PROXY="$PROXY"
    echo "    （已挂代理 $PROXY）"
  fi
  echo "\$ $*"
  "$@"
}

check_env() {
  step "环境体检（$(uname -s) $(uname -m)）"

  if [[ "$(uname -s)" != "Linux" ]]; then
    bad "本脚本只用于 Linux；Windows 请用 scripts\\agentdb.ps1 或 scripts\\dev.bat"
    return
  fi

  if have node; then
    local nv major
    nv="$(node -v | tr -d 'v')"
    major="${nv%%.*}"
    if [[ "$major" -ge 18 ]]; then ok "node $nv"; else bad "node $nv 太旧，vite 6 需要 18+（建议 20）"; fi
  else
    bad "找不到 node，请装 Node.js 18+（推荐 https://nodejs.org 或 nvm）"
  fi

  have npm && ok "npm $(npm -v)" || bad "找不到 npm"
  have cargo && ok "$(cargo --version)" || bad "找不到 cargo，装 Rust 稳定版：curl https://sh.rustup.rs -sSf | sh"
  have rustc && ok "$(rustc --version)" || bad "找不到 rustc"

  # Tauri 在 Linux 上的图形栈依赖，缺哪个包名都列清楚
  local missing=()
  pkg-config --exists webkit2gtk-4.1 2>/dev/null ||
    pkg-config --exists webkit2gtk-4.0 2>/dev/null || missing+=("libwebkit2gtk-4.1-dev")
  pkg-config --exists gtk+-3.0 2>/dev/null || missing+=("libgtk-3-dev")
  pkg-config --exists librsvg-2.0 2>/dev/null || missing+=("librsvg2-dev")
  pkg-config --exists libssl 2>/dev/null || have pkg-config || missing+=("libssl-dev")
  pkg-config --exists libxdo 2>/dev/null || missing+=("libxdo-dev")
  have file || missing+=("file")
  have patchelf || missing+=("patchelf")
  have curl || missing+=("curl")

  if [[ ${#missing[@]} -eq 0 ]]; then
    ok "系统依赖齐全（webkit2gtk / gtk / rsvg / xdo / patchelf）"
  elif [[ $WITH_DEPS -eq 1 ]] && have apt-get; then
    step "安装缺失的系统依赖：${missing[*]}"
    run sudo apt-get update
    run sudo apt-get install -y --no-install-recommends "${missing[@]}"
  else
    bad "缺系统依赖：${missing[*]}"
    if have apt-get; then
      echo "    一条命令装齐：sudo apt-get install -y --no-install-recommends ${missing[*]} build-essential"
      echo "    或者重跑本脚本并加 --with-deps"
    else
      echo "    非 Debian 系请自行装等价包（Fedora: webkit2gtk4.1-devel gtk3-devel librsvg2-devel libxdo-devel patchelf file openssl-devel）"
    fi
  fi

  local icons=(icons/32x32.png icons/128x128.png icons/128x128@2x.png icons/icon.icns icons/icon.ico)
  local lost=()
  for i in "${icons[@]}"; do [[ -f "src-tauri/$i" ]] || lost+=("$i"); done
  if [[ ${#lost[@]} -eq 0 ]]; then ok "tauri 图标齐全"
  elif [[ "$CMD" == "build" ]]; then bad "缺图标：${lost[*]}（先跑 npx tauri icon）"
  else warn "缺图标：${lost[*]}（打包会失败，dev 不受影响）"; fi

  [[ -d node_modules ]] && ok "node_modules 已存在" || warn "还没装前端依赖，dev/build 会自动 npm install"
}

ensure_npm() {
  [[ $NO_INSTALL -eq 1 ]] && return 0
  [[ -d node_modules ]] && return 0
  step "安装前端依赖（npm install）"
  run npm install
}

start_dev() {
  ensure_npm
  step "启动开发模式（改代码即时热更新，Ctrl+C 退出）"
  run npx tauri dev
}

start_build() {
  if [[ $CLEAN -eq 1 ]]; then
    step "清理本 crate 的旧产物（cargo clean -p agentdb_app）"
    run cargo clean -p agentdb_app
  fi
  ensure_npm
  step "打包：$BUNDLES（首次编译较久）"
  # shellcheck disable=SC2086
  run npx tauri build --bundles $BUNDLES

  step "产物"
  if [[ -d src-tauri/target/release/bundle ]]; then
    find src-tauri/target/release/bundle -type f \( -name '*.deb' -o -name '*.AppImage' -o -name '*.rpm' \) -printf '    ok   %p (%s bytes)\n' | sort
    echo "    AppImage 若不能直接运行：chmod +x 那个 .AppImage"
  else
    bad "没找到 src-tauri/target/release/bundle"
  fi
}

echo "AgentDB — Agent for Any Database"
check_env

if [[ "$CMD" == "check" ]]; then
  [[ $FAILED -eq 0 ]] && echo -e "\n环境没问题。" || echo -e "\n有 $FAILED 项不满足，按上面的提示补一下。"
  exit "$FAILED"
fi

if [[ $FAILED -gt 0 ]]; then
  echo -e "\n先解决上面 $FAILED 个 FAIL 再跑 $CMD。" >&2
  exit 1
fi

case "$CMD" in
  dev) start_dev ;;
  build) start_build ;;
  *)
    echo "未知命令：$CMD" >&2
    usage >&2
    exit 2
    ;;
esac
