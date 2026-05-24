@echo on
setlocal

set "CARGO_BUILD_TARGET=x86_64-pc-windows-msvc"
powershell.exe -NoLogo -ExecutionPolicy Bypass -File scripts\ci\circleci-release.ps1
if errorlevel 1 exit /b %ERRORLEVEL%

call scripts\ci\assert-release-assets.cmd
if errorlevel 1 exit /b %ERRORLEVEL%

exit /b 0
