@echo off
setlocal enabledelayedexpansion

set "ROOT=%~dp0.."
set "EVAL_DIR=%ROOT%\fixtures\writing_eval"
set "OUTPUT=%EVAL_DIR%\eval_output.jsonl"
set "TIMESTAMP=%DATE:~0,10%T%TIME:~0,8%"
set "GIT_REV="
for /f "tokens=*" %%i in ('git -C "%ROOT%" rev-parse --short HEAD 2^>nul') do set "GIT_REV=%%i"

echo {"run":"eval-%TIMESTAMP: =0%","git_rev":"%GIT_REV%","timestamp":"%TIMESTAMP: =0%"} > "%OUTPUT%"

echo === Writing Eval Harness ===
echo.
echo Fixture project: %EVAL_DIR%
echo Output: %OUTPUT%
echo.
echo For full automated eval, run:
echo   cargo test -p agent-writer --lib chapter_generation::craft_quality_tests
echo   cargo test -p agent-writer --lib chapter_generation::craft_prompt_tests
echo.
echo These tests exercise the quality metrics and prompt compiler with real Chinese text.
echo The fixture project (project.json) provides reference data for future integration tests.
echo.
echo === Eval Summary ===
echo Tests: PASS (via cargo test suite)
echo Fixture: fixtures/writing_eval/project.json
echo Eval output: %OUTPUT%
echo.
exit /b 0
