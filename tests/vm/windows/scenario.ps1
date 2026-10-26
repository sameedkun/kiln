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
        & $composer create-project laravel/laravel laravel-app --no-interaction --prefer-dist 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) { throw "composer create-project failed ($LASTEXITCODE)" }
    } finally { Pop-Location }
}
Check 'WordPress, from wordpress.org' {
    $archive = Join-Path $dir 'wordpress.tar.gz'
    curl.exe -fsSL -o $archive https://wordpress.org/latest.tar.gz
    if ($LASTEXITCODE -ne 0) { throw "download failed ($LASTEXITCODE)" }
    tar.exe -xzf $archive -C $sites
    if ($LASTEXITCODE -ne 0) { throw "tar failed ($LASTEXITCODE)" }
    Rename-Item (Join-Path $sites 'wordpress') 'wp-site'
}
Check 'ca init' { Kiln ca init }
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
    Expect 'WordPress' (curl.exe -sSL --ssl-revoke-best-effort $url 2>&1 | Out-String)
}
Check 'kiln doctor reports Herd under Other tools' {
    $doctor = & $Kiln doctor 2>&1 | Out-String
    Expect 'Laravel Herd is installed' $doctor
}
function ShimPhp($folder) {
    powershell -NoProfile -Command "`$env:Path = [Environment]::GetEnvironmentVariable('Path','User') + ';' + [Environment]::GetEnvironmentVariable('Path','Machine'); Set-Location '$folder'; php -r 'echo PHP_VERSION;'" | Out-String
}
Check 'the php shim in a new terminal runs each site''s PHP (8.3 in php-app, 8.4 in laravel-app)' {
    Expect '8.3.' (ShimPhp "$sites\php-app")
    Expect '8.4.' (ShimPhp "$sites\laravel-app")
}

Say '== Herd restarts while Kiln runs (I-11)'
$caddy = (Get-Process kiln-caddy).Id
Check 'herd restart' { & (Join-Path $env:USERPROFILE '.config\herd\bin\herd.bat') restart 2>&1 | Out-String }
Start-Sleep -Seconds 5
Check 'Kiln''s server survived it (same PID)' { if ((Get-Process kiln-caddy).Id -ne $caddy) { throw 'kiln-caddy restarted or died' } }
Check 'Kiln''s sites still answer' { Expect 'static from kiln' (Get 'http://static-site.test:8080/') }
Check 'Herd''s site answers' { Expect 'herd check' (Get 'http://herdcheck.test/') }

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
