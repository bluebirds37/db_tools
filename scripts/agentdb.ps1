<#
AgentDB 一键脚本（Windows）
  .\scripts\agentdb.ps1 dev            一键启动开发模式（自动装依赖）
  .\scripts\agentdb.ps1 build          一键打包（NSIS 安装包）
  .\scripts\agentdb.ps1 check          只做环境体检
可选参数：
  -Clean      打包前先 cargo clean -p agentdb_app（遇到 LNK2019 之类的坏产物时用）
  -NoInstall  跳过 npm install
  -Proxy URL  给子进程挂 HTTP(S)_PROXY（GitHub/crates 直连超时时用）
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('dev', 'build', 'check')]
    [string]$Command = 'check',
    [switch]$Clean,
    [switch]$NoInstall,
    [string]$Proxy
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()

# 仓库根目录（本脚本在 scripts/ 下）
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$script:Failed = @()
$script:Warned = @()

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Ok($msg) { Write-Host "    ok   $msg" -ForegroundColor Green }
function Bad($msg) { Write-Host "    FAIL $msg" -ForegroundColor Red; $script:Failed += $msg }
function Warn($msg) { Write-Host "    WARN $msg" -ForegroundColor Yellow; $script:Warned += $msg }

function Test-Cmd($name) {
    $null -ne (Get-Command $name -ErrorAction SilentlyContinue)
}

function Version-Ge($actual, [version]$need) {
    try { return [version]$actual -ge $need } catch { return $false }
}

function Invoke-Tool($exe, $arguments) {
    if ($Proxy) {
        $env:HTTP_PROXY = $Proxy
        $env:HTTPS_PROXY = $Proxy
        Write-Host "    （已挂代理 $Proxy）" -ForegroundColor DarkGray
    }
    Write-Host "`$ $exe $($arguments -join ' ')" -ForegroundColor DarkGray
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$exe 执行失败（退出码 $LASTEXITCODE）"
    }
}

function Check-Env {
    Step '环境体检'

    if (Test-Cmd node) {
        $nodeVer = (node -v).TrimStart('v')
        if (Version-Ge $nodeVer ([version]'18.0.0')) { Ok "node $nodeVer" }
        else { Bad "node $nodeVer 太旧，vite 6 需要 18+（建议 20 或 22）" }
    }
    else { Bad '找不到 node，请安装 Node.js 18+ 后重试' }

    if (Test-Cmd npm) { Ok "npm $(npm -v)" } else { Bad '找不到 npm（随 Node.js 一起装）' }

    if (Test-Cmd cargo) { Ok "cargo $((cargo --version) -replace '^cargo ', '')" }
    else { Bad '找不到 cargo，请装 Rust 稳定版：https://rustup.rs（Windows 需要 MSVC 工具链）' }

    if (Test-Cmd rustc) { Ok "rustc $((rustc --version) -replace '^rustc ', '')" }
    else { Bad '找不到 rustc' }

    # WebView2 运行时：Tauri 在 Windows 上的渲染依赖，装过 Edge/Chrome 一般都有
    $wv2 = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue
    if (-not $wv2) { $wv2 = Get-ItemProperty 'HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue }
    if ($wv2 -and $wv2.pv) { Ok "WebView2 运行时 $($wv2.pv)" }
    else { Warn '没检测到 WebView2 运行时；界面可能起不来，去微软官网装 Evergreen Runtime' }

    # 打包必须有图标，缺了 tauri build 会直接失败
    $icons = @('icons/32x32.png', 'icons/128x128.png', 'icons/128x128@2x.png', 'icons/icon.icns', 'icons/icon.ico')
    $missing = $icons | Where-Object { -not (Test-Path (Join-Path $Root "src-tauri\$_")) }
    if ($missing.Count -eq 0) { Ok 'tauri 图标齐全' }
    elseif ($Command -eq 'build') { Bad "缺图标：$($missing -join ', ')（先跑 npx tauri icon）" }
    else { Warn "缺图标：$($missing -join ', ')（打包会失败，dev 不受影响）" }

    if (Test-Path (Join-Path $Root 'node_modules')) { Ok 'node_modules 已存在' }
    else { Warn '还没装前端依赖，dev/build 会自动 npm install' }

    # 端口要真实 bind 才算数：Hyper-V / WSL 会保留整段，netstat 里看不到监听者
    if (Test-Cmd node) {
        $probe = node scripts/tauri-dev.mjs --check-port 2>&1
        if ($LASTEXITCODE -eq 0) { Ok "端口 44100 可用" }
        else { Write-Host "    info $probe；dev 会自动换到空闲端口" -ForegroundColor DarkGray }
    }
}

function Ensure-NpmDeps {
    if ($NoInstall) { return }
    if (Test-Path (Join-Path $Root 'node_modules')) { return }
    Step '安装前端依赖（npm install）'
    Invoke-Tool npm @('install')
}

function Start-Dev {
    Ensure-NpmDeps
    Step '启动开发模式（自动挑空闲端口，改代码即时热更新，Ctrl+C 退出）'
    Invoke-Tool npm @('run', 'tauri:dev')
}

function Start-Build {
    if ($Clean) {
        Step '清理本 crate 的旧产物（cargo clean -p agentdb_app）'
        Invoke-Tool cargo @('clean', '-p', 'agentdb_app')
    }
    Ensure-NpmDeps
    Step '打包 NSIS 安装包（首次编译较久）'
    # nsis 走离线模板，msi 需要额外下载 WiX，故默认只出 nsis
    Invoke-Tool npx @('tauri', 'build', '--bundles', 'nsis')

    Step '产物'
    $dir = Join-Path $Root 'src-tauri\target\release\bundle\nsis'
    if (Test-Path $dir) {
        Get-ChildItem $dir -File | ForEach-Object {
            Ok "$($_.Name)  ($([math]::Round($_.Length / 1MB, 1)) MB)  $($_.FullName)"
        }
    }
    else { Bad "没找到产物目录 $dir" }
}

try {
    Write-Host "AgentDB — Agent for Any Database" -ForegroundColor Magenta
    Check-Env
    if ($Command -eq 'check') {
        if ($script:Failed.Count -eq 0) { Write-Host "`n环境没问题。" -ForegroundColor Green }
        else { Write-Host "`n有 $($script:Failed.Count) 项不满足，按上面的提示补一下。" -ForegroundColor Red }
        exit $script:Failed.Count
    }
    if ($script:Failed.Count -gt 0) {
        Write-Host "`n先解决上面 $($script:Failed.Count) 个 FAIL 再跑 $($Command)。" -ForegroundColor Red
        exit 1
    }
    switch ($Command) {
        'dev' { Start-Dev }
        'build' { Start-Build }
    }
}
catch {
    Write-Host "`n出错了：$($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
