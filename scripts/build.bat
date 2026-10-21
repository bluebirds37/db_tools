@echo off
REM AgentDB one-click packaging (Windows, NSIS installer). Extra args pass through, e.g. -Clean -Proxy http://127.0.0.1:7897
chcp 65001 >nul
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0agentdb.ps1" build %*
if errorlevel 1 pause
