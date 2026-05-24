@echo on
powershell.exe -NoLogo -ExecutionPolicy Bypass -File scripts\ci\install-circleci-release-tools.ps1
if errorlevel 1 exit /b %ERRORLEVEL%
