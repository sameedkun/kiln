# Kiln's services scenario on a GitHub Actions Windows runner (ADR 0039 §3, P3-13): snapshot,
# services added and exercised, uninstall, snapshot again, compare. No desktop session and no
# Herd here: those are checked by hand in the Windows VM before a release.
#   windows-services.ps1 -Kiln <path to kiln.exe>
# Reuses the VM scenario's helpers (lib.ps1) and snapshot (snapshot.ps1). The runner's image
# may run its own MySQL or PostgreSQL, so the checks read the port Kiln chose instead of
# assuming the usual one: another service on it is a coexistence check for free.
param([Parameter(Mandatory)] [string] $Kiln)
$ErrorActionPreference = 'Continue'
$dir = Join-Path $env:RUNNER_TEMP 'kiln-ci'
$log = Join-Path $dir 'scenario.log'
$status = Join-Path $dir 'status'
$vm = Join-Path $PSScriptRoot '..\vm\windows'
New-Item -ItemType Directory -Force $dir | Out-Null
Set-Content $log '' -Encoding utf8
$script:failed = $false
. (Join-Path $vm 'lib.ps1')

# Everything Say writes goes to the job's output as well.
$sayToFile = ${function:Say}
function Say($text) { & $sayToFile $text; Write-Host $text }

& (Join-Path $vm 'snapshot.ps1') | Set-Content (Join-Path $dir 'before.txt') -Encoding utf8
if (Select-String -Path (Join-Path $dir 'before.txt') -Pattern 'Kiln present' -Quiet) {
    Say 'FAIL Kiln is already set up here'; exit 2
}
Say "== runner: $((Get-CimInstance Win32_OperatingSystem).Caption)"
Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalPort -in 1025, 3306, 3307, 5432, 6379, 8025 } |
    ForEach-Object { Say "     already listening: $($_.LocalAddress):$($_.LocalPort) (PID $($_.OwningProcess))" }

# The port an add printed: "running as <name>: <role> 127.0.0.1:<port>".
function PortOf($out, $role) { [int][regex]::Match($out, "$role 127\.0\.0\.1:(\d+)").Groups[1].Value }

Say '== Mailpit'
$script:smtp = 0; $script:http = 0
Check 'service add mailpit: downloaded, checked, running on loopback' {
    $out = Kiln service add mailpit
    Expect 'running as mailpit-1' $out
    $script:smtp = PortOf $out 'smtp'; $script:http = PortOf $out 'http'
    if (-not $script:smtp -or -not $script:http) { throw "no ports in:`n$out" }
}
function MailTotal { (Invoke-RestMethod "http://127.0.0.1:$($script:http)/api/v1/messages" -TimeoutSec 5).total }
Check 'a mail sent over SMTP is caught (Mailpit''s API counts 1)' {
    Send-MailMessage -SmtpServer 127.0.0.1 -Port $script:smtp -From app@example.test -To you@example.test `
        -Subject 'from CI' -Body 'hello from kiln' -WarningAction SilentlyContinue
    if ((MailTotal) -ne 1) { throw "Mailpit has $(MailTotal) message(s)" }
}
Check 'service stop closes its ports; start brings it back with its mail' {
    Kiln service stop mailpit-1 | Out-Null
    if (Test-NetConnection 127.0.0.1 -Port $script:http -InformationLevel Quiet -WarningAction SilentlyContinue) {
        throw 'still answering after stop'
    }
    Kiln service start mailpit-1 | Out-Null
    if ((MailTotal) -ne 1) { throw 'the caught mail is gone after a restart' }
}

Say '== Ports next to other programs (ADR 0037)'
Check 'a second instance moves off the first one''s ports and says whose they are' {
    $out = Kiln service add mailpit --name mail-two
    Expect "used by Kiln's mailpit-1" $out
    Kiln service remove mail-two --delete-data | Out-Null
}
Kiln service stop mailpit-1 | Out-Null
$squatter = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList '-NoProfile', '-Command',
    "`$l = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $($script:smtp)); `$l.Start(); Start-Sleep 300"
Start-Sleep -Seconds 2
Check 'service start refuses a port another program took, naming it' {
    $out = (& $Kiln service start mailpit-1 2>&1 | ForEach-Object { "$_" }) -join "`n"
    if ($LASTEXITCODE -eq 0) { throw "it started:`n$out" }
    if ($out -notmatch "mailpit-1 can't start: port $($script:smtp) \(smtp\) is used by .*powershell.exe") { throw "no holder named:`n$out" }
    $global:LASTEXITCODE = 0
}
Check 'kiln doctor warns about the instance' {
    $out = (& $Kiln doctor 2>&1 | ForEach-Object { "$_" }) -join "`n"
    if ($out -notmatch "mailpit-1 can't run: port $($script:smtp) \(smtp\)") { throw "no warning:`n$out" }
    $global:LASTEXITCODE = 0
}
Stop-Process -Id $squatter.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1
Check 'service start works once the other program is gone' {
    Kiln service start mailpit-1 | Out-Null
    if ((MailTotal) -ne 1) { throw 'Mailpit is not back with its mail' }
}

