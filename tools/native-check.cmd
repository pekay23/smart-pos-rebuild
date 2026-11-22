@echo off
setlocal
if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" call "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if exist "%~dp0..\.planning\toolchain\strawberry\perl\bin\perl.exe" set "PERL=%~dp0..\.planning\toolchain\strawberry\perl\bin\perl.exe"
if exist "%~dp0..\.planning\toolchain\strawberry\c\bin" set "PATH=%~dp0..\.planning\toolchain\strawberry\c\bin;%PATH%"
cd /d "%~dp0.."
cargo %*
exit /b %errorlevel%
