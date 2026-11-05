$py = "C:\Users\Amaury\AppData\Local\hermes\hermes-agent\venv\Scripts\python.exe"
$dir = Join-Path $env:TEMP "tts"
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $dir | Out-Null

$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText("$dir\input.txt", (Get-Clipboard -Raw), $utf8)

$proc = Start-Process $py -ArgumentList "`"D:\DEV\SANDBOX\SpeakWrite\legacy\gen.py`" `"$dir`"" -WindowStyle Hidden -PassThru

Add-Type -AssemblyName PresentationCore
$p = New-Object System.Windows.Media.MediaPlayer

while (-not (Test-Path "$dir\count.txt")) {
    if ($proc.HasExited) { break }
    Start-Sleep -Milliseconds 50
}
if (-not (Test-Path "$dir\count.txt")) { exit }
Start-Sleep -Milliseconds 100
$n = [int](Get-Content "$dir\count.txt")

for ($i = 0; $i -lt $n; $i++) {
    $f = "$dir\$i.mp3"
    while (-not (Test-Path $f)) {
        if ($proc.HasExited -and -not (Test-Path $f)) { break }
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path $f)) { break }
    $p.Open([Uri]$f)
    $p.Play()
    while (-not $p.NaturalDuration.HasTimeSpan) { Start-Sleep -Milliseconds 30 }
    Start-Sleep -Milliseconds ([int]$p.NaturalDuration.TimeSpan.TotalMilliseconds + 100)
    $p.Close()
}