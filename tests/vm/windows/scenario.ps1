# Kiln's Windows integration scenario (ADR 0024 §1, §4-5; ADR 0034): snapshot, then a full
# Kiln session next to a running Laravel Herd (including Herd restarting), up to
# `kiln daemon enable`. Then it asks run.ps1 for a reboot; after-reboot.ps1 checks what came
# back, uninstalls, snapshots again and compares.
# Runs inside the Windows test VM, in the logged-on test user's desktop session (run.ps1 starts
# it as a scheduled task), not elevated: UAC elevates Kiln's helper without asking there
# (setup-test-vm.ps1). Results go to C:\kiln-test\scenario.log; C:\kiln-test\status tells the
# host what's happening ("waiting-trust-dialog" while Windows' certificate dialog is open,
# "reboot" at the end of this part).
param([string] $Kiln = 'C:\kiln-test\bin\kiln.exe')
$ErrorActionPreference = 'Continue'
$dir = 'C:\kiln-test'
$log = Join-Path $dir 'scenario.log'
$status = Join-Path $dir 'status'
Set-Content $log '' -Encoding utf8
Remove-Item (Join-Path $dir 'failed'), (Join-Path $dir 'started.txt') -ErrorAction SilentlyContinue
$script:failed = $false
. (Join-Path $dir 'lib.ps1')

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
Set-Content "$sites\toml-app\kiln.toml" "[site]`naliases = [`"toml-alias`"]`n[php]`nversion = `"8.3`"`n[node]`nversion = `"20`"`n"
# Node (P2-05, ADR 0028): php-app asks for an LTS line by name in .nvmrc.
Set-Content "$sites\php-app\.nvmrc" "lts/iron"
# PHP updates (P2-06, ADR 0029): a site on the default PHP that shows which patch serves it.
New-Item -ItemType Directory -Force "$sites\ver-app" | Out-Null
Set-Content "$sites\ver-app\index.php" '<?php echo "ver ", PHP_VERSION, " xsl=", extension_loaded("xsl") ? "yes" : "no", " curl=", extension_loaded("curl") ? "yes" : "no";'
$herdApp = Join-Path $dir 'herd-app'
Remove-Item -Recurse -Force $herdApp -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $herdApp | Out-Null
Set-Content "$herdApp\index.php" '<?php echo "herd-import ", PHP_VERSION;'
Set-Content "$herdApp\herd.yml" "name: herd-demo`nphp: '8.3'`nsecured: true`naliases:`n  - herd-alias`n"

# The previous 8.4 patch, pinned in Kiln (ADR 0029), so `kiln php update` has something to do.
Check 'php install 8.4.26 (the previous patch, from windows.php.net''s archives, pinned in Kiln)' {
    Expect 'Verified and installed PHP 8.4.26' (Kiln php install 8.4.26)
}
Check 'php install 8.3 (a second version, for per-site PHP)' { Kiln php install 8.3 }
Check 'composer install' { Kiln composer install }
# Herd's Node (nvm) is on the machine PATH, which Windows puts before the user's: Kiln says that
# `node` stays Herd's in new terminals (ADR 0028 §6).
Check 'shims install (it says Herd''s Node on the machine PATH stays first)' {
    Expect 'machine PATH' (Kiln shims install --yes)
}
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
Check 'ca init refuses a TLD that isn''t reserved, and another TLD over the .test root (P2-10)' {
    $out = & $Kiln ca init --tld lan 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0) { throw "expected a refusal:`n$out" }
    Expect 'reserved development TLD' $out
    $out = & $Kiln ca init --tld internal 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0) { throw "expected a refusal:`n$out" }
    Expect 'only vouch for names under .test' $out
    $global:LASTEXITCODE = 0
}
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

