# Spike 03, inside the Windows test VM (docs/spikes/03-windows-cache.md): a Laravel app made
# with Kiln's PHP and Composer, run against Garnet. Prints facts and the check lines.
#   guest.ps1 -Dir C:\kiln-spike
param([string] $Dir = 'C:\kiln-spike')
$ErrorActionPreference = 'Continue'   # native stderr must not end the run (Windows PowerShell 5.1)
$K = 'C:\kiln-test\bin\kiln.exe'
function Step($text) { Write-Output "== $text" }

Step 'environment'
"windows: $((Get-CimInstance Win32_OperatingSystem).Caption) $((Get-CimInstance Win32_OperatingSystem).Version)"
"dotnet on PATH: $([bool](Get-Command dotnet -ErrorAction SilentlyContinue))"
"dotnet installed (HKLM:\SOFTWARE\dotnet): $(Test-Path 'HKLM:\SOFTWARE\dotnet')"
"dotnet folder: $(Test-Path "$env:ProgramFiles\dotnet")"

Step 'Garnet: unpack under a Kiln name'
$garnet = Join-Path $Dir 'garnet'
Remove-Item -Recurse -Force $garnet -ErrorAction SilentlyContinue
Expand-Archive (Join-Path $Dir 'garnet-win-x64.zip') $garnet
$bin = Join-Path $garnet 'net10.0'
Rename-Item (Join-Path $bin 'GarnetServer.exe') 'kiln-garnet.exe'
$exe = Join-Path $bin 'kiln-garnet.exe'
"files: $((Get-ChildItem $bin -Recurse -File).Count), $([math]::Round((Get-ChildItem $bin -Recurse -File | Measure-Object Length -Sum).Sum / 1MB, 1)) MB"
Step '.NET 10 runtime: unpacked privately, found through DOTNET_ROOT'
$runtime = Join-Path $Dir 'dotnet'
Remove-Item -Recurse -Force $runtime -ErrorAction SilentlyContinue
Expand-Archive (Join-Path $Dir 'dotnet-runtime-10.0.14-win-x64.zip') $runtime
"runtime: $((Get-ChildItem $runtime -Recurse -File).Count) files, $([math]::Round((Get-ChildItem $runtime -Recurse -File | Measure-Object Length -Sum).Sum / 1MB, 1)) MB"
$env:DOTNET_ROOT = $runtime   # this session and the processes it starts only
$help = & $exe --help 2>&1 | Out-String
"--help exit: $LASTEXITCODE"
"help mentions --lua: $($help -match '--lua')"
($help -split "`n" | Select-String -Pattern '--bind|--port|--lua|--checkpointdir|--recover|--memory|--aof' | ForEach-Object { "  $($_.Line.Trim())" }) -join "`n"

Step 'Garnet: start'
$data = Join-Path $Dir 'garnet-data'
New-Item -ItemType Directory -Force $data | Out-Null
$clock = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process $exe -ArgumentList '--bind', '127.0.0.1', '--port', '6379', '--lua' `
    -RedirectStandardOutput (Join-Path $Dir 'garnet.out') -RedirectStandardError (Join-Path $Dir 'garnet.err') -PassThru -WindowStyle Hidden
while (-not (Test-NetConnection 127.0.0.1 -Port 6379 -InformationLevel Quiet -WarningAction SilentlyContinue) -and $clock.Elapsed.TotalSeconds -lt 60) {
    if ($p.HasExited) { break }
    Start-Sleep -Milliseconds 100
}
"started: $(-not $p.HasExited), listening after $([int]$clock.Elapsed.TotalMilliseconds) ms, process name $($p.ProcessName)"
"listening on: $((Get-NetTCPConnection -OwningProcess $p.Id -State Listen -ErrorAction SilentlyContinue | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) -join ', ')"
Get-Content (Join-Path $Dir 'garnet.out') -TotalCount 12 -ErrorAction SilentlyContinue | ForEach-Object { "  out: $_" }

Step 'Laravel app with Kiln PHP and Composer'
$env:COMPOSER_HOME = Join-Path $Dir 'composer-home'
$env:COMPOSER_CACHE_DIR = Join-Path $Dir 'composer-cache'
& $K php install 8.4 *> (Join-Path $Dir 'php.txt')
& $K composer install *> (Join-Path $Dir 'composer.txt')
& $K shims install --yes *> (Join-Path $Dir 'shims.txt')
$composer = Join-Path $env:LOCALAPPDATA 'Kiln\bin\shims\composer.exe'
$php = Join-Path $env:LOCALAPPDATA 'Kiln\bin\shims\php.exe'
$app = Join-Path $Dir 'app'
Remove-Item -Recurse -Force $app -ErrorAction SilentlyContinue
Push-Location $Dir
& $composer create-project laravel/laravel app --no-interaction --prefer-dist *> (Join-Path $Dir 'create.txt')
Pop-Location
"create-project exit: $LASTEXITCODE"
Push-Location $app
& $composer require predis/predis --no-interaction *> (Join-Path $Dir 'predis.txt')
"predis exit: $LASTEXITCODE"
"laravel: $(& $php artisan --version)"
"php: $(& $php -r 'echo PHP_VERSION;'), phpredis: $((& $php -m) -contains 'redis')"
New-Item -ItemType Directory -Force (Join-Path $app 'app\Console\Commands') | Out-Null
Copy-Item (Join-Path $Dir 'KilnSpike.php') (Join-Path $app 'app\Console\Commands\KilnSpike.php')
$envFile = Join-Path $app '.env'
$text = (Get-Content $envFile -Raw) -replace '(?m)^REDIS_CLIENT=.*$', 'REDIS_CLIENT=predis' `
    -replace '(?m)^CACHE_STORE=.*$', 'CACHE_STORE=redis'
if ($text -notmatch '(?m)^REDIS_CLIENT=') { $text += "`nREDIS_CLIENT=predis`n" }
Set-Content $envFile $text -NoNewline

Step 'checks against Garnet'
& $php artisan kiln:spike 2>&1
"exit: $LASTEXITCODE"

Step 'Garnet: after the checks'
$p.Refresh()
"still running: $(-not $p.HasExited), working set $([math]::Round($p.WorkingSet64 / 1MB)) MB, private $([math]::Round($p.PrivateMemorySize64 / 1MB)) MB"

Step 'nothing installed system-wide'
"HKLM:\SOFTWARE\dotnet: $(Test-Path 'HKLM:\SOFTWARE\dotnet'), $env:ProgramFiles\dotnet: $(Test-Path "$env:ProgramFiles\dotnet"), machine DOTNET_ROOT: '$([Environment]::GetEnvironmentVariable('DOTNET_ROOT', 'Machine'))', user DOTNET_ROOT: '$([Environment]::GetEnvironmentVariable('DOTNET_ROOT', 'User'))'"

Step 'without --lua, for comparison'
Stop-Process -Id $p.Id -Force
Start-Sleep -Seconds 1
$p2 = Start-Process $exe -ArgumentList '--bind', '127.0.0.1', '--port', '6379' -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput (Join-Path $Dir 'garnet2.out') -RedirectStandardError (Join-Path $Dir 'garnet2.err')
$clock.Restart()
while (-not (Test-NetConnection 127.0.0.1 -Port 6379 -InformationLevel Quiet -WarningAction SilentlyContinue) -and $clock.Elapsed.TotalSeconds -lt 60) { Start-Sleep -Milliseconds 100 }
& $php artisan kiln:spike 2>&1 | Select-String -Pattern '^FAIL|PASSED|FAILED'
Stop-Process -Id $p2.Id -Force
Pop-Location
