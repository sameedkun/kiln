# Spike 02 Part 2 harness. Runs INSIDE the Hyper-V test VM via PowerShell Direct, as the local
# admin "kiln" (elevated remote token). Each phase appends key=value lines to $Out.
#   part2.ps1 -Phase <env|hosts|nrpt|edge|wg-up|probe|wg-down|port53|revert> [-Label x]
param([Parameter(Mandatory)][string] $Phase, [string] $Label = '')
$ErrorActionPreference = 'Continue'
$P = 'C:\spike'
$Out = "$P\part2.txt"
$hostsFile = "$env:windir\System32\drivers\etc\hosts"
$tool = "$P\hosts-writer.exe"
function Rec($k, $v) { Add-Content $Out "$k=$v" -Encoding utf8 }
function Resolves($name, $timeoutMs = 4000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        try { if (([Net.Dns]::GetHostAddresses($name)).IPAddressToString -contains '127.0.0.1') { return $sw.ElapsedMilliseconds } } catch {}
        Start-Sleep -Milliseconds 100
    }
    return -1
}
function ResolveExternal($name) {
    try { $r = Resolve-DnsName $name -Type A -QuickTimeout -ErrorAction Stop | Where-Object Type -eq 'A' | Select-Object -First 1; if ($r) { 'ok' } else { 'empty' } } catch { 'fail' }
}
function Edge($url) {
    # Edge must run like a real user's browser: in the interactive console session (PowerShell
    # Direct runs in session 0, where headless Edge prints nothing) and NOT elevated (Edge exits
    # when started with an elevated token). A one-off scheduled task gives us both.
    $edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
    if (-not (Test-Path $edge)) { return 'no-edge' }
    $id = [guid]::NewGuid().ToString('N').Substring(0, 8)
    $prof = "$P\edge-profile-$id"; $outf = "$P\edge-$id.txt"; $cmdf = "$P\edge-$id.cmd"
    Set-Content $cmdf "@`"$edge`" --headless=new --disable-gpu --no-first-run --user-data-dir=`"$prof`" --virtual-time-budget=4000 --dump-dom $url > `"$outf`" 2>nul" -Encoding ascii
    $action = New-ScheduledTaskAction -Execute $cmdf
    $principal = New-ScheduledTaskPrincipal -UserId 'kiln' -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskName "kiln-edge-$id" -Action $action -Principal $principal -Force | Out-Null
    Start-ScheduledTask "kiln-edge-$id"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    do { Start-Sleep -Milliseconds 500 } while ((Get-ScheduledTask "kiln-edge-$id").State -eq 'Running' -and $sw.ElapsedMilliseconds -lt 30000)
    Unregister-ScheduledTask "kiln-edge-$id" -Confirm:$false
    $o = Get-Content $outf -Raw -ErrorAction SilentlyContinue
    Get-Process msedge -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
    Remove-Item -Recurse -Force $prof, $outf, $cmdf -ErrorAction SilentlyContinue
    if ($o -match 'kiln-ok') { 'ok' } else { 'fail' }
}
function StartWeb {
    # Tiny HTTP responder on 127.0.0.1:8099 so browsers have something to load.
    if (Get-NetTCPConnection -LocalPort 8099 -State Listen -ErrorAction SilentlyContinue) { return }
    Start-Job -Name web -ScriptBlock {
        $l = [Net.HttpListener]::new(); $l.Prefixes.Add('http://+:8099/'); $l.Start()
        while ($true) { $c = $l.GetContext(); $b = [Text.Encoding]::UTF8.GetBytes("<html><body>kiln-ok $($c.Request.Url.Host)</body></html>"); $c.Response.OutputStream.Write($b, 0, $b.Length); $c.Response.Close() }
    } | Out-Null
    Start-Sleep -Seconds 2
}
function SetEdgeDoh($mode) {
    $k = 'HKLM:\SOFTWARE\Policies\Microsoft\Edge'
    if ($mode -eq 'off') { Remove-ItemProperty $k -Name DnsOverHttpsMode, DnsOverHttpsTemplates -ErrorAction SilentlyContinue; return }
    New-Item $k -Force | Out-Null
    Set-ItemProperty $k DnsOverHttpsMode $mode
    Set-ItemProperty $k DnsOverHttpsTemplates 'https://cloudflare-dns.com/dns-query'
}
function ProbeAll($tag) {
    Rec "${tag}_hosts_name" (Resolves 'hostsmode.test')
    Rec "${tag}_nrpt_wildcard" (Resolves "w$([Environment]::TickCount).deep.nrpt.test")
    Rec "${tag}_external_example_com" (ResolveExternal 'example.com')
    StartWeb
    Rec "${tag}_edge_hosts_name" (Edge 'http://hostsmode.test:8099/')
    Rec "${tag}_edge_nrpt_wildcard" (Edge 'http://edge.deep.nrpt.test:8099/')
}

