# Spike 02, Part 1: hosts-block mode. Runs INSIDE Windows Sandbox only (LogonCommand).
# Writes key=value results to C:\spike\results\part1.txt and touches done.txt at the end.
$ErrorActionPreference = 'Continue'
$out = 'C:\spike\results\part1.txt'
$tool = 'C:\spike\tools\hosts-writer.exe'
$hosts = "$env:windir\System32\drivers\etc\hosts"
Set-Content $out "started=$(Get-Date -Format o)" -Encoding utf8
function Rec($k, $v) { Add-Content $out "$k=$v" -Encoding utf8 }
function Sha($p) { (Get-FileHash $p -Algorithm SHA256).Hash }
function Sddl($p) { (Get-Acl $p).Sddl }
function Resolves($name, $timeoutMs = 5000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        try { $a = [Net.Dns]::GetHostAddresses($name); if ($a.IPAddressToString -contains '127.0.0.1') { return $sw.ElapsedMilliseconds } } catch {}
        Start-Sleep -Milliseconds 50
    }
    return -1
}
function Names($n, $prefix) { 1..$n | ForEach-Object { "$prefix$_.test" } }

try {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    Rec 'user' $id.Name
    Rec 'is_admin' $(([Security.Principal.WindowsPrincipal]$id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))
    Rec 'os' (Get-CimInstance Win32_OperatingSystem).Version
    $mp = Get-MpComputerStatus -ErrorAction SilentlyContinue
    Rec 'defender_rtp' $(if ($mp) { $mp.RealTimeProtectionEnabled } else { 'n/a' })

    $orig = Get-Content $hosts -Raw -Encoding Byte
    $origSha = Sha $hosts; $origSddl = Sddl $hosts
    Rec 'baseline_sha' $origSha
    Rec 'baseline_crlf' ([Text.Encoding]::UTF8.GetString($orig).Contains("`r`n"))
    Rec 'baseline_sddl' $origSddl

    # --- T1: resolution latency + ACL preservation per write strategy ---
    foreach ($strategy in 'replacefile', 'rename') {
        foreach ($n in 1, 50, 500) {
            $names = @(Names $n "$strategy-$n-")
            $res = & $tool apply $hosts $strategy @names
            Rec "t1_${strategy}_${n}_tool" (($res | Where-Object { $_ -match 'written_ms|error' }) -join ' ')
            Rec "t1_${strategy}_${n}_resolve_last_ms" (Resolves $names[-1])
            Rec "t1_${strategy}_${n}_resolve_first_ms" (Resolves $names[0])
        }
        Rec "t1_${strategy}_sddl_preserved" ((Sddl $hosts) -eq $origSddl)
        $res = & $tool remove $hosts $strategy
        Rec "t1_${strategy}_removed_sha_matches" ((Sha $hosts) -eq $origSha)
        Rec "t1_${strategy}_stale_name_after_remove_ms" (Resolves "$strategy-1-1.test" 1500)
        # Put the original bytes and ACL back before the next strategy.
        [IO.File]::WriteAllBytes($hosts, $orig); Set-Acl $hosts -AclObject (Get-Acl $hosts | ForEach-Object { $_.SetSecurityDescriptorSddlForm($origSddl); $_ })
    }

    # --- T2: coexistence with another tool's block, 100 random updates ---
    $foreign = "`r`n# Other Tool generated Hosts. Do not change.`r`n127.0.0.1 other-a.test`r`n127.0.0.1 other-b.test`r`n# End Other Tool generated Hosts`r`n"
    [IO.File]::AppendAllText($hosts, $foreign)
    $withForeignSha = Sha $hosts
    $rnd = [Random]::new(42); $ok = $true
    for ($i = 0; $i -lt 100; $i++) {
        $k = $rnd.Next(0, 6)
        $names = if ($k -eq 0) { @() } else { 1..$k | ForEach-Object { "r$($rnd.Next(0, 20)).test" } }
        & $tool apply $hosts replacefile @names | Out-Null
        if (-not ([IO.File]::ReadAllText($hosts)).Contains($foreign)) { $ok = $false; break }
    }
    Rec 't2_foreign_block_intact_100_updates' $ok
    & $tool remove $hosts replacefile | Out-Null
    Rec 't2_remove_restores_bytes' ((Sha $hosts) -eq $withForeignSha)
    Rec 't2_foreign_names_resolve' ((Resolves 'other-a.test') -ge 0)

    # --- T3: edge cases ---
    [IO.File]::WriteAllBytes($hosts, $orig)
    # BOM
    [IO.File]::WriteAllBytes($hosts, [byte[]](0xEF, 0xBB, 0xBF) + $orig)
    $r = & $tool apply $hosts replacefile bom.test
    $b = [IO.File]::ReadAllBytes($hosts)
    Rec 't3_bom_tool' (($r | Where-Object { $_ -match 'written|error' }) -join ' ')
    Rec 't3_bom_preserved' ($b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)
    Rec 't3_bom_name_resolves_ms' (Resolves 'bom.test')
    [IO.File]::WriteAllBytes($hosts, $orig)
    # Read-only attribute
    Set-ItemProperty $hosts -Name IsReadOnly -Value $true
    foreach ($strategy in 'replacefile', 'rename', 'inplace') {
        $r = & $tool apply $hosts $strategy ro.test
        Rec "t3_readonly_$strategy" (($r | Where-Object { $_ -match 'written|error' }) -join ' ')
    }
    Set-ItemProperty $hosts -Name IsReadOnly -Value $false
    [IO.File]::WriteAllBytes($hosts, $orig)
    # Non-UTF-8 byte in a comment (e.g. a Latin-1 comment written by another tool)
    [IO.File]::WriteAllBytes($hosts, $orig + [byte[]](0x23, 0x20, 0x63, 0x61, 0x66, 0xE9, 0x0D, 0x0A))
    $r = & $tool apply $hosts replacefile latin1.test
    Rec 't3_non_utf8' (($r | Where-Object { $_ -match 'utf8|error|written' }) -join ' ')
    Rec 't3_non_utf8_byte_preserved' ([IO.File]::ReadAllBytes($hosts) -contains 0xE9)
    Rec 't3_non_utf8_name_resolves_ms' (Resolves 'latin1.test')
    [IO.File]::WriteAllBytes($hosts, $orig)
    # Exclusive lock held by another process
    $fs = [IO.File]::Open($hosts, 'Open', 'Read', 'None')
    foreach ($strategy in 'replacefile', 'rename') {
        $r = & $tool apply $hosts $strategy locked.test
        Rec "t3_locked_$strategy" (($r | Where-Object { $_ -match 'written|error' }) -join ' ')
    }
    $fs.Close()
    # Missing file
    Rename-Item $hosts "$hosts.bak"
    $r = & $tool apply $hosts replacefile missing.test
    Rec 't3_missing' (($r | Where-Object { $_ -match 'written|error' }) -join ' ')
    Rename-Item "$hosts.bak" $hosts

    # --- T4: Defender ---
    Start-Sleep -Seconds 5
    $det = Get-MpThreatDetection -ErrorAction SilentlyContinue
    Rec 't4_defender_detections' $(if ($det) { ($det | ForEach-Object { "$($_.ThreatID):$($_.Resources -join ';')" }) -join ' | ' } else { 0 })

    # --- T6: final state ---
    [IO.File]::WriteAllBytes($hosts, $orig)
    Rec 'final_sha_matches_baseline' ((Sha $hosts) -eq $origSha)
}
catch { Rec 'fatal' $_.Exception.Message }
finally {
    Rec 'finished' (Get-Date -Format o)
    Set-Content 'C:\spike\results\done.txt' 'done'
}
