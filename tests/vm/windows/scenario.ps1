# Kiln's Windows integration scenario (ADR 0024 §1, §4-5): snapshot, a full Kiln session next
# to a running Laravel Herd (including Herd restarting), uninstall, snapshot again, compare.
# Runs inside the Windows test VM, in the logged-on test user's desktop session (run.ps1 starts
# it as a scheduled task), not elevated: UAC elevates Kiln's helper without asking there
# (setup-test-vm.ps1). Results go to C:\kiln-test\scenario.log; C:\kiln-test\status tells the
# host what's happening ("waiting-trust-dialog" while Windows' certificate dialog is open).
param([string] $Kiln = 'C:\kiln-test\bin\kiln.exe')
$ErrorActionPreference = 'Continue'
$dir = 'C:\kiln-test'
$log = Join-Path $dir 'scenario.log'
$status = Join-Path $dir 'status'
Set-Content $log '' -Encoding utf8
$script:failed = $false

function Say($text) { Add-Content $log $text -Encoding utf8 }
function Status($text) { Set-Content $status $text -Encoding ascii }
function Check($desc, [scriptblock] $test) {
    try {
        $out = & $test 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne $null) { throw "exit code $LASTEXITCODE`n$out" }
        Say "PASS $desc"
    } catch {
        Say "FAIL $desc"
        ($_ | Out-String).Trim().Split("`n") | Select-Object -Last 8 | ForEach-Object { Say "     $_" }
        $script:failed = $true
    }
}
function Kiln { & $Kiln @args 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { throw "kiln $args failed ($LASTEXITCODE)" } }
function Get($url) { curl.exe -sS --ssl-revoke-best-effort $url 2>&1 | Out-String }
function Expect($text, $output) { if (-not $output.Contains($text)) { throw "expected '$text' in:`n$output" } }

Status 'running'
& (Join-Path $dir 'snapshot.ps1') | Set-Content (Join-Path $dir 'before.txt') -Encoding utf8
if (Select-String -Path (Join-Path $dir 'before.txt') -Pattern 'Kiln present' -Quiet) {
    Say 'FAIL Kiln is already set up here'; Status 'done'; exit 2
}

Say '== session'
$sites = Join-Path $dir 'Sites'
Remove-Item -Recurse -Force $sites -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$sites\static-site", "$sites\php-app" | Out-Null
Set-Content "$sites\static-site\index.html" '<h1>static from kiln</h1>'
Set-Content "$sites\php-app\index.php" '<?php echo "php ", PHP_VERSION, " via ", PHP_SAPI;'

Check 'php install 8.4' { Kiln php install 8.4 }
Check 'web install' { Kiln web install }
Check 'ca init' { Kiln ca init }
Check 'park the sites folder' { Kiln park $sites }
Check 'secure php-app' { Kiln secure php-app }
Check 'hosts sync (UAC, answered by the test VM setting)' { Kiln hosts sync --yes }
Status 'waiting-trust-dialog'
Check 'ca trust (Windows'' certificate dialog, answered by the host)' { Kiln ca trust --yes }
Status 'running'
Check 'shims install' { Kiln shims install --yes }
$started = ''
Check 'web start next to Herd moves to free ports and says why' {
    $script:started = Kiln web start
    Expect 'used by Laravel Herd' $script:started
    Expect '8080' $script:started
}
Check 'a static site over HTTP' { Expect 'static from kiln' (Get 'http://static-site.test:8080/') }
Check 'a PHP site over HTTPS, trusted for this user' { Expect 'via cgi-fcgi' (Get 'https://php-app.test:8443/') }
Check 'kiln doctor reports Herd under Other tools' {
    $doctor = & $Kiln doctor 2>&1 | Out-String
    Expect 'Laravel Herd is installed' $doctor
}
Check 'the php shim in a new terminal runs Kiln''s PHP' {
    $out = powershell -NoProfile -Command "`$env:Path = [Environment]::GetEnvironmentVariable('Path','User') + ';' + [Environment]::GetEnvironmentVariable('Path','Machine'); Set-Location '$sites\php-app'; php -r 'echo PHP_VERSION;'" | Out-String
    Expect '8.4.' $out
}

Say '== Herd restarts while Kiln runs (I-11)'
$caddy = (Get-Process kiln-caddy).Id
Check 'herd restart' { & (Join-Path $env:USERPROFILE '.config\herd\bin\herd.bat') restart 2>&1 | Out-String }
Start-Sleep -Seconds 5
Check 'Kiln''s server survived it (same PID)' { if ((Get-Process kiln-caddy).Id -ne $caddy) { throw 'kiln-caddy restarted or died' } }
Check 'Kiln''s sites still answer' { Expect 'static from kiln' (Get 'http://static-site.test:8080/') }
Check 'Herd''s site answers' { Expect 'herd check' (Get 'http://herdcheck.test/') }

Say '== uninstall'
Check 'kiln uninstall (UAC answered by the test VM setting)' { Kiln uninstall --yes }
Start-Sleep -Seconds 2
& (Join-Path $dir 'snapshot.ps1') | Set-Content (Join-Path $dir 'after.txt') -Encoding utf8
Check 'Herd''s site still answers after the uninstall' { Expect 'herd check' (Get 'http://herdcheck.test/') }

Say '== compare (ADR 0024 §2)'
$diff = Compare-Object (Get-Content (Join-Path $dir 'before.txt')) (Get-Content (Join-Path $dir 'after.txt'))
if ($diff) {
    Say 'FAIL the machine differs after uninstall:'
    $diff | ForEach-Object { Say "     $($_.SideIndicator) $($_.InputObject)" }
    $script:failed = $true
} else {
    Say "PASS the machine is as it was: $((Get-Content (Join-Path $dir 'before.txt')).Count) facts compared"
}
Say $(if ($script:failed) { '== FAILED' } else { '== PASSED' })
Status 'done'
