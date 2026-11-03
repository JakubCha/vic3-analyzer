@echo off
rem Start the Oikonomia Analyzer (unless it already runs), then Victoria 3.
rem Double-click it, or put it in Steam launch options as:
rem   "M:\games\vic3_oikonomia\scripts\play-vic3.cmd" %command%
rem VIC3_GAME_DIR overrides the game folder (default M:\games\Victoria 3).
setlocal
set "ROOT=%~dp0.."
if not defined VIC3_GAME_DIR set "VIC3_GAME_DIR=M:\games\Victoria 3"
tasklist /FI "IMAGENAME eq vic3-analyzer.exe" | find /I "vic3-analyzer.exe" >nul || start "" "%ROOT%\bin\vic3-analyzer.exe"
if "%~1"=="" (
  start "" /D "%VIC3_GAME_DIR%\binaries" "%VIC3_GAME_DIR%\binaries\victoria3.exe"
) else (
  rem Launched by Steam: run the game command it passes.
  %*
)
