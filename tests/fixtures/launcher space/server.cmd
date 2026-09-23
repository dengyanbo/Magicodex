@echo off
powershell.exe -NoLogo -NoProfile -NonInteractive -File "%~dp0..\app-server.ps1" %*
exit /b %ERRORLEVEL%
