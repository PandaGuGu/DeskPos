@echo off
chcp 65001 >nul
schtasks /change /tn "DeskPosLayoutRestore" /enable >nul 2>&1
if errorlevel 1 (
  echo [X] Task not found. Re-register it first.
) else (
  echo [OK] Auto-restore on logon: ENABLED
)
pause
