# The search engines on a clean runner (ADR 0077): OpenSearch and Elasticsearch, each
# downloaded and checked against the vendor's own SHA-512 too, started on loopback, a
# document indexed and found, then removed with their data. Separate from the services
# Scenario: their downloads are 1.7 GB together.
#   windows-search.ps1 -Kiln <path to kiln.exe>
param([Parameter(Mandatory)] [string] $Kiln)
$ErrorActionPreference = 'Continue'
$dir = Join-Path $env:RUNNER_TEMP 'kiln-search'
New-Item -ItemType Directory -Force $dir | Out-Null
$script:failed = $false

function Kiln {
    $out = & $Kiln @args 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw "kiln $args failed ($LASTEXITCODE): $($out.Trim())" }
    $out
}
function Check($name, [scriptblock] $body) {
    $start = Get-Date
    try { & $body; Write-Host "PASS $name ($([int]((Get-Date) - $start).TotalSeconds) s)" }
    catch { $script:failed = $true; Write-Host "FAIL $name ($([int]((Get-Date) - $start).TotalSeconds) s)`n     $_" }
}
function Http($method, $url, $body) {
    $args = @('-sS', '--max-time', '30', '-X', $method, $url)
    if ($body) { $args += @('-H', 'Content-Type: application/json', '-d', $body) }
    curl.exe @args 2>&1 | Out-String
}

$engines = @(
    @{ kind = 'opensearch'; name = 'opensearch-3'; url = 'https://artifacts.opensearch.org/releases/bundle/opensearch/3.9.0/opensearch-3.9.0-windows-x64.zip'; archive = 'opensearch-3.9.0-windows-x64.zip' },
    @{ kind = 'elasticsearch'; name = 'elasticsearch-9'; url = 'https://artifacts.elastic.co/downloads/elasticsearch/elasticsearch-9.5.6-windows-x86_64.zip'; archive = 'elasticsearch-9.5.6-windows-x86_64.zip' }
)
foreach ($e in $engines) {
    Check "$($e.kind): the pinned download is the one the vendor's SHA-512 names" {
        $published = (curl.exe -sSL --max-time 60 "$($e.url).sha512" | Out-String).Split(' ')[0].Trim()
        $cached = Get-ChildItem "$env:LOCALAPPDATA\Kiln" -Recurse -Filter $e.archive -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $cached) {
            # Not downloaded yet: `service add` below fetches it (checked by its SHA-256 pin).
            $file = Join-Path $dir $e.archive
            curl.exe -sSL --max-time 900 -o $file $e.url
            $cached = Get-Item $file
        }
        $actual = (Get-FileHash $cached.FullName -Algorithm SHA512).Hash.ToLower()
        if ($actual -ne $published) { throw "SHA-512 $actual, published $published" }
    }
    Check "$($e.kind): added, answers on loopback only, indexes and finds a document" {
        $out = Kiln service add $e.kind
        if ($out -notmatch "running as $($e.name)") { throw $out }
        $port = [int][regex]::Match($out, 'http 127\.0\.0\.1:(\d+)').Groups[1].Value
        $base = "http://127.0.0.1:$port"
        $deadline = (Get-Date).AddSeconds(120)
        do { Start-Sleep -Seconds 2; $health = Http GET "$base/_cluster/health" } until ($health -match '"status"' -or (Get-Date) -gt $deadline)
        if ($health -notmatch '"status":"(green|yellow)"') { throw "health: $health`n$(Kiln service logs $e.name)" }
        $listening = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.OwningProcess -in (Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like "*\Kiln\services\bin\$($e.kind)\*" }).ProcessId })
        $beyond = @($listening | Where-Object { $_.LocalAddress -notin '127.0.0.1', '::1' })
        if ($beyond.Length) { throw "listens beyond loopback: $(($beyond | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) -join ', ')" }
        Http PUT "$base/kiln/_doc/1?refresh=true" '{"title":"Kiln serves PHP sites"}' | Out-Null
        $found = Http POST "$base/kiln/_search" '{"query":{"match":{"title":"sites"}}}'
        if ($found -notmatch 'Kiln serves PHP sites') { throw "search: $found" }
    }
    Check "$($e.kind): removed with its data, its JVMs gone" {
        Kiln service remove $e.name --delete-data | Out-Null
        Start-Sleep -Seconds 3
        $left = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like "*\Kiln\services\bin\$($e.kind)\*" })
        if ($left.Length) { throw "still running: $($left.ExecutablePath -join ', ')" }
    }
}
& $Kiln uninstall --delete-data --yes 2>&1 | Out-Null
if ($script:failed) { Write-Host '== FAILED'; exit 1 }
Write-Host '== PASSED'
