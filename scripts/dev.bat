@echo off
REM AgentDB one-click dev launcher (Windows). Any extra args are passed through.
chcp 65001 >nul
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0agentdb.ps1" dev %*
if errorlevel 1 pause
