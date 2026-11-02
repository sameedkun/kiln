# Prints everything Kiln may touch on this Windows machine, one fact per line, for comparing
# before and after a Kiln session (ADR 0024 §2), plus Laravel Herd's state (§4).
$ErrorActionPreference = 'Continue'

function Hash($path) {
    if (Test-Path -LiteralPath $path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash } else { 'absent' }
}

$hosts = Join-Path ([Environment]::SystemDirectory) 'drivers\etc\hosts'
"file hosts $(Hash $hosts)"
# Herd's own block in the hosts file, separately, so a change there is named as Herd's.
$text = if (Test-Path $hosts) { [IO.File]::ReadAllText($hosts) } else { '' }
$herdBlock = [regex]::Match($text, '(?s)# Herd generated Hosts.*?# End Herd generated Hosts').Value
"herd hosts block $([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($herdBlock))).Replace('-', '')) ($($herdBlock.Length) chars)"

$env_key = Get-Item 'HKCU:\Environment'
"user PATH ($($env_key.GetValueKind('Path'))): $($env_key.GetValue('Path', '', 'DoNotExpandEnvironmentNames'))"

# The user's own root store, where `kiln ca trust` adds its root, as stored in the registry.
# Not Cert:\CurrentUser\Root: that view includes the machine's roots, which Windows adds to by
# itself (automatic root updates) when a download needs one.
$own = 'HKCU:\Software\Microsoft\SystemCertificates\Root\Certificates'
$thumbprints = @(Get-ChildItem $own -ErrorAction SilentlyContinue | ForEach-Object PSChildName | Sort-Object)
"user root store: $($thumbprints.Count) certificates"
foreach ($t in $thumbprints) { "  $t" }

# Login items (P2-02): every value in the user's Run key, with what it runs.
$run = Get-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue
# Only its values matter: a fresh runner has no key, and Windows may create it empty later.
if (-not $run -or -not $run.GetValueNames()) { 'run key: no values' }
else { foreach ($name in ($run.GetValueNames() | Sort-Object)) { "run key: $name = $($run.GetValue($name))" } }

foreach ($d in @("$env:APPDATA\Kiln", "$env:LOCALAPPDATA\Kiln")) {
    if (Test-Path $d) { "dir $d present" } else { "dir $d absent" }
}
"kiln processes: $((Get-Process kiln-caddy, kiln-supervisor, kilnd, kiln-mailpit, kiln-mysqld, kiln-mariadbd, kiln-postgres -ErrorAction SilentlyContinue | ForEach-Object Name | Sort-Object) -join ' ')"
# By Kiln's folders, not '*\Kiln\*': the test user is called kiln, so Herd's PHP matches that.
$kilnPhp = Get-Process php-cgi -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -like "$env:LOCALAPPDATA\Kiln\*" -or $_.Path -like "$env:APPDATA\Kiln\*" }
"kiln php-cgi: $(@($kilnPhp).Count)"

# Laravel Herd: its configuration, its sites, and that its server still listens.
$herd = Join-Path $env:USERPROFILE '.config\herd\config'
foreach ($f in 'config.json', 'php.json', 'valet\config.json', 'nginx\nginx.conf', 'nginx\herd.conf') {
    "herd $f $(Hash (Join-Path $herd $f))"
}
"herd nginx listening on 80: $([bool](Get-NetTCPConnection -LocalPort 80 -State Listen -ErrorAction SilentlyContinue))"
