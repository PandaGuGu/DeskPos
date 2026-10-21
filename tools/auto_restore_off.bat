@echo off
chcp 65001 >nul
schtasks /change /tn "DeskPosLayoutRestore" /disable >nul 2>&1
if errorlevel 1 (
  echo [X] Task not found.
) else (
  echo [OK] Auto-restore on logon: DISABLED
)
pause