Say '== MySQL and MariaDB (ADR 0038)'
function Sql($instance, $statement) { ($statement | & $Kiln db $instance 2>&1 | ForEach-Object { "$_" }) -join "`n" }
$script:mysqlPort = 0
Check 'service add mysql --version 8.4: from Oracle, set up and running on loopback' {
    $out = Kiln service add mysql --version 8.4
    Expect 'running as mysql-8.4' $out
    $script:mysqlPort = PortOf $out 'mysql'
}
Check 'service add mariadb --version 11.8: beside it, on another port' {
    $out = Kiln service add mariadb --version 11.8
    Expect 'running as mariadb-11.8' $out
    if ((PortOf $out 'mysql') -eq $script:mysqlPort) { throw "same port as mysql-8.4:`n$out" }
}
Check 'only loopback listens on their ports (no X plugin on 33060)' {
    $ours = (Kiln service list) -split "`n" | ForEach-Object { [regex]::Match($_, '127\.0\.0\.1:(\d+)').Groups[1].Value } | Where-Object { $_ }
    $wide = Get-NetTCPConnection -State Listen | Where-Object {
        "$($_.LocalPort)" -in $ours -and $_.LocalAddress -ne '127.0.0.1' -and
        (Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName -like 'kiln-*'
    }
    if ($wide) { throw "listening beyond loopback: $(($wide | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) -join ', ')" }
    if (Get-NetTCPConnection -State Listen -LocalPort 33060 -ErrorAction SilentlyContinue | Where-Object { (Get-Process -Id $_.OwningProcess).ProcessName -eq 'kiln-mysqld' }) {
        throw 'the X plugin is listening'
    }
}
foreach ($instance in 'mysql-8.4', 'mariadb-11.8') {
    Check "kiln db $instance`: a database, a table, a row" {
        $out = Sql $instance "CREATE DATABASE kiln_ci; CREATE TABLE kiln_ci.t (v VARCHAR(20)); INSERT INTO kiln_ci.t VALUES ('kept'); SELECT v FROM kiln_ci.t;"
        Expect 'kept' $out
    }
}
Check 'data survives a restart' {
    Kiln service restart mysql-8.4 | Out-Null
    Expect 'kept' (Sql 'mysql-8.4' 'SELECT v FROM kiln_ci.t;')
}
Check 'a stop shuts it down cleanly (its own log says so), and start brings it back' {
    Kiln service stop mariadb-11.8 | Out-Null
    $out = Kiln service logs mariadb-11.8 --lines 8
    if ($out -notmatch 'Shutdown complete') { throw "no clean shutdown in:`n$out" }
    Kiln service start mariadb-11.8 | Out-Null
    Expect 'kept' (Sql 'mariadb-11.8' 'SELECT v FROM kiln_ci.t;')
}

Say '== uninstall'
Check 'kiln uninstall refuses while services hold data, and changes nothing' {
    $out = (& $Kiln uninstall --yes 2>&1 | ForEach-Object { "$_" }) -join "`n"
    if ($LASTEXITCODE -eq 0) { throw "uninstall went ahead without --delete-data:`n$out" }
    Expect 'hold data' $out
    if (-not (Test-Path "$env:LOCALAPPDATA\Kiln")) { throw 'Kiln folders were touched' }
    $global:LASTEXITCODE = 0
}
Check 'kiln uninstall --delete-data' { Kiln uninstall --yes --delete-data }
Start-Sleep -Seconds 2
& (Join-Path $vm 'snapshot.ps1') | Set-Content (Join-Path $dir 'after.txt') -Encoding utf8
Compare-Snapshot 'after.txt' 'after uninstall'

Say $(if ($script:failed) { '== FAILED' } else { '== PASSED' })
exit $(if ($script:failed) { 1 } else { 0 })
