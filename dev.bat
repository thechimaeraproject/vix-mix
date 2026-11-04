@echo off
setlocal
REM One entry point for this repository: the dev contract (https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/dev-commands.md).
REM Dispatch lives in scripts\dev.mjs; this file only finds a Node and forwards.
set "NODE=node"
if defined KIT_NODE set "NODE=%KIT_NODE%"
"%NODE%" "%~dp0scripts\dev.mjs" %*
exit /b %ERRORLEVEL%
