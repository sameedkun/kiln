# Runs Kiln's Windows integration scenario in the test VM (ADR 0024) and prints its report.
#   run.ps1 [-Name kiln-win11] [-Bin <folder with kiln.exe, kiln-helper.exe, ...>]
# Build the binaries first, with the C runtime linked in (a fresh Windows may lack it):
#   $env:RUSTFLAGS='-C target-feature=+crt-static'; cargo build --release --target-dir target\vm-win
# Copies them and the scripts in over PowerShell Direct, starts scenario.ps1 in the test user's
# desktop session, reboots the VM when it asks to (ADR 0034), starts after-reboot.ps1 once the
# user is logged on again, and prints C:\kiln-test\scenario.log. Exit code 0 = passed.
#   -Reset first returns the VM to its `clean` checkpoint (ADR 0024 §7) and starts it.
#   -SetupOnly stops after copying the binaries in (for other checks, such as autostart.ps1).
param(
    [string] $Name = 'kiln-win11',
    [string] $Bin = (Join-Path $PSScriptRoot '..\..\..\target\vm-win\release'),
    [string] $Credential = 'E:\VMs\kiln-win11\vm-credential.xml',
    [switch] $Reset,
    [switch] $SetupOnly
)
$ErrorActionPreference = 'Stop'
$cred = Import-Clixml $Credential

if ($Reset) {
    Stop-VM $Name -TurnOff -Force -WarningAction SilentlyContinue
    # Restoring applies the checkpoint even when there isn't memory to resume it right away.
    try { Restore-VMSnapshot -VMName $Name -Name clean -Confirm:$false } catch { Write-Host $_ }
    # The host may be short of the ~4 GB it needs for a moment: retry for a few minutes.
    $deadline = (Get-Date).AddMinutes(5)
    while ((Get-VM $Name).State -ne 'Running') {
        try { Start-VM $Name } catch {
            if ((Get-Date) -gt $deadline) { throw }
            Write-Host "Can't start $Name yet ($($_.Exception.Message.Split("`n")[0])); retrying."
            Start-Sleep -Seconds 15
        }
    }
    do { Start-Sleep -Seconds 5 } until ((Get-VMIntegrationService -VMName $Name -Name Heartbeat).PrimaryStatusDescription -eq 'OK')
    Start-Sleep -Seconds 20   # let the desktop session settle
}
$s = New-PSSession -VMName $Name -Credential $cred
Invoke-Command -Session $s {
    # A restored checkpoint keeps the address it had then; the host's virtual switch may have
    # moved since (after a host reboot), leaving the VM without a network.
    # Renewing alone keeps the stale lease; release it first.
    ipconfig /release | Out-Null
    ipconfig /renew | Out-Null
    if (-not (Test-NetConnection downloads.php.net -Port 443 -InformationLevel Quiet)) {
        throw 'The test VM has no internet connection.'
    }
    # Vite (the proxy-site checks) makes Windows ask whether Node may use the network. Nobody
    # answers that prompt here, and it would cover the certificate dialog during uninstall.
    # Inbound connections are still blocked, as they are when nobody answers the prompt.
    Set-NetFirewallProfile -All -NotifyOnListen False
    New-Item -ItemType Directory -Force C:\kiln-test\bin | Out-Null
    Remove-Item C:\kiln-test\status, C:\kiln-test\scenario.log -ErrorAction SilentlyContinue
}
foreach ($exe in 'kiln.exe', 'kiln-helper.exe', 'kiln-supervisor.exe', 'kiln-shim.exe', 'kilnd.exe') {
    Copy-Item -ToSession $s -Path (Join-Path $Bin $exe) -Destination C:\kiln-test\bin\ -Force
}
foreach ($script in 'scenario.ps1', 'after-reboot.ps1', 'lib.ps1', 'snapshot.ps1') {
    Copy-Item -ToSession $s -Path (Join-Path $PSScriptRoot $script) -Destination C:\kiln-test\ -Force
}
if ($SetupOnly) {
    Remove-PSSession $s
    return
}

# Start a script in the logged-on user's desktop session (not elevated), where UAC and Windows'
# certificate dialog behave as for a real user.
function Start-InSession($session, $script) {
    Invoke-Command -Session $session {
        param($user, $script)
        $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
            -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Minimized -File C:\kiln-test\$script"
        $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
        Register-ScheduledTask -TaskName KilnScenario -Action $action -Principal $principal -Force | Out-Null
        Start-ScheduledTask -TaskName KilnScenario
    } -ArgumentList $cred.UserName, $script
}

# Follow it until it says "done" or "reboot". When Windows' certificate dialog is open, the
# scenario writes where its Yes button is (screen pixels) to C:\kiln-test\click; click there
# with the VM's mouse.
function Wait-Scenario($session, $minutes) {
    $cs = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
    $mouse = Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_SyntheticMouse
    $deadline = (Get-Date).AddMinutes($minutes)
    do {
        Start-Sleep -Seconds 5
        $state, $click = Invoke-Command -Session $session {
            Get-Content C:\kiln-test\status -ErrorAction SilentlyContinue
            if (Test-Path C:\kiln-test\click) { Get-Content C:\kiln-test\click; Remove-Item C:\kiln-test\click }
        }
        if ($click) {
            $x, $y = $click -split ' '
            Invoke-CimMethod -InputObject $mouse -MethodName SetAbsolutePosition `
                -Arguments @{ horizontalPosition = [int32]$x; verticalPosition = [int32]$y } | Out-Null
            Start-Sleep -Milliseconds 500
            Invoke-CimMethod -InputObject $mouse -MethodName ClickButton -Arguments @{ buttonIndex = [int32]1 } | Out-Null
            Write-Host "Clicked Yes in the certificate dialog at $x,$y."
        }
    } until ($state -in 'done', 'reboot' -or (Get-Date) -gt $deadline)
    Invoke-Command -Session $session { Unregister-ScheduledTask KilnScenario -Confirm:$false }
    $state
}

Start-InSession $s 'scenario.ps1'
$state = Wait-Scenario $s 60   # downloads plus Composer; slower while Windows Update runs

# ADR 0034: a graceful restart, as a user's, with the test user's automatic logon; then a
# minute for kilnd's first watchdog pass (every 15 s) to bring the web server back.
if ($state -eq 'reboot') {
    Remove-PSSession $s
    Write-Host 'Rebooting the VM (ADR 0034).'
    Restart-VM -Name $Name -Type Reboot -Force -Wait -For Heartbeat
    do { Start-Sleep -Seconds 5 } until ((Get-VMIntegrationService -VMName $Name -Name Heartbeat).PrimaryStatusDescription -eq 'OK')
    Start-Sleep -Seconds 60
    $s = New-PSSession -VMName $Name -Credential $cred
    Start-InSession $s 'after-reboot.ps1'
    $state = Wait-Scenario $s 20
}

$report = Invoke-Command -Session $s { Get-Content C:\kiln-test\scenario.log -Raw }
Remove-PSSession $s
$report
if ($state -ne 'done') { Write-Host 'TIMED OUT'; exit 3 }
if ($report -match '== PASSED') { exit 0 } else { exit 1 }
