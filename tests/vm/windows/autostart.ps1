# Checks Kiln's autostart across a real reboot in the Windows test VM (P2-02, ADR 0026 §6):
# a site served, `kiln daemon enable`, reboot (the test VM logs on by itself), then kilnd must
# be running again and have brought the web server back. Exit code 0 = passed.
#   autostart.ps1 [-Name kiln-win11] [-Bin <folder with kiln.exe, kilnd.exe, ...>] [-HardReset]
# Starts from the `clean` checkpoint and leaves the VM changed; run.ps1 -Reset resets it.
param(
    [string] $Name = 'kiln-win11',
    [string] $Bin = (Join-Path $PSScriptRoot '..\..\..\target\vm-win\release'),
    [string] $Credential = 'E:\VMs\kiln-win11\vm-credential.xml',
    # A hard reset instead of a graceful restart: checks Kiln flushed its registry writes.
    [switch] $HardReset
)
$ErrorActionPreference = 'Stop'
$cred = Import-Clixml $Credential

function Wait-Heartbeat {
    $deadline = (Get-Date).AddMinutes(5)
    do { Start-Sleep -Seconds 5 }
    until ((Get-VMIntegrationService -VMName $Name -Name Heartbeat).PrimaryStatusDescription -eq 'OK' -or (Get-Date) -gt $deadline)
}

# A clean VM with Herd; then Kiln's binaries.
& (Join-Path $PSScriptRoot 'run.ps1') -Name $Name -Bin $Bin -Reset -SetupOnly
$s = New-PSSession -VMName $Name -Credential $cred

# In the user's desktop session: a site, the web server, and autostart.
$setup = @'
$k = 'C:\kiln-test\bin\kiln.exe'
$log = 'C:\kiln-test\autostart.log'
New-Item -ItemType Directory -Force C:\kiln-test\site | Out-Null
Set-Content C:\kiln-test\site\index.html 'autostart ok'
& { & $k web install; & $k link site --path C:\kiln-test\site; & $k web start; & $k daemon enable --yes; & $k daemon status } *>&1 |
    Out-File $log -Encoding utf8
Set-Content C:\kiln-test\status 'done' -Encoding ascii
'@
Invoke-Command -Session $s {
    param($script, $user)
    Remove-Item C:\kiln-test\status -ErrorAction SilentlyContinue
    Set-Content C:\kiln-test\autostart-setup.ps1 $script
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\kiln-test\autostart-setup.ps1'
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskName KilnAutostart -Action $action -Principal $principal -Force | Out-Null
    Start-ScheduledTask -TaskName KilnAutostart
    $deadline = (Get-Date).AddMinutes(10)
    do { Start-Sleep -Seconds 3 } until ((Test-Path C:\kiln-test\status) -or (Get-Date) -gt $deadline)
    Unregister-ScheduledTask KilnAutostart -Confirm:$false
    Get-Content C:\kiln-test\autostart.log
    "Run value: $((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name Kiln -ErrorAction SilentlyContinue).Kiln)"
} -ArgumentList $setup, $cred.UserName
Remove-PSSession $s

Write-Host '== reboot'
# A graceful restart, as a user's: a hard reset can also lose registry writes that Windows
# hadn't flushed yet (Kiln now flushes its own, see kiln-sys).
Restart-VM -Name $Name -Type $(if ($HardReset) { 'Reset' } else { 'Reboot' }) -Force -Wait -For Heartbeat
Wait-Heartbeat
Start-Sleep -Seconds 60   # logon, then kilnd's first watchdog pass (every 15 s)

$s = New-PSSession -VMName $Name -Credential $cred
$result = Invoke-Command -Session $s {
    $kilnd = Get-Process kilnd -ErrorAction SilentlyContinue
    $caddy = Get-Process kiln-caddy -ErrorAction SilentlyContinue
    $page = curl.exe -s -m 10 -H 'Host: site.test' http://127.0.0.1:8080/ 2>&1 | Out-String
    [pscustomobject]@{
        KilndRunning = [bool]$kilnd
        CaddyRunning = [bool]$caddy
        SiteAnswers  = $page.Contains('autostart ok')
        Log          = (Get-Content "$env:LOCALAPPDATA\Kiln\logs\kilnd.log" -Tail 12 -ErrorAction SilentlyContinue) -join "`n"
    }
}
Remove-PSSession $s
"kilnd running after reboot: $($result.KilndRunning)"
"web server running after reboot: $($result.CaddyRunning)"
"site answers after reboot: $($result.SiteAnswers)"
"kilnd.log (end):"
$result.Log
if ($result.KilndRunning -and $result.CaddyRunning -and $result.SiteAnswers) { '== PASSED'; exit 0 }
'== FAILED'; exit 1
