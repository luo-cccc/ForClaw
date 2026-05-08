@echo off
setlocal

set "ROOT=%~dp0.."
set "BIN=%ROOT%\target\release\forge-agent-mcp.exe"
if not exist "%BIN%" set "BIN=%ROOT%\target\debug\forge-agent-mcp.exe"

if not exist "%BIN%" (
  >&2 echo forge-agent-mcp.exe was not found. Build it with: cargo build -p forge-agent-mcp
  exit /b 1
)

if "%FORGE_AGENT_DATA_DIR%"=="" set "FORGE_AGENT_DATA_DIR=%ROOT%\.forge-agent-data"

"%BIN%" %*
