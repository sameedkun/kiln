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
    Say "     ... $(Get-Date -Format HH:mm:ss) $desc"   # shows which step a stuck run is in
    $started = Get-Date
    try {
        $out = & $test 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne $null) { throw "exit code $LASTEXITCODE`n$out" }
        Say "PASS $desc ($([int]((Get-Date) - $started).TotalSeconds) s)"
    } catch {
        Say "FAIL $desc ($([int]((Get-Date) - $started).TotalSeconds) s)"
        ($_ | Out-String).Trim().Split("`n") | Select-Object -Last 8 | ForEach-Object { Say "     $_" }
        $script:failed = $true
    }
}
function Kiln {
    $out = & $Kiln @args 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw "kiln $args failed ($LASTEXITCODE): $($out.Trim())" }
    $out
}
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
# Project config (P2-04, ADR 0027): a parked project whose kiln.toml asks for PHP 8.3 and an
# alias, and a project elsewhere with a Laravel Herd herd.yml to import.
New-Item -ItemType Directory -Force "$sites\toml-app" | Out-Null
Set-Content "$sites\toml-app\index.php" '<?php echo "toml ", PHP_VERSION;'
Set-Content "$sites\toml-app\kiln.toml" "[site]`naliases = [`"toml-alias`"]`n[php]`nversion = `"8.3`"`n"
$herdApp = Join-Path $dir 'herd-app'
Remove-Item -Recurse -Force $herdApp -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $herdApp | Out-Null
Set-Content "$herdApp\index.php" '<?php echo "herd-import ", PHP_VERSION;'
Set-Content "$herdApp\herd.yml" "name: herd-demo`nphp: '8.3'`nsecured: true`naliases:`n  - herd-alias`n"

Check 'php install 8.4' { Kiln php install 8.4 }
Check 'php install 8.3 (a second version, for per-site PHP)' { Kiln php install 8.3 }
Check 'composer install' { Kiln composer install }
Check 'shims install' { Kiln shims install --yes }
Check 'web install' { Kiln web install }