Say '== kiln new (P2-08, ADR 0031)'
# Outside the parked folder, so `kiln new` links it itself. Composer's cache from the first
# Laravel project makes this one quick.
$newDir = Join-Path $dir 'new-projects'
New-Item -ItemType Directory -Force $newDir | Out-Null
Check 'kiln new shop --secure --php 8.3: a Laravel app over HTTPS, pinned to PHP 8.3' {
    # One retry: a dropped download is the network, not Kiln.
    # Output goes to files and the wait is bounded, so a stuck run says where it stopped.
    foreach ($attempt in 1, 2) {
        Remove-Item -Recurse -Force "$newDir\shop" -ErrorAction SilentlyContinue
        $p = Start-Process $Kiln -ArgumentList 'new', 'shop', '--path', $newDir, '--secure', '--php', '8.3' `
            -RedirectStandardOutput "$dir\new.out" -RedirectStandardError "$dir\new.err" -NoNewWindow -PassThru
        $null = $p.Handle   # keeps ExitCode readable after the exit (Windows PowerShell 5.1)
        if (-not $p.WaitForExit(20 * 60 * 1000)) {
            Say '     kiln new is still running after 20 minutes; its output so far:'
            Get-Content "$dir\new.out", "$dir\new.err" | ForEach-Object { Say "       $_" }
            Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'kiln|php|composer|caddy|curl' } |
                ForEach-Object { Say "       $($_.ProcessId) <- $($_.ParentProcessId): $($_.CommandLine)" }
            Stop-Process -Id $p.Id -Force
            throw 'kiln new hung'
        }
        $code = $p.ExitCode
        $out = (Get-Content "$dir\new.out", "$dir\new.err" -Raw) -join ''
        if ($code -eq 0) { break }
    }
    if ($code -ne 0) { throw "kiln new failed ($code):`n$out" }
    $url = [regex]::Match($out, 'https://shop\.test[^\s]*').Value
    if (-not $url) { throw "no HTTPS URL in:`n$out" }
    Kiln hosts sync --yes | Out-Null
    Expect 'Laravel' (Get $url)
    Push-Location "$newDir\shop"
    try {
        $which = Kiln which
        Expect 'kiln.toml' $which
        Expect '8.3.' $which
    } finally { Pop-Location }
}
Check 'kiln new refuses a folder that already exists' {
    $out = & $Kiln new shop --path $newDir 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0) { throw "expected a refusal:`n$out" }
    Expect 'already exists' $out
    $global:LASTEXITCODE = 0
}

Say '== PHP extensions (P2-07, ADR 0030)'
$ini = Join-Path $env:LOCALAPPDATA 'Kiln\php\8.4.26\php.ini'
$iniHash = (Get-FileHash $ini).Hash
$xslOp = ''
function ShimPhpM($folder) {
    Push-Location $folder
    try { & (Join-Path $env:LOCALAPPDATA 'Kiln\bin\shims\php.exe') -m 2>&1 | Out-String } finally { Pop-Location }
}
Check 'php ext list shows xsl off, curl on and the built-in ones' {
    $out = Kiln php ext list
    if ($out -notmatch '(?m)^\s+xsl\s+off') { throw "xsl isn't listed as off:`n$out" }
    if ($out -notmatch '(?m)^\s+curl\s+on') { throw "curl isn't listed as on:`n$out" }
    Expect 'Built in, always on' $out
}
Check 'php ext on xsl: journaled, and live in the site and the php shim' {
    $out = Kiln php ext on xsl
    $script:xslOp = [regex]::Match($out, 'kiln undo (\d+)').Groups[1].Value
    if (-not $script:xslOp) { throw "no operation number in:`n$out" }
    Expect 'xsl=yes' (Get (SiteUrl 'ver-app.test'))
    if ((ShimPhpM "$sites\ver-app") -notmatch '(?m)^xsl\s*$') { throw 'the php shim has no xsl' }
}
Check 'php ext off curl takes it out of the site; kiln undo puts it back' {
    Expect 'curl is now off' (Kiln php ext off curl)
    Expect 'curl=no' (Get (SiteUrl 'ver-app.test'))
    Expect 'PHP 8.4.26: curl turned off' (Kiln history)
    Expect 'Undone' (Kiln undo --yes)
    Expect 'curl=yes' (Get (SiteUrl 'ver-app.test'))
}
Check 'an unknown or built-in extension is refused' {
    $out = & $Kiln php ext on nosuchext 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0) { throw "expected a refusal:`n$out" }
    Expect 'no extension `nosuchext`' $out
    $out = & $Kiln php ext off date 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0) { throw "expected a refusal:`n$out" }
    Expect 'built into PHP 8.4.26' $out
    $global:LASTEXITCODE = 0
}

