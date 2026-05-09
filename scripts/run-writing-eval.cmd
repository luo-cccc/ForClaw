@echo off
setlocal enabledelayedexpansion

set "ROOT=%~dp0.."
set "TIMESTAMP=%DATE:~0,10%T%TIME:~0,8%"
set "GIT_REV="
for /f "tokens=*" %%i in ('git -C "%ROOT%" rev-parse --short HEAD 2^>nul') do set "GIT_REV=%%i"

set "MODE=full"
if "%~1"=="--smoke" set "MODE=smoke"

echo === Writing Eval Harness ===
echo Mode: %MODE%
echo.

REM Build and run the JSONL eval runner
cargo run -p agent-writer --bin eval_runner --release -- --%MODE%
if errorlevel 1 (
    echo Eval runner failed.
    exit /b 1
)

echo.
echo === Eval Summary ===
echo Run metadata: {"run":"eval-%TIMESTAMP: =0%","git_rev":"%GIT_REV%","timestamp":"%TIMESTAMP: =0%","mode":"%MODE%"}
echo.
exit /b 0