switch ($Phase) {
    'env' {
        Set-Content $Out "phase_env=$(Get-Date -Format o)" -Encoding utf8
        $os = Get-CimInstance Win32_OperatingSystem
        Rec 'os' "$($os.Caption) $($os.BuildNumber)"
        Rec 'user' ([Security.Principal.WindowsIdentity]::GetCurrent().Name)
        Rec 'elevated' ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        $mp = Get-MpComputerStatus
        Rec 'defender_rtp' $mp.RealTimeProtectionEnabled
        Rec 'defender_engine' $mp.AMEngineVersion
        Rec 'edge' ((Get-Item "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe" -ErrorAction SilentlyContinue).VersionInfo.ProductVersion)
        Rec 'nrpt_rules_baseline' (@(Get-DnsClientNrptRule).Count)
        Rec 'port53_baseline' (@(Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue).Count)
        Rec 'hosts_sha_baseline' (Get-FileHash $hostsFile).Hash
        Rec 'external_baseline' (ResolveExternal 'example.com')
    }
    'hosts' {
        # Hosts mode with Defender real-time protection ON, as an elevated admin (not SYSTEM).
        $names = 1..50 | ForEach-Object { "h$_.test" }
        $r = & $tool apply $hostsFile replacefile hostsmode.test @names
        Rec 'hosts_apply' (($r | Where-Object { $_ -match 'written|error' }) -join ' ')
        Rec 'hosts_resolve_ms' (Resolves 'h50.test')
        Start-Sleep -Seconds 20
        Rec 'hosts_block_intact_after_20s' ((& $tool read $hostsFile | Where-Object { $_ -like 'entry=*' }).Count)
        $det = Get-MpThreatDetection -ErrorAction SilentlyContinue
        Rec 'hosts_defender_detections' $(if ($det) { ($det | ForEach-Object { "$($_.ThreatID):$($_.Resources -join ';')" }) -join ' | ' } else { 0 })
        StartWeb
        Rec 'hosts_edge_default' (Edge 'http://hostsmode.test:8099/')
        SetEdgeDoh 'secure'; Rec 'hosts_edge_doh_secure' (Edge 'http://hostsmode.test:8099/')
        SetEdgeDoh 'automatic'; Rec 'hosts_edge_doh_automatic' (Edge 'http://hostsmode.test:8099/')
        SetEdgeDoh 'off'
    }
    'nrpt' {
        $stub = Start-Process "$P\dns-stub.exe" -ArgumentList 'test' -PassThru -WindowStyle Hidden -RedirectStandardOutput "$P\stub.out"
        Start-Sleep -Seconds 1
        Rec 'nrpt_stub_pid' $stub.Id
        Rec 'nrpt_stub_out' ((Get-Content "$P\stub.out" -ErrorAction SilentlyContinue) -join ' ')
        Add-DnsClientNrptRule -Namespace '.test' -NameServers '127.0.0.1' -Comment 'kiln-spike' | Out-Null
        Clear-DnsClientCache
        Rec 'nrpt_rule_added' (@(Get-DnsClientNrptRule | Where-Object Comment -eq 'kiln-spike').Count)
        Rec 'nrpt_wildcard_ms' (Resolves 'any.thing.nrpt.test')
        Rec 'nrpt_resolve_dnsname' $(try { (Resolve-DnsName 'x.nrpt.test' -Type A -ErrorAction Stop | Where-Object Type -eq 'A').IPAddress } catch { 'fail' })
        Rec 'nrpt_external_still_ok' (ResolveExternal 'example.com')
        StartWeb
        Rec 'nrpt_edge_default' (Edge 'http://edge.deep.nrpt.test:8099/')
        SetEdgeDoh 'secure'; Rec 'nrpt_edge_doh_secure' (Edge 'http://edge2.deep.nrpt.test:8099/')
        SetEdgeDoh 'automatic'; Rec 'nrpt_edge_doh_automatic' (Edge 'http://edge3.deep.nrpt.test:8099/')
        SetEdgeDoh 'off'
    }
    'probe' { ProbeAll $Label }
    'matrix' {
        # Edge x DoH mode x DNS mode, 3 trials each, on a warm interactive session.
        StartWeb
        Edge 'http://warmup.test:8099/' | Out-Null
        foreach ($mode in 'off', 'automatic', 'secure') {
            SetEdgeDoh $mode
            $h = 0; $n = 0
            for ($t = 1; $t -le 3; $t++) {
                if ((Edge 'http://hostsmode.test:8099/') -eq 'ok') { $h++ }
                if ((Edge "http://t$t$mode.deep.nrpt.test:8099/") -eq 'ok') { $n++ }
            }
            Rec "matrix_edge_doh_${mode}_hosts" "$h/3"
            Rec "matrix_edge_doh_${mode}_nrpt" "$n/3"
        }
        SetEdgeDoh 'off'
    }
    'wgcheck' {
        for ($t = 1; $t -le 3; $t++) { Rec "wgcheck_nrpt_via_dnsclient_$t" (Resolves "w$t.deep.nrpt.test" 3000) }
        Rec 'wgcheck_nrpt_error' $(try { Resolve-DnsName 'e.nrpt.test' -Type A -ErrorAction Stop | Out-Null; 'none' } catch { $_.Exception.Message })
        Rec 'wgcheck_direct_query_to_stub' $(try { (Resolve-DnsName 'd.nrpt.test' -Type A -Server 127.0.0.1 -DnsOnly -ErrorAction Stop | Where-Object Type -eq 'A').IPAddress } catch { 'fail: ' + $_.Exception.Message })
        Rec 'wgcheck_effective_policy' ((Get-DnsClientNrptPolicy -Effective -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Namespace)->$($_.NameServers -join ',')" }) -join ' | ')
        Rec 'wgcheck_wfp_filters_wireguard' $(try { (netsh wfp show filters file=- 2>$null | Select-String -SimpleMatch 'WireGuard').Count } catch { 'n/a' })
    }
    'wg-up' {
        $msi = "$P\wireguard-amd64-1.1.1.msi"
        # Not "$p": PowerShell names are case-insensitive and $P is the spike directory.
        $msiProc = Start-Process msiexec.exe -ArgumentList "/i `"$msi`" /qn /norestart DO_NOT_LAUNCH=1" -Wait -PassThru
        Rec 'wg_install_exit' $msiProc.ExitCode
        $wg = "$env:ProgramFiles\WireGuard"
        $priv = & "$wg\wg.exe" genkey
        $peerPub = & "$wg\wg.exe" genkey | & "$wg\wg.exe" pubkey
        # Full tunnel to an unreachable TEST-NET peer: the client still applies its DNS servers,
        # routes and leak-protection firewall rules, which is what we want to observe.
        @"
[Interface]
PrivateKey = $priv
Address = 10.64.0.2/32, fd00:64::2/128
DNS = 10.64.0.1

[Peer]
PublicKey = $peerPub
AllowedIPs = 0.0.0.0/0, ::/0
Endpoint = 192.0.2.1:51820
"@ | Set-Content "$P\wgkiln.conf" -Encoding ascii
        & "$wg\wireguard.exe" /installtunnelservice "$P\wgkiln.conf"
        Start-Sleep -Seconds 8
        Rec 'wg_service' ((Get-Service 'WireGuardTunnel$wgkiln' -ErrorAction SilentlyContinue).Status)
        Rec 'wg_adapter_dns' ((Get-DnsClientServerAddress -InterfaceAlias 'wgkiln' -AddressFamily IPv4 -ErrorAction SilentlyContinue).ServerAddresses -join ',')
        Rec 'wg_block_rules' (@(Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object DisplayName -match 'WireGuard').Count)
    }
    'wg-down' {
        & "$env:ProgramFiles\WireGuard\wireguard.exe" /uninstalltunnelservice wgkiln
        Start-Sleep -Seconds 5
        Rec 'wg_service_after_down' $(if (Get-Service 'WireGuardTunnel$wgkiln' -ErrorAction SilentlyContinue) { 'still-present' } else { 'removed' })
    }
    'port53' {
        Get-Process dns-stub -ErrorAction SilentlyContinue | Stop-Process -Force
        Start-Sleep -Seconds 1
        $u = [Net.Sockets.UdpClient]::new([Net.IPEndPoint]::new([Net.IPAddress]::Loopback, 53))
        $o = & "$P\dns-stub.exe" test 2>&1
        Rec 'port53_conflict_stub_exit' $LASTEXITCODE
        Rec 'port53_conflict_stub_out' ($o -join ' ')
        $u.Close()
        $stub = Start-Process "$P\dns-stub.exe" -ArgumentList 'test' -PassThru -WindowStyle Hidden
        Start-Sleep -Seconds 1
        Rec 'port53_restart_ok' ((Resolves 'after.conflict.test') -ge 0)
    }
    'revert' {
        Get-DnsClientNrptRule | Where-Object Comment -eq 'kiln-spike' | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
        Get-Process dns-stub -ErrorAction SilentlyContinue | Stop-Process -Force
        & $tool remove $hostsFile replacefile | Out-Null
        Get-Job -Name web -ErrorAction SilentlyContinue | Stop-Job -PassThru | Remove-Job
        Clear-DnsClientCache
        Rec 'revert_nrpt_rules' (@(Get-DnsClientNrptRule).Count)
        Rec 'revert_port53_listeners' (@(Get-NetUDPEndpoint -LocalPort 53 -ErrorAction SilentlyContinue).Count)
        Rec 'revert_hosts_sha' (Get-FileHash $hostsFile).Hash
        Rec 'revert_wildcard_gone' ((Resolves 'gone.nrpt.test' 1500) -eq -1)
    }
}