Say '== PHP update and rollback (P2-06, ADR 0029)'
$phpRoot = Join-Path $env:LOCALAPPDATA 'Kiln\php'
$newest = ''
Check 'before: ver-app runs the default PHP, 8.4.26' { Expect 'ver 8.4.26' (Get (SiteUrl 'ver-app.test')) }
Check 'php update 8.4 installs the newest patch beside 8.4.26 and switches to it live' {
    $out = Kiln php update 8.4
    # `8.4.26 → 8.4.27`, read loosely: the arrow may not survive the console's code page.
    $script:newest = [regex]::Match($out, '8\.4\.26\s+\S+\s+(8\.4\.\d+)').Groups[1].Value
    if (-not $script:newest) { throw "no new patch named in:`n$out" }
    Expect 'kept for `kiln php rollback 8.4`' $out
    Expect 'Extensions carried over from PHP 8.4.26: xsl on' $out
    Expect "ver $script:newest xsl=yes" (Get (SiteUrl 'ver-app.test'))
    if (-not (Test-Path "$phpRoot\8.4.26\php.exe")) { throw '8.4.26 was not kept' }
}
Check 'php rollback goes back to 8.4.26 live and removes the newer patch' {
    Expect 'Rolled back to PHP 8.4.26' (Kiln php rollback)
    Expect 'ver 8.4.26' (Get (SiteUrl 'ver-app.test'))
    if ((Kiln php list).Contains($script:newest)) { throw "$script:newest is still listed" }
    if (Test-Path "$phpRoot\$script:newest\php.exe") {
        if (-not (Test-Path "$phpRoot\$script:newest\.kiln-install-incomplete")) { throw "$script:newest is still in use" }
    }
}
Check 'a second rollback has nothing to do' {
    $out = & $Kiln php rollback 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0) { throw "expected a refusal:`n$out" }
    Expect 'nothing to roll back' $out
    $global:LASTEXITCODE = 0
}
Check 'after the rollback 8.4.26 still has xsl; undoing it restores php.ini byte for byte' {
    Expect 'ver 8.4.26 xsl=yes' (Get (SiteUrl 'ver-app.test'))
    if ((Kiln history).Contains("PHP $($script:newest): xsl")) { throw 'the removed patch''s toggle is still in effect' }
    Expect 'Undone' (Kiln undo $script:xslOp --yes)
    Expect 'xsl=no' (Get (SiteUrl 'ver-app.test'))
    if ((Get-FileHash $ini).Hash -ne $iniHash) { throw 'php.ini differs from before the toggles' }
}

