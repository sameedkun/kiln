# The by-hand part of Phase 3's exit run (P3-12, ADR 0039 amendment): Kiln next to a running
# Laravel Herd on a developer's own machine, which CI's runners can't have.
#
#   powershell -ExecutionPolicy Bypass -File tests\manual\herd-check.ps1 [-Kiln <kiln.exe>] [-HerdSite http://app.test]
#
# Run it in a normal (not administrator) terminal, with Herd running. It:
#   1. snapshots what Kiln may touch and Herd's state (config, its server on port 80, and
#      every port Herd's programs listen on);
#   2. sets Kiln up: PHP 8.4, its CA (Windows asks you to trust it: click Yes), an HTTPS
#      test site, and two services (Mailpit, Redis);
#   3. checks Kiln took other ports than Herd's and said why, that the site answers over
#      HTTPS with the trusted certificate, and that a Herd site still answers as before;
#   4. uninstalls Kiln with --delete-data (Windows asks to remove the certificate: click Yes);
#   5. snapshots again and compares: everything must be as it was.
# The hosts file is never changed: the checks resolve the test names themselves.
# Everything is written to %TEMP%\kiln-herd-check\check.log; paste its summary back.
param(
    [string] $Kiln = (Join-Path $PSScriptRoot '..\..\target\release\kiln.exe'),
    # One of your Herd sites, to check it answers the same before and after.
    [string] $HerdSite = ''
)
$ErrorActionPreference = 'Continue'
$Kiln = (Resolve-Path $Kiln -ErrorAction Stop).Path
$dir = Join-Path $env:TEMP 'kiln-herd-check'
$log = Join-Path $dir 'check.log'
$status = Join-Path $dir 'status'
$vm = Join-Path $PSScriptRoot '..\vm\windows'
New-Item -ItemType Directory -Force $dir | Out-Null
Set-Content $log '' -Encoding utf8
$script:failed = $false
. (Join-Path $vm 'lib.ps1')
$sayToFile = ${function:Say}
function Say($text) { & $sayToFile $text; Write-Host $text }

# Refuse to start over an existing Kiln: this checks a clean machine against itself.
foreach ($folder in "$env:LOCALAPPDATA\Kiln", "$env:APPDATA\Kiln") {
    if (Test-Path $folder) {
        Say "STOP: $folder exists, so Kiln is already set up here. Run ``kiln uninstall`` first (or keep it, and don't run this check)."
        exit 2
    }
}

# Herd's listening ports and their programs, by name, sorted: the same before and after.
function HerdPorts {
    Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | ForEach-Object {
        $process = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue
        if ($process.Path -like '*Herd*') { "$($process.ProcessName) $($_.LocalAddress):$($_.LocalPort)" }
    } | Sort-Object -Unique
}
# A Herd site's HTTP status code, or 'none' without one.
function HerdStatus {
    if (-not $HerdSite) { return 'none' }
    curl.exe -s -o NUL --max-time 30 -w '%{http_code}' $HerdSite 2>$null
}

Say "== Kiln next to Laravel Herd: $(Get-Date -Format 'yyyy-MM-dd HH:mm') on $env:COMPUTERNAME"
Say "     kiln: $Kiln ($(& $Kiln --version 2>&1))"
& (Join-Path $vm 'snapshot.ps1') | Set-Content (Join-Path $dir 'before.txt') -Encoding utf8
$herdPortsBefore = @(HerdPorts)
$herdSiteBefore = HerdStatus
Say "     Herd listens on: $($herdPortsBefore -join ', ')"
if (-not ($herdPortsBefore -match ':80$')) { Say 'WARN Herd''s server isn''t listening on port 80: is Herd running?' }
Say "     Herd site ($(if ($HerdSite) { $HerdSite } else { 'none given' })): $herdSiteBefore"

Say '== Set up Kiln'
Check 'php install 8.4' { Kiln php install 8.4 }
Check 'web install, ca init' { Kiln web install; Kiln ca init }
Check 'ca trust (Windows asks: click Yes)' { Kiln ca trust --yes }
$site = Join-Path $dir 'kilncheck'
New-Item -ItemType Directory -Force $site | Out-Null
Set-Content (Join-Path $site 'index.php') '<?php echo "kiln check " . PHP_VERSION;' -Encoding ascii
Check 'link kilncheck --secure' { Kiln link kilncheck --path $site --secure }
$script:started = ''
Check 'web start next to Herd: Kiln takes other ports and says why' {
    $script:started = Kiln web start
    Expect 'used by Laravel Herd' $script:started
}
Check 'the site answers over HTTPS, with a certificate Windows trusts' {
    $url = [regex]::Match((Kiln sites), 'https://kilncheck\.test[^\s]*').Value
    if (-not $url) { throw "no HTTPS URL in:`n$(Kiln sites)" }
    $uri = [Uri] $url
    # No --cacert and no -k: Windows' own trust store has to vouch for it.
    $page = curl.exe -sS --max-time 60 --ssl-revoke-best-effort --resolve "$($uri.Host):$($uri.Port):127.0.0.1" $url 2>&1 | Out-String
    Expect 'kiln check 8.4' $page
}

Say '== Services next to Herd''s'
Check 'service add mailpit' { Kiln service add mailpit }
Check 'service add redis' { Kiln service add redis }
Check 'kiln service list: both running' {
    $list = Kiln service list
    if (($list -split "`n" | Where-Object { $_ -match 'running' }).Count -lt 2) { throw "not both running:`n$list" }
    Say ($list.Trim() -split "`n" | ForEach-Object { "     $_" }) -join "`n"
}
Check 'kiln doctor names Herd and Kiln''s own ports' {
    $out = (& $Kiln doctor 2>&1 | ForEach-Object { "$_" }) -join "`n"
    Expect 'Laravel Herd' $out
    $global:LASTEXITCODE = 0
}
Check 'Herd still listens on the same ports with the same programs' {
    $now = @(HerdPorts)
    $diff = Compare-Object $herdPortsBefore $now
    if ($diff) { throw "Herd's ports changed:`n$($diff | ForEach-Object { "$($_.SideIndicator) $($_.InputObject)" } | Out-String)" }
}
Check 'the Herd site answers as before' {
    $now = HerdStatus
    if ($now -ne $herdSiteBefore) { throw "it answered $herdSiteBefore before and $now now" }
}

Say '== Uninstall'
Check 'kiln uninstall --delete-data (Windows asks to remove the certificate: click Yes)' {
    Kiln uninstall --yes --delete-data
}
Start-Sleep -Seconds 2
& (Join-Path $vm 'snapshot.ps1') | Set-Content (Join-Path $dir 'after.txt') -Encoding utf8
Compare-Snapshot 'after.txt' 'after uninstall'
Check 'Herd still listens on the same ports after the uninstall' {
    $diff = Compare-Object $herdPortsBefore @(HerdPorts)
    if ($diff) { throw "Herd's ports changed:`n$($diff | ForEach-Object { "$($_.SideIndicator) $($_.InputObject)" } | Out-String)" }
}
Check 'the Herd site still answers as before' {
    $now = HerdStatus
    if ($now -ne $herdSiteBefore) { throw "it answered $herdSiteBefore before and $now now" }
}

Say $(if ($script:failed) { '== FAILED' } else { '== PASSED' })
Say "     Full log: $log"
exit $(if ($script:failed) { 1 } else { 0 })
