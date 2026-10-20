# Spike 02 Part 2 orchestrator (host side). Copies the payload into the VM over PowerShell
# Direct, runs part2.ps1 phase by phase, drives save/restore and network changes from the host,
# and copies the results back to docs-ready text.
param([string] $Name = 'kiln-spike02')
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$cache = (Resolve-Path (Join-Path $here '..\.cache')).Path
$cred = Import-Clixml (Join-Path $cache 'vm-credential.xml')
function NewSession {
    $s = New-PSSession -VMName $Name -Credential $cred
    # Fresh Windows defaults to "Restricted"; allow scripts for this remote process only.
    Invoke-Command -Session $s { Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force }
    $s
}
function Phase($s, $phase, $label = '') {
    Write-Host "== phase $phase $label"
    Invoke-Command -Session $s -ScriptBlock {
        param($p, $l)
        try { & C:\spike\part2.ps1 -Phase $p -Label $l }
        catch {
            $pos = ($_.InvocationInfo.PositionMessage -split "`n")[0]
            Add-Content C:\spike\part2.txt "phase_error_${p}_$l=$($_.Exception.Message) @ $pos" -Encoding utf8
            "phase $p failed: $($_.Exception.Message) @ $pos"
        }
    } -ArgumentList $phase, $label
}
function WaitForGuest {
    for ($i = 0; $i -lt 60; $i++) {
        try { $s = NewSession; Invoke-Command -Session $s { 1 } | Out-Null; return $s } catch { Start-Sleep -Seconds 5 }
    }
    throw 'guest did not come back'
}

$s = NewSession
Invoke-Command -Session $s { New-Item -ItemType Directory -Force C:\spike | Out-Null; Remove-Item C:\spike\part2.txt -ErrorAction SilentlyContinue }
foreach ($f in @("$here\part2.ps1", "$cache\payload\hosts-writer.exe", "$cache\payload\dns-stub.exe", "$cache\payload\wireguard-amd64-1.1.1.msi")) {
    Copy-Item -ToSession $s -Path $f -Destination C:\spike\ -Force
}

Phase $s env
Phase $s hosts
Phase $s nrpt
Phase $s probe 'baseline'

# Sleep/resume equivalent: save the VM's state, restore it, probe again.
Remove-PSSession $s
Save-VM -Name $Name; Start-VM -Name $Name
$s = WaitForGuest
Invoke-Command -Session $s { Add-Content C:\spike\part2.txt 'event=vm saved and restored' }
Phase $s probe 'after_save_restore'

# Network change: unplug and replug the virtual NIC (Wi-Fi <-> Ethernet style change).
Disconnect-VMNetworkAdapter -VMName $Name; Start-Sleep -Seconds 5
Phase $s probe 'nic_unplugged'
Connect-VMNetworkAdapter -VMName $Name -SwitchName 'Default Switch'; Start-Sleep -Seconds 15
Phase $s probe 'nic_replugged'

# Full-tunnel WireGuard with DNS pushed and leak protection.
Phase $s wg-up
Phase $s probe 'wireguard_full_tunnel'
Phase $s wg-down
Phase $s probe 'after_wireguard'

Phase $s port53
Phase $s revert

Copy-Item -FromSession $s -Path C:\spike\part2.txt -Destination (Join-Path $cache 'part2.txt') -Force
Remove-PSSession $s
Get-Content (Join-Path $cache 'part2.txt')
