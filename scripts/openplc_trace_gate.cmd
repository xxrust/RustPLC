@echo off
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0openplc_trace_gate.ps1" %*
exit /b %ERRORLEVEL%
