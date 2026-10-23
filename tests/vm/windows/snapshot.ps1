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

$roots = Get-ChildItem Cert:\CurrentUser\Root | Sort-Object Thumbprint
"CurrentUser\Root: $($roots.Count) certificates"
foreach ($c in $roots | Where-Object Subject -like '*Kiln*') { "  Kiln root: $($c.Thumbprint) $($c.Subject)" }

foreach ($d in @("$env:APPDATA\Kiln", "$env:LOCALAPPDATA\Kiln")) {
    if (Test-Path $d) { "dir $d present" } else { "dir $d absent" }
}
"kiln processes: $((Get-Process kiln-caddy, kiln-supervisor -ErrorAction SilentlyContinue | ForEach-Object Name | Sort-Object) -join ' ')"
$kilnPhp = Get-Process php-cgi -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '*\Kiln\*' }
"kiln php-cgi: $(@($kilnPhp).Count)"

# Laravel Herd: its configuration, its sites, and that its server still listens.
$herd = Join-Path $env:USERPROFILE '.config\herd'
"herd config.json $(Hash (Join-Path $herd 'config.json'))"
"herd sites.json $(Hash (Join-Path $herd 'sites.json'))"
"herd nginx listening on 80: $([bool](Get-NetTCPConnection -LocalPort 80 -State Listen -ErrorAction SilentlyContinue))"
