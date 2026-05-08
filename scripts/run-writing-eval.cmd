@echo off
setlocal
echo Running writing eval harness...
echo.
echo Eval 1: forge_status
forge-agent-mcp stdio < fixtures\writing_eval\eval_init.jsonl
echo.
echo Eval 2: forge_list_chapters
echo For full automated eval, run the test suite.
echo.
echo Done.
exit /b 0