Say '== Node (P2-05, ADR 0028)'
# Kiln's shims are run by their full path: in new terminals Herd's Node comes first (above).
# npm's cache goes under C:\kiln-test, so the run leaves nothing of npm's own behind either.
$env:npm_config_cache = Join-Path $dir 'npm-cache'
$shims = Join-Path $env:LOCALAPPDATA 'Kiln\bin\shims'
function ShimNode($folder) {
    Push-Location $folder
    try { & "$shims\node.exe" -v 2>&1 | Out-String } finally { Pop-Location }
}
Check 'node install 22 and lts/iron (signature-checked SHASUMS)' {
    Expect 'Checked SHASUMS256.txt''s signature' (Kiln node install 22)
    Expect 'Node 20.' (Kiln node install lts/iron)
}
Check 'the node shim follows kiln.toml (20), .nvmrc (lts/iron) and the default elsewhere (22)' {
    Expect 'v20.' (ShimNode "$sites\toml-app")
    Expect 'v20.' (ShimNode "$sites\php-app")
    Expect 'v22.' (ShimNode "$sites\laravel-app")
}
# npm prints the versions of the Node it runs on (ADR 0034 §3).
function ShimNpmNode($folder) {
    Push-Location $folder
    try { [regex]::Match((& "$shims\npm.exe" version --json 2>&1 | Out-String), '"node":\s*"([^"]+)"').Groups[1].Value }
    finally { Pop-Location }
}
Check 'the npm shim runs on the project''s Node: 20 in toml-app (kiln.toml), 22 in laravel-app' {
    $toml, $laravel = (ShimNpmNode "$sites\toml-app"), (ShimNpmNode "$sites\laravel-app")
    if (-not $toml.StartsWith('20.') -or -not $laravel.StartsWith('22.')) {
        throw "npm ran on Node '$toml' in toml-app and '$laravel' in laravel-app"
    }
}
Check 'kiln which names the .nvmrc' {
    Push-Location "$sites\php-app"
    try { Expect '.nvmrc asks for lts/iron' (Kiln which) } finally { Pop-Location }
}
Check 'npm install and npm run build in the Laravel app, with Kiln''s Node' {
    Push-Location "$sites\laravel-app"
    try {
        # One retry: a dropped download is the network, not Kiln.
        foreach ($attempt in 1, 2) {
            $out = & "$shims\npm.exe" install --no-audit --no-fund 2>&1 | Out-String
            if ($LASTEXITCODE -eq 0) { break }
        }
        if ($LASTEXITCODE -ne 0) { throw "npm install failed ($LASTEXITCODE):`n$out" }
        # The build downloads the skeleton's fonts (laravel-vite-plugin/fonts): one retry too.
        # Node gives each address only 250 ms to connect, less than Bunny Fonts can take (I-70).
        $env:NODE_OPTIONS = '--network-family-autoselection-attempt-timeout=10000'
        foreach ($attempt in 1, 2) {
            $out = & "$shims\npm.exe" run build 2>&1 | Out-String
            if ($LASTEXITCODE -eq 0) { break }
            Say "     npm run build failed (attempt $attempt): $(($out -split "`n" | Select-String 'error|fetch|ENOTFOUND|ECONN' | Select-Object -First 4) -join ' | ')"
        }
        Remove-Item Env:NODE_OPTIONS
        if ($LASTEXITCODE -ne 0) { throw "npm run build failed ($LASTEXITCODE):`n$out" }
        if (-not (Test-Path 'public\build\manifest.json')) { throw "no public\build\manifest.json:`n$out" }
    } finally { Pop-Location }
}
Check 'Herd''s Node keeps its place: a new terminal''s node is still Herd''s' {
    $herd = & 'C:\Program Files\nodejs\node.exe' -v | Out-String
    # Windows' order: the machine PATH, then the user's.
    $new = powershell -NoProfile -Command "`$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User'); node -v" | Out-String
    Expect $herd.Trim() $new
}
Check 'kiln doctor says the machine-wide Node comes first' {
    Expect 'comes before Kiln''s shim' (& $Kiln doctor 2>&1 | Out-String)
}

Say '== Proxy sites (P2-09, ADR 0032)'
# Laravel's own Vite dev server, started with Kiln's npm, reached as vite.test over HTTPS.
$vite = $null
$viteUrl = ''
Check 'npm run dev starts Vite on localhost:5173' {
    $script:vite = Start-Process -FilePath "$shims\npm.exe" -ArgumentList 'run', 'dev' -WorkingDirectory "$sites\laravel-app" `
        -WindowStyle Hidden -PassThru -RedirectStandardOutput "$dir\vite.log" -RedirectStandardError "$dir\vite-err.log"
    $deadline = (Get-Date).AddMinutes(1)
    do {
        Start-Sleep -Seconds 2
        $client = curl.exe -s http://localhost:5173/@vite/client 2>&1 | Out-String
    } until ($client.Contains('vite') -or (Get-Date) -gt $deadline)
    if (-not $client.Contains('vite')) { throw "Vite didn't answer:`n$(Get-Content "$dir\vite.log", "$dir\vite-err.log" -Raw)" }
}
Check 'kiln proxy vite 5173 --secure: Vite''s HMR client over HTTPS at vite.test' {
    $out = Kiln proxy vite 5173 --secure
    $script:viteUrl = [regex]::Match($out, 'https://vite\.test[^\s]*').Value
    if (-not $script:viteUrl) { throw "no HTTPS URL in:`n$out" }
    Kiln hosts sync --yes | Out-Null
    # Vite answers only with its own Host (Vite 6+ blocks others), which Kiln sets.
    $client = Get "$($script:viteUrl)@vite/client"
    Expect 'createHotContext' $client
    Expect 'http://localhost:5173' (Kiln sites)
}
Check 'kiln unproxy vite: the name is no longer served' {
    Expect 'no longer proxies' (Kiln unproxy vite)
    Expect 'No Kiln site is linked' (Get "http://vite.test:8080/")
}
if ($vite) { taskkill /T /F /PID $vite.Id 2>&1 | Out-Null }
Remove-Item "$sites\laravel-app\public\hot" -ErrorAction SilentlyContinue

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
# Times the restart (ADR 0034 §2): asks for the static site over HTTP every 50 ms, each on a new
# connection, and returns the longest time between two answers, in ms.
function Start-Poller($url) {
    $stop = Join-Path $dir 'poll.stop'
    Remove-Item $stop, "$stop.ready" -ErrorAction SilentlyContinue
    $job = Start-Job -ArgumentList $url, $stop {
        param($url, $stop)
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $last = $null; $worst = 0
        while (-not (Test-Path $stop)) {
            try {
                $request = [Net.WebRequest]::Create($url)
                $request.Timeout = 2000
                $request.KeepAlive = $false
                $request.GetResponse().Close()
                $now = $clock.ElapsedMilliseconds
                if ($null -eq $last) { Set-Content "$stop.ready" 'yes' } else { $worst = [Math]::Max($worst, $now - $last) }
                $last = $now
            } catch {}
            Start-Sleep -Milliseconds 50
        }
        if ($null -eq $last) { -1 } else { [Math]::Max($worst, $clock.ElapsedMilliseconds - $last) }
    }
    $deadline = (Get-Date).AddSeconds(30)
    while (-not (Test-Path "$stop.ready") -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
    $job
}
function Stop-Poller($job) {
    Set-Content (Join-Path $dir 'poll.stop') 'yes'
    $gap = Receive-Job $job -Wait -AutoRemoveJob
    Remove-Item (Join-Path $dir 'poll.stop*') -ErrorAction SilentlyContinue
    $gap
}
$script:renewalGap = $null
Check 'renewal through the daemon: a new intermediate, the server restarts to use it, HTTPS keeps working (P2-03)' {
    $before = IntermediateSha
    $oldPid = (Get-Process kiln-caddy).Id
    $poller = Start-Poller 'http://static-site.test:8080/'
    # Windows PowerShell drops bare double quotes in arguments to programs, hence \".
    try {
        Expect '"ok": true' (Kiln daemon call ca.renew '{\"force\":true}')
        Start-Sleep -Seconds 2
        $script:renewalGap = Stop-Poller $poller
    }
    catch {
        Stop-Poller $poller | Out-Null
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
Check 'the renewal''s restart left the sites unanswered for under a second (ADR 0034)' {
    Say "     longest time without an answer: $script:renewalGap ms"
    if ($null -eq $script:renewalGap -or $script:renewalGap -lt 0 -or $script:renewalGap -ge 1000) {
        throw "longest time without an answer: $script:renewalGap ms"
    }
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
# On again, and left running, so the login after the reboot has to start it, and uninstall
# then has to stop it and remove the Run value.
Check 'daemon enable (again, for the reboot and uninstall)' { Kiln daemon enable --yes }

# What after-reboot.ps1 needs: the URLs Kiln printed, and whether anything failed so far.
Set-Content (Join-Path $dir 'started.txt') $script:started -Encoding utf8
if ($script:failed) { Set-Content (Join-Path $dir 'failed') 'yes' -Encoding ascii }
Say '== reboot (ADR 0034)'
Status 'reboot'

