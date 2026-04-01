@echo off
REM Launch Chrome with CDP (Chrome DevTools Protocol) enabled
REM This preserves all your login sessions and cookies

setlocal

REM Detect Chrome location
set "CHROME="
if exist "%PROGRAMFILES%\Google\Chrome\Application\chrome.exe" (
    set "CHROME=%PROGRAMFILES%\Google\Chrome\Application\chrome.exe"
) else if exist "%PROGRAMFILES(X86)%\Google\Chrome\Application\chrome.exe" (
    set "CHROME=%PROGRAMFILES(X86)%\Google\Chrome\Application\chrome.exe"
) else if exist "%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe" (
    set "CHROME=%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe"
)

if "%CHROME%"=="" (
    echo Chrome not found. Please install Google Chrome.
    exit /b 1
)

REM Check if Chrome is already running
tasklist /FI "IMAGENAME eq chrome.exe" 2>NUL | find /I "chrome.exe" >NUL
if %ERRORLEVEL%==0 (
    echo Chrome is already running.
    echo Close all Chrome windows first, then run this script again.
    echo Or add --remote-debugging-port=9222 to your Chrome shortcut.
    exit /b 1
)

REM Launch Chrome with CDP
echo Launching Chrome with CDP on port 9222...
start "" "%CHROME%" --remote-debugging-port=9222 --user-data-dir="%LOCALAPPDATA%\Google\Chrome\User Data"
echo Chrome started with CDP enabled. Your login sessions are preserved.
