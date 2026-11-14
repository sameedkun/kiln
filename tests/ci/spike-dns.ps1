# Spike for P4C-14 / P4D-02 (names without the hosts file), on a throwaway runner only: does
# Windows send every `.test` name to a resolver on 127.0.0.1 once one Name Resolution Policy
# rule says so, and stop when the rule goes? Nothing here runs on a developer's PC.
#   spike-dns.ps1
$ErrorActionPreference = 'Stop'
$failed = $false
function Check($name, [scriptblock] $body) {
    try { & $body; Write-Host "PASS $name" } catch { $script:failed = $true; Write-Host "FAIL $name`n     $_" }
}

# Like Internet Connection Sharing on a developer's PC (found 2026-11-15): something already
# on UDP 0.0.0.0:53 that answers nothing. Kiln's responder binds 127.0.0.1:53 beside it.
$squatter = Start-Job -ScriptBlock {
    $udp = [Net.Sockets.UdpClient]::new([Net.IPEndPoint]::new([Net.IPAddress]::Any, 53))
    Start-Sleep -Seconds 600
}
Start-Sleep -Seconds 2

# A minimal DNS responder: an A question under .test gets 127.0.0.1; any other type under
# .test, "no records" (NOERROR, empty: AAAA and HTTPS questions then fall back to A); a name
# outside .test, "no such name". It runs as a background job on UDP 127.0.0.1:53.
$responder = Start-Job -ScriptBlock {
    $udp = [Net.Sockets.UdpClient]::new([Net.IPEndPoint]::new([Net.IPAddress]::Loopback, 53))
    $from = [Net.IPEndPoint]::new([Net.IPAddress]::Any, 0)
    while ($true) {
        $q = $udp.Receive([ref] $from)
        $i = 12; $labels = @()
        while ($q[$i] -ne 0) { $n = $q[$i]; $labels += [Text.Encoding]::ASCII.GetString($q, $i + 1, $n); $i += $n + 1 }
        $qtype = $q[$i + 1] * 256 + $q[$i + 2]
        $end = $i + 5
        $name = ($labels -join '.').ToLower()
        $ours = $name -like '*.test' -or $name -eq 'test'
        $answer = $ours -and $qtype -eq 1
        $r = [Collections.Generic.List[byte]]::new()
        $r.AddRange([byte[]]$q[0..1])
        $r.Add(0x85); $r.Add($(if ($ours) { 0x80 } else { 0x83 }))
        $r.AddRange([byte[]](0, 1, 0, $(if ($answer) { 1 } else { 0 }), 0, 0, 0, 0))
        $r.AddRange([byte[]]$q[12..($end - 1)])
        if ($answer) { $r.AddRange([byte[]](0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 127, 0, 0, 1)) }
        [void]$udp.Send($r.ToArray(), $r.Count, $from)
    }
}
Start-Sleep -Seconds 2
Write-Host "squatter: $((Get-Job $squatter.Id).State); responder: $((Get-Job $responder.Id).State)"

Check 'before the rule: a .test name does not resolve' {
    $r = Resolve-DnsName 'kiln-spike.test' -Type A -ErrorAction SilentlyContinue
    if ($r | Where-Object { $_.IPAddress -eq '127.0.0.1' }) { throw 'it resolved already' }
}

# A name in the hosts file, to see it still wins with the rule on and the responder down.
$hostsFile = "$env:SystemRoot\System32\drivers\etc\hosts"
$hostsBefore = [IO.File]::ReadAllText($hostsFile)
[IO.File]::AppendAllText($hostsFile, "`r`n127.0.0.1 kiln-hosts.test # kiln spike`r`n")

