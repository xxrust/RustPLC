@echo off
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0st_codegen_matiec_gate.ps1" %*
exit /b %ERRORLEVEL%
