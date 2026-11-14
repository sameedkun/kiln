# Spike for P4C-14 (names without the hosts file), on a throwaway runner only: does Windows
# send every `.test` name to a resolver on 127.0.0.1 once one Name Resolution Policy rule
# says so, and stop when the rule goes? Nothing here runs on a developer's PC.
#   spike-dns.ps1
$ErrorActionPreference = 'Stop'
$failed = $false
function Check($name, [scriptblock] $body) {
    try { & $body; Write-Host "PASS $name" } catch { $script:failed = $true; Write-Host "FAIL $name`n     $_" }
}

# A minimal DNS responder: any A question under .test gets 127.0.0.1; anything else, "no such
# name". It runs as a background job on UDP 127.0.0.1:53.
$responder = Start-Job -ScriptBlock {
    $udp = [Net.Sockets.UdpClient]::new([Net.IPEndPoint]::new([Net.IPAddress]::Loopback, 53))
    $from = [Net.IPEndPoint]::new([Net.IPAddress]::Any, 0)
    while ($true) {
        $q = $udp.Receive([ref] $from)
        # The question's name: labels from byte 12 to a zero byte.
        $i = 12; $labels = @()
        while ($q[$i] -ne 0) { $n = $q[$i]; $labels += [Text.Encoding]::ASCII.GetString($q, $i + 1, $n); $i += $n + 1 }
        $qtype = $q[$i + 1] * 256 + $q[$i + 2]
        $end = $i + 5
        $name = ($labels -join '.').ToLower()
        $ours = $name -like '*.test' -or $name -eq 'test'
        $answer = $ours -and $qtype -eq 1
        $r = [Collections.Generic.List[byte]]::new()
        $r.AddRange([byte[]]$q[0..1])                       # id
        $r.Add(0x85); $r.Add($(if ($ours) { 0x80 } else { 0x83 }))  # response, authoritative; NXDOMAIN if not ours
        $r.AddRange([byte[]](0, 1, 0, $(if ($answer) { 1 } else { 0 }), 0, 0, 0, 0))
        $r.AddRange([byte[]]$q[12..($end - 1)])            # the question
        if ($answer) {
            $r.AddRange([byte[]](0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 127, 0, 0, 1))
        }
        [void]$udp.Send($r.ToArray(), $r.Count, $from)
    }
}
Start-Sleep -Seconds 2
Write-Host "responder: $((Get-Job $responder.Id).State)"

Check 'before the rule: a .test name does not resolve' {
    $r = Resolve-DnsName 'kiln-spike.test' -Type A -ErrorAction SilentlyContinue
    if ($r | Where-Object { $_.IPAddress -eq '127.0.0.1' }) { throw 'it resolved already' }
}

Add-DnsClientNrptRule -Namespace '.test' -NameServers '127.0.0.1' -Comment 'Kiln spike (P4C-14)'
Clear-DnsClientCache
try {
    Check 'with the rule: Resolve-DnsName asks the responder' {
        $r = Resolve-DnsName 'kiln-spike.test' -Type A -DnsOnly
        if (-not ($r | Where-Object { $_.IPAddress -eq '127.0.0.1' })) { throw "answer: $($r | Out-String)" }
    }
    Check 'with the rule: getaddrinfo (.NET, as programs and browsers use it) resolves any name' {
        $ips = [Net.Dns]::GetHostAddresses('shop.kiln-spike.test') | ForEach-Object { $_.ToString() }
        if ($ips -notcontains '127.0.0.1') { throw "addresses: $($ips -join ', ')" }
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
    Check 'other names still go to the usual DNS' {
        $r = [Net.Dns]::GetHostAddresses('github.com')
        if (-not $r) { throw 'github.com did not resolve' }
    }
    Check 'the rule as Windows lists it' {
        $rule = Get-DnsClientNrptRule | Where-Object { $_.Namespace -contains '.test' }
        Write-Host "     $($rule | Select-Object Name, Namespace, NameServers | Out-String)"
        if (-not $rule) { throw 'no rule listed' }
    }
} finally {
    Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'Kiln spike (P4C-14)' } | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
    Clear-DnsClientCache
}
Check 'after removing the rule: .test names stop resolving' {
    $r = Resolve-DnsName 'kiln-spike.test' -Type A -ErrorAction SilentlyContinue
    if ($r | Where-Object { $_.IPAddress -eq '127.0.0.1' }) { throw 'still resolves' }
}
Stop-Job $responder; Remove-Job $responder -Force
if ($failed) { Write-Host '== FAILED'; exit 1 }
Write-Host '== PASSED'