Add-DnsClientNrptRule -Namespace '.test' -NameServers '127.0.0.1' -Comment 'Kiln spike (P4C-14)'
Clear-DnsClientCache
try {
    Check 'with the rule (and a wildcard listener on :53 beside it): Resolve-DnsName asks the responder' {
        $r = Resolve-DnsName 'kiln-spike.test' -Type A -DnsOnly
        if (-not ($r | Where-Object { $_.IPAddress -eq '127.0.0.1' })) { throw "answer: $($r | Out-String)" }
    }
    Check 'with the rule: getaddrinfo (.NET) resolves any name under .test' {
        $ips = [Net.Dns]::GetHostAddresses('shop.kiln-spike.test') | ForEach-Object { $_.ToString() }
        if ($ips -notcontains '127.0.0.1') { throw "addresses: $($ips -join ', ')" }
    }
    Check 'AAAA questions get an empty answer, not "no such name" (type 65 is covered by unit tests: Resolve-DnsName has no HTTPS type)' {
        foreach ($type in 'AAAA') {
            $err = $null
            $null = Resolve-DnsName 'kiln-spike.test' -Type $type -DnsOnly -ErrorAction SilentlyContinue -ErrorVariable err
            if ($err -and "$err" -match 'does not exist|DNS name does not exist|NXDOMAIN') { throw "${type}: $err" }
        }
    }
    Check 'with the rule: curl reaches a local server by a .test name' {
        $listener = [Net.HttpListener]::new()
        $listener.Prefixes.Add('http://+:18080/')
        $listener.Start()
        $serve = $listener.GetContextAsync()
        $job = Start-Job { curl.exe -sS --max-time 10 'http://anything.test:18080/' 2>&1 | Out-String }
        if (-not $serve.Wait(15000)) { throw "no request arrived: $(Receive-Job $job -Wait)" }
        $ctx = $serve.Result
        $bytes = [Text.Encoding]::UTF8.GetBytes('kiln-spike-ok')
        $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length); $ctx.Response.Close()
        $out = Receive-Job $job -Wait
        $listener.Stop()
        if ($out -notmatch 'kiln-spike-ok') { throw "curl: $out" }
    }
    Check 'with the rule: Edge opens a .test name (its own resolver, secure DNS and all)' {
        $edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
        if (-not $edge) { throw 'no Edge on this runner' }
        $listener = [Net.HttpListener]::new()
        $listener.Prefixes.Add('http://+:18081/')
        $listener.Start()
        $serve = $listener.GetContextAsync()
        $profile = Join-Path $env:RUNNER_TEMP 'edge-spike'
        $job = Start-Job -ArgumentList $edge, $profile { param($edge, $profile) & $edge --headless=new --disable-gpu --no-first-run "--user-data-dir=$profile" --dump-dom 'http://edge-check.test:18081/' 2>&1 | Out-String }
        if (-not $serve.Wait(45000)) { throw "Edge never asked: $(Receive-Job $job -Wait)" }
        $ctx = $serve.Result
        $bytes = [Text.Encoding]::UTF8.GetBytes('<html><body>kiln-edge-ok</body></html>')
        $ctx.Response.ContentType = 'text/html'
        $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length); $ctx.Response.Close()
        $out = Receive-Job $job -Wait
        $listener.Stop()
        if ($out -notmatch 'kiln-edge-ok') { throw "Edge: $out" }
    }
    Check 'other names still go to the usual DNS' {
        if (-not [Net.Dns]::GetHostAddresses('github.com')) { throw 'github.com did not resolve' }
    }
    Check 'with the rule and the responder down: a hosts-file name still resolves; others under .test fail (timed)' {
        Stop-Job $responder
        Clear-DnsClientCache
        $ips = [Net.Dns]::GetHostAddresses('kiln-hosts.test') | ForEach-Object { $_.ToString() }
        if ($ips -notcontains '127.0.0.1') { throw "hosts name: $($ips -join ', ')" }
        $start = Get-Date
        try { [void][Net.Dns]::GetHostAddresses('gone.test') } catch { }
        Write-Host "     a .test name with no responder failed after $([int]((Get-Date) - $start).TotalMilliseconds) ms"
    }
} finally {
    Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'Kiln spike (P4C-14)' } | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
    [IO.File]::WriteAllText($hostsFile, $hostsBefore)
    Clear-DnsClientCache
}
Check 'after removing the rule: .test names stop resolving' {
    $r = Resolve-DnsName 'kiln-spike.test' -Type A -ErrorAction SilentlyContinue
    if ($r | Where-Object { $_.IPAddress -eq '127.0.0.1' }) { throw 'still resolves' }
}
Stop-Job $responder, $squatter -ErrorAction SilentlyContinue; Remove-Job $responder, $squatter -Force
if ($failed) { Write-Host '== FAILED'; exit 1 }
Write-Host '== PASSED'
