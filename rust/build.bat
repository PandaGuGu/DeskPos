@echo off
chcp 65001 >nul
setlocal
cd /d "%~dp0"

echo ============================================
echo  DeskPos Rust 版构建
echo ============================================
echo.

where cargo >nul 2>nul
if errorlevel 1 (
    echo [x] 没找到 cargo，先装 Rust: https://rustup.rs
    pause
    exit /b 1
)

echo [1/3] 编译命令行版 deskpos.exe ...
cargo build --release --no-default-features --bin deskpos
if errorlevel 1 goto fail

echo [2/3] 编译图形版 deskpos-gui.exe ...
cargo build --release --features gui --bin deskpos-gui
if errorlevel 1 goto fail

echo [3/3] 收集产物到 dist ...
if not exist dist mkdir dist
copy /y target\release\deskpos.exe dist\ >nul
copy /y target\release\deskpos-gui.exe dist\ >nul

echo.
echo 完成，产物在 dist\ 目录：
dir /b dist
echo.
echo 用法: dist\deskpos-gui.exe      图形界面
echo       dist\deskpos.exe status   命令行
echo.
pause
exit /b 0

:fail
echo.
echo [x] 编译失败，看上面的报错
pause
exit /b 1