# Real apps (plan.md, Phase 1 exit criteria): a new Laravel project made with Kiln's Composer,
# and WordPress from wordpress.org. Composer's cache and config go under C:\kiln-test, so the
# run leaves nothing of Composer's own behind either.
$env:COMPOSER_HOME = Join-Path $dir 'composer-home'
$env:COMPOSER_CACHE_DIR = Join-Path $dir 'composer-cache'
$composer = Join-Path $env:LOCALAPPDATA 'Kiln\bin\shims\composer.exe'
Check 'a new Laravel project, made with Kiln''s composer shim' {
    Push-Location $sites
    try {
        # One retry: a dropped download (Composer's exit code 100) is the network, not Kiln.
        foreach ($attempt in 1, 2) {
            Remove-Item -Recurse -Force laravel-app -ErrorAction SilentlyContinue
            $out = & $composer create-project laravel/laravel laravel-app --no-interaction --prefer-dist 2>&1 | Out-String
            if ($LASTEXITCODE -eq 0) { break }
        }
        if ($LASTEXITCODE -ne 0) { throw "composer create-project failed ($LASTEXITCODE):`n$out" }
    } finally { Pop-Location }
}
Check 'WordPress, from wordpress.org' {
    $archive = Join-Path $dir 'wordpress.tar.gz'
    curl.exe -fsSL --retry 5 --retry-delay 5 --retry-all-errors -o $archive https://wordpress.org/latest.tar.gz
    if ($LASTEXITCODE -ne 0) { throw "download failed ($LASTEXITCODE)" }
    tar.exe -xzf $archive -C $sites
    if ($LASTEXITCODE -ne 0) { throw "tar failed ($LASTEXITCODE)" }
    Rename-Item (Join-Path $sites 'wordpress') 'wp-site'
}
Check 'ca init' { Kiln ca init }
Check 'kiln import herd: herd.yml becomes kiln.toml and the folder is linked (P2-04)' {
    Expect 'herd-demo' (Kiln import herd $herdApp)
    Expect 'version = "8.3"' (Get-Content "$herdApp\kiln.toml" -Raw)
}
Check 'park the sites folder' { Kiln park $sites }
Check 'secure php-app' { Kiln secure php-app }
Check 'secure laravel-app' { Kiln secure laravel-app }
Check 'secure wp-site' { Kiln secure wp-site }
Check 'isolate php-app to PHP 8.3' { Kiln isolate 8.3 php-app }
Check 'hosts sync (UAC, answered by the test VM setting)' { Kiln hosts sync --yes }
# Windows asks before it adds a root to the user's store ("Security Warning") and before it
# deletes one ("Root Certificate Store"). Those dialogs open behind whatever window has focus
# (Windows doesn't let a background process take it), so keystrokes from the host can miss
# them, and their buttons are panes UI Automation can't press. So: find the dialog through UI
# Automation, minimize the windows that could cover it, and write where its Yes button is to
# C:\kiln-test\click, in screen pixels; run.ps1 clicks there with the VM's mouse. (Clicks made
# from inside this session by a scheduled task's process don't reach the dialog.)
function Start-Answerer($title) { Start-Job -ArgumentList $title {
    param($title)
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    Add-Type -Namespace KilnTest -Name Win -MemberDefinition '
        [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
        [DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr hwnd, int cmd);'
    [KilnTest.Win]::SetProcessDPIAware() | Out-Null   # positions in physical pixels
    $ae = [Windows.Automation.AutomationElement]
    $byName = { param($n) New-Object Windows.Automation.PropertyCondition($ae::NameProperty, $n) }
    $deadline = (Get-Date).AddMinutes(3)
    while ((Get-Date) -lt $deadline) {
        $dialog = $ae::RootElement.FindFirst('Children', (& $byName $title))
        if ($dialog) {
            foreach ($w in $ae::RootElement.FindAll('Children', [Windows.Automation.Condition]::TrueCondition)) {
                if ($w.Current.Name -ne $title -and $w.Current.ClassName -notin 'Shell_TrayWnd', 'Progman') {
                    [KilnTest.Win]::ShowWindow([IntPtr]$w.Current.NativeWindowHandle, 6) | Out-Null   # minimize
                }
            }
            Start-Sleep -Seconds 1
            $b = $dialog.FindFirst('Descendants', (& $byName 'Yes')).Current.BoundingRectangle
            Set-Content C:\kiln-test\click "$([int]($b.X + $b.Width / 2)) $([int]($b.Y + $b.Height / 2))"
            return "found; Yes is at $b"
        }
        Start-Sleep -Milliseconds 500
    }
    "no '$title' dialog seen"
} }

Status 'waiting-trust-dialog'
$answerer = Start-Answerer 'Security Warning'
Check 'ca trust (Windows'' certificate dialog, answered with the VM''s mouse)' { Kiln ca trust --yes }
Say "     dialog: $(Receive-Job $answerer -Wait -AutoRemoveJob)"
Status 'running'
$started = ''
# Herd holds port 80 (and 443 once it serves a site over HTTPS): Kiln takes the next free port
# for what's taken and prints each site's URL.
Check 'web start next to Herd moves off port 80 and says why' {
    $script:started = Kiln web start
    Expect 'used by Laravel Herd' $script:started
    Expect 'http://static-site.test:8080/' $script:started
}
function SiteUrl($host_) { [regex]::Match($script:started, "https?://$([regex]::Escape($host_))[^\s]*").Value }
Check 'a static site over HTTP' { Expect 'static from kiln' (Get (SiteUrl 'static-site.test')) }
Check 'a PHP site over HTTPS, trusted for this user, on its own PHP 8.3' {
    $url = SiteUrl 'php-app.test'
    Expect 'https://' $url
    $page = Get $url
    Expect 'php 8.3.' $page
    Expect 'via cgi-fcgi' $page
}
Check 'the Laravel app over HTTPS (served from public\, PHP 8.4)' {
    $url = SiteUrl 'laravel-app.test'
    Expect 'https://' $url
    Expect 'Laravel' (Get $url)
}
Check 'WordPress over HTTPS (its setup page)' {
    $url = SiteUrl 'wp-site.test'
    Expect 'https://' $url
    # Its setup page, not "Requirements Not Met", which WordPress shows without the mysqli
    # extension (I-62). Step 1 directly: step 0, the language chooser, fetches the language
    # list from api.wordpress.org and fails when that is slow.
    $page = curl.exe -sSL --ssl-revoke-best-effort "${url}wp-admin/setup-config.php?step=1" 2>&1 | Out-String
    Expect 'Setup Configuration File' $page
    if ($page.Contains('Requirements Not Met')) { throw "WordPress says: Requirements Not Met" }
}
Check 'kiln.toml: PHP 8.3 for toml-app and its alias (P2-04)' {
    Expect 'toml 8.3.' (Get (SiteUrl 'toml-app.test'))
    Expect 'toml 8.3.' (Get (SiteUrl 'toml-alias.test'))
}
Check 'the project imported from herd.yml: HTTPS, PHP 8.3 and its alias' {
    $url = SiteUrl 'herd-demo.test'
    Expect 'https://' $url
    Expect 'herd-import 8.3.' (Get $url)
    Expect 'herd-import 8.3.' (Get (SiteUrl 'herd-alias.test'))
}
Check 'kiln which names kiln.toml as the source' {
    Push-Location "$sites\toml-app"
    try { Expect 'kiln.toml' (Kiln which) } finally { Pop-Location }
}
Check 'kiln doctor reports Herd under Other tools' {
    $doctor = & $Kiln doctor 2>&1 | Out-String
    Expect 'Laravel Herd is installed' $doctor
}
function ShimPhp($folder) {
    powershell -NoProfile -Command "`$env:Path = [Environment]::GetEnvironmentVariable('Path','User') + ';' + [Environment]::GetEnvironmentVariable('Path','Machine'); Set-Location '$folder'; php -r 'echo PHP_VERSION;'" | Out-String
}
Check 'the php shim in a new terminal runs each site''s PHP (8.3 in php-app and toml-app, 8.4 in laravel-app)' {
    Expect '8.3.' (ShimPhp "$sites\php-app")
    Expect '8.3.' (ShimPhp "$sites\toml-app")
    Expect '8.4.' (ShimPhp "$sites\laravel-app")
}

Say '== Herd restarts while Kiln runs (I-11)'
$caddy = (Get-Process kiln-caddy).Id
Check 'herd restart' { & (Join-Path $env:USERPROFILE '.config\herd\bin\herd.bat') restart 2>&1 | Out-String }
Start-Sleep -Seconds 5
Check 'Kiln''s server survived it (same PID)' { if ((Get-Process kiln-caddy).Id -ne $caddy) { throw 'kiln-caddy restarted or died' } }
Check 'Kiln''s sites still answer' { Expect 'static from kiln' (Get 'http://static-site.test:8080/') }
Check 'Herd''s site answers again (within a minute of its restart)' {
    $deadline = (Get-Date).AddMinutes(1)
    do { $page = Get 'http://herdcheck.test/'; if ($page.Contains('herd check')) { break }; Start-Sleep -Seconds 3 }
    until ((Get-Date) -gt $deadline)
    Expect 'herd check' $page
}

Say '== kilnd (ADR 0026)'
Check 'daemon start' { Kiln daemon start }
Check 'daemon status reports the web server running' { Expect 'Web server: running' (Kiln daemon status) }
Check 'the daemon restarts a web server that died' {
    $old = (Get-Process kiln-caddy).Id
    Stop-Process -Id $old -Force
    $deadline = (Get-Date).AddSeconds(60)
    do {
        Start-Sleep -Seconds 3
        $now = Get-Process kiln-caddy -ErrorAction SilentlyContinue
    } until (($now -and $now.Id -ne $old) -or (Get-Date) -gt $deadline)
    if (-not $now -or $now.Id -eq $old) { throw 'kiln-caddy was not restarted' }
}
Check 'Kiln''s sites answer after that restart' {
    $deadline = (Get-Date).AddSeconds(30)
    do { $page = Get 'http://static-site.test:8080/'; if ($page.Contains('static from kiln')) { break }; Start-Sleep 2 }
    until ((Get-Date) -gt $deadline)
    Expect 'static from kiln' $page
}
Check 'daemon status counts the restart' { Expect 'Restarts by the daemon: 1' (Kiln daemon status) }
Check 'daemon call web.status (the API)' { Expect '"running"' (Kiln daemon call web.status) }
function IntermediateSha { @((Kiln ca info) -split "`n" | Where-Object { $_ -match 'SHA-256' })[1] }
Check 'daemon status shows the intermediate certificate''s days left (P2-03)' {
    Expect 'Intermediate certificate:' (Kiln daemon status)
}
Check 'renewal through the daemon: a new intermediate, the server restarts to use it, HTTPS keeps working (P2-03)' {
    $before = IntermediateSha
    $oldPid = (Get-Process kiln-caddy).Id
    # Windows PowerShell drops bare double quotes in arguments to programs, hence \".
    try { Expect '"ok": true' (Kiln daemon call ca.renew '{\"force\":true}') }
    catch {
        # Keep what uninstall would delete, to see why the restart failed.
        Get-Process kiln-caddy -ErrorAction SilentlyContinue | Format-Table Id, StartTime | Out-String | Say
        Get-Content "$env:LOCALAPPDATA\Kiln\caddy\caddy.log" -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { Say "     caddy: $_" }
        throw
    }
    if ((IntermediateSha) -eq $before) { throw "the intermediate didn't change: $before" }
    if ((Get-Process kiln-caddy).Id -eq $oldPid) { throw "the web server didn't restart to use it" }
    # Caddy issues the sites' certificates from the new intermediate in the background right
    # after the restart; a handshake in the first second or two can fail.
    $deadline = (Get-Date).AddSeconds(20)
    do { $page = Get (SiteUrl 'php-app.test'); if ($page.Contains('via cgi-fcgi')) { break }; Start-Sleep 1 }
    until ((Get-Date) -gt $deadline)
    Expect 'via cgi-fcgi' $page
}
Check 'daemon status counts the renewal' { Expect 'Renewals by the daemon: 1' (Kiln daemon status) }
Check 'daemon stop leaves the web server running' {
    Kiln daemon stop
    Expect 'static from kiln' (Get 'http://static-site.test:8080/')
}
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
function RunValue { (Get-ItemProperty $runKey -Name Kiln -ErrorAction SilentlyContinue).Kiln }
Check 'daemon enable: a Run value starts kilnd at login (P2-02)' {
    Kiln daemon enable --yes
    Expect 'kilnd.exe' "$(RunValue)"
    Expect 'Starts at login: yes' (Kiln daemon status)
}
Check 'daemon disable removes the Run value' {
    Kiln daemon disable --yes
    if (RunValue) { throw "Run value still there: $(RunValue)" }
}
# On again, and running, so uninstall has to stop it and remove the Run value.
Check 'daemon enable (again, for uninstall)' { Kiln daemon enable --yes }

Say '== uninstall'
Status 'waiting-untrust-dialog'
$answerer = Start-Answerer 'Root Certificate Store'
Check 'kiln uninstall (UAC by the test VM setting; the certificate dialog with the VM''s mouse)' { Kiln uninstall --yes }
Say "     dialog: $(Receive-Job $answerer -Wait -AutoRemoveJob)"
Status 'running'
Start-Sleep -Seconds 2
& (Join-Path $dir 'snapshot.ps1') | Set-Content (Join-Path $dir 'after.txt') -Encoding utf8
Check 'Herd''s site still answers after the uninstall' { Expect 'herd check' (Get 'http://herdcheck.test/') }

Say '== compare (ADR 0024, section 2)'   # ASCII: Windows PowerShell reads this file as ANSI
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
