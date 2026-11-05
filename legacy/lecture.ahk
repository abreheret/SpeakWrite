lecturePid := 0

F9::
{
    global lecturePid
    if lecturePid
        Run 'taskkill /PID ' lecturePid ' /T /F', , "Hide"
    Run 'powershell -NoProfile -ExecutionPolicy Bypass -File "D:\DEV\SANDBOX\SpeakWrite\legacy\lire.ps1"', , "Hide", &lecturePid
}

F10::
{
    global lecturePid
    if lecturePid
        Run 'taskkill /PID ' lecturePid ' /T /F', , "Hide"
}