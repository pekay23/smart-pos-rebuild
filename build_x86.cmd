@echo off
setlocal
pushd "%~dp0"
set "SMARTPOS_VSROOT=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
if not exist "%SMARTPOS_VSROOT%\VC\Auxiliary\Build\vcvars32.bat" (
  echo Visual Studio C++ x86 tools were not found.
  popd
  exit /b 1
)
call "%SMARTPOS_VSROOT%\VC\Auxiliary\Build\vcvars32.bat"
if errorlevel 1 (
  popd
  exit /b 1
)
set "SMARTPOS_CMAKE=%SMARTPOS_VSROOT%\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
"%SMARTPOS_CMAKE%" -S . -B build-msvc-x86 -G "NMake Makefiles"
if errorlevel 1 (
  popd
  exit /b 1
)
"%SMARTPOS_CMAKE%" --build build-msvc-x86
set "SMARTPOS_RESULT=%errorlevel%"
popd
exit /b %SMARTPOS_RESULT%
