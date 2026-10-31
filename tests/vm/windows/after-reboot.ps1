# The second part of Kiln's Windows integration scenario (ADR 0034): after scenario.ps1 left
# kilnd enabled and running, run.ps1 rebooted the VM and the test user logged on by itself.
# Checks that the login started kilnd and that it brought the web server back, then uninstalls,
# snapshots again and compares with scenario.ps1's first snapshot. Then the .internal stage
# (ADR 0033). Appends to C:\kiln-test\scenario.log; ASCII only, as Windows PowerShell reads this
# file as ANSI.
param([string] $Kiln = 'C:\kiln-test\bin\kiln.exe')
$ErrorActionPreference = 'Continue'
$dir = 'C:\kiln-test'
$log = Join-Path $dir 'scenario.log'
$status = Join-Path $dir 'status'
$script:failed = Test-Path (Join-Path $dir 'failed')
. (Join-Path $dir 'lib.ps1')
$script:started = Get-Content (Join-Path $dir 'started.txt') -Raw
$sites = Join-Path $dir 'Sites'
Status 'running'

Say '== after the reboot (ADR 0034)'
$boot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime
# Nothing has run a kiln command since the boot: what runs was started by the login.
# Windows runs the Run key's programs a while after the logon, later on a slow boot.
Check 'kilnd was started at login by its Run value' {
    $deadline = (Get-Date).AddMinutes(3)
    while (-not (Get-Process kilnd -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep 3 }
    if (-not (Get-Process kilnd -ErrorAction SilentlyContinue)) { throw 'kilnd is not running 3 minutes after the boot' }
}
Check 'kilnd brought the web server and the PHP workers back, as new processes' {
    $deadline = (Get-Date).AddSeconds(60)
    do { $caddy = Get-Process kiln-caddy -ErrorAction SilentlyContinue; if ($caddy) { break }; Start-Sleep 3 }
    until ((Get-Date) -gt $deadline)
    if (-not $caddy) { throw 'kiln-caddy is not running' }
    if ($caddy.StartTime -lt $boot) { throw "kiln-caddy started at $($caddy.StartTime), before the boot at $boot" }
    if (-not (Get-Process kiln-supervisor -ErrorAction SilentlyContinue)) { throw 'kiln-supervisor is not running' }
}
Check 'daemon status: the web server runs, and kilnd starts at login' {
    # The server is up once `kiln web start`, run by kilnd's watchdog, has finished.
    $deadline = (Get-Date).AddSeconds(60)
    do { $out = Kiln daemon status; if ($out.Contains('Web server: running')) { break }; Start-Sleep 3 }
    until ((Get-Date) -gt $deadline)
    Expect 'Web server: running' $out
    Expect 'Starts at login: yes' $out
}
Check 'a static site over HTTP' { Expect 'static from kiln' (Get (SiteUrl 'static-site.test')) }
Check 'a PHP site over trusted HTTPS, on its own PHP 8.3' {
    $page = Get (SiteUrl 'php-app.test')
    Expect 'php 8.3.' $page
    Expect 'via cgi-fcgi' $page
}
Check 'the Laravel app over HTTPS, PHP 8.4' { Expect 'Laravel' (Get (SiteUrl 'laravel-app.test')) }
# Herd's state is in the snapshot, its server on port 80 included. Herd's app starts its server,
# and the test VM's Herd has no login item, so open the app as its user would after a reboot.
# Kiln keeps the ports it chose next to Herd (8080), so the two don't collide.
Check 'Herd is running again after the reboot (its app opened, as a user would)' {
    if (-not (Get-NetTCPConnection -LocalPort 80 -State Listen -ErrorAction SilentlyContinue)) {
        Start-Process 'C:\Program Files\Herd\Herd.exe'
    }
    $deadline = (Get-Date).AddSeconds(120)
    do { $page = Get 'http://herdcheck.test/'; if ($page.Contains('herd check')) { break }; Start-Sleep 3 }
    until ((Get-Date) -gt $deadline)
    Expect 'herd check' $page
}

Say '== uninstall'
Status 'waiting-untrust-dialog'
$answerer = Start-Answerer 'Root Certificate Store'
Check 'kiln uninstall (UAC by the test VM setting; the certificate dialog with the VM''s mouse)' { Kiln uninstall --yes }
Say "     dialog: $(Receive-Job $answerer -Wait -AutoRemoveJob)"
Status 'running'
Start-Sleep -Seconds 2
& (Join-Path $dir 'snapshot.ps1') | Set-Content (Join-Path $dir 'after.txt') -Encoding utf8
Check 'Herd''s site still answers after the uninstall' { Expect 'herd check' (Get 'http://herdcheck.test/') }

Say '== compare (ADR 0024, section 2)'
Compare-Snapshot 'after.txt' 'after uninstall'

Say '== another TLD from the start (P2-10, ADR 0033)'
# A fresh Kiln that chose .internal: sites, the hosts block and the root follow it. Not
# trusted, so no certificate dialog; a static site over HTTP is enough to show the names.
Check 'web install, then ca init --tld internal: a root for .internal' {
    Kiln web install | Out-Null
    Expect 'names under .internal' (Kiln ca init --tld internal)
}
Check 'link static-site: served as static-site.internal after hosts sync' {
    Expect 'static-site.internal' (Kiln link static-site --path "$sites\static-site")
    Kiln hosts sync --yes | Out-Null
    $started = Kiln web start
    $url = [regex]::Match($started, 'http://static-site\.internal[^\s]*').Value
    if (-not $url) { throw "no .internal URL in:`n$started" }
    Expect 'static from kiln' (Get $url)
    Expect 'static-site.internal' (Get-Content "$env:SystemRoot\System32\drivers\etc\hosts" -Raw)
}
Check 'kiln uninstall again' { Kiln uninstall --yes }
Start-Sleep -Seconds 2
& (Join-Path $dir 'snapshot.ps1') | Set-Content (Join-Path $dir 'after2.txt') -Encoding utf8
Compare-Snapshot 'after2.txt' 'after the second uninstall too'
Say $(if ($script:failed) { '== FAILED' } else { '== PASSED' })
Status 'done'
