# Runs Kiln's Windows integration scenario in the test VM (ADR 0024) and prints its report.
#   run.ps1 [-Name kiln-win11] [-Bin <folder with kiln.exe, kiln-helper.exe, ...>]
# Build the binaries first, with the C runtime linked in (a fresh Windows may lack it):
#   $env:RUSTFLAGS='-C target-feature=+crt-static'; cargo build --release --target-dir target\vm-win
# Copies them and the scripts in over PowerShell Direct, starts scenario.ps1 in the test user's
# desktop session, answers Windows' certificate dialog through the VM's keyboard, and prints
# C:\kiln-test\scenario.log. Exit code 0 = passed.
param(
    [string] $Name = 'kiln-win11',
    [string] $Bin = (Join-Path $PSScriptRoot '..\..\..\target\vm-win\release'),
    [string] $Credential = 'E:\VMs\kiln-win11\vm-credential.xml'
)
$ErrorActionPreference = 'Stop'
$cred = Import-Clixml $Credential
$s = New-PSSession -VMName $Name -Credential $cred
Invoke-Command -Session $s {
    New-Item -ItemType Directory -Force C:\kiln-test\bin | Out-Null
    Remove-Item C:\kiln-test\status, C:\kiln-test\scenario.log -ErrorAction SilentlyContinue
}
foreach ($exe in 'kiln.exe', 'kiln-helper.exe', 'kiln-supervisor.exe', 'kiln-shim.exe') {
    Copy-Item -ToSession $s -Path (Join-Path $Bin $exe) -Destination C:\kiln-test\bin\ -Force
}
foreach ($script in 'scenario.ps1', 'snapshot.ps1') {
    Copy-Item -ToSession $s -Path (Join-Path $PSScriptRoot $script) -Destination C:\kiln-test\ -Force
}

# Start it in the logged-on user's desktop session (not elevated), where UAC and Windows'
# certificate dialog behave as for a real user.
Invoke-Command -Session $s {
    param($user)
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' `
        -Argument '-NoProfile -ExecutionPolicy Bypass -WindowStyle Minimized -File C:\kiln-test\scenario.ps1'
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskName KilnScenario -Action $action -Principal $principal -Force | Out-Null
    Start-ScheduledTask -TaskName KilnScenario
} -ArgumentList $cred.UserName

# Follow it; answer the certificate dialog once it is open.
$cs = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
$kb = Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_Keyboard
$answered = $false
$deadline = (Get-Date).AddMinutes(30)
do {
    Start-Sleep -Seconds 5
    $state = Invoke-Command -Session $s { Get-Content C:\kiln-test\status -ErrorAction SilentlyContinue }
    if ($state -eq 'waiting-trust-dialog' -and -not $answered) {
        Start-Sleep -Seconds 5   # let the dialog open and take focus
        & (Join-Path $PSScriptRoot '..\..\..\spikes\windows-dns\vm\screen.ps1') -Name $Name `
            -Out (Join-Path $env:TEMP 'kiln-trust-dialog.png') | Out-Null
        # "Do you want to install this certificate?": Alt+Y chooses Yes.
        Invoke-CimMethod -InputObject $kb -MethodName PressKey -Arguments @{ keyCode = 0x12 } | Out-Null
        Invoke-CimMethod -InputObject $kb -MethodName TypeKey -Arguments @{ keyCode = 0x59 } | Out-Null
        Invoke-CimMethod -InputObject $kb -MethodName ReleaseKey -Arguments @{ keyCode = 0x12 } | Out-Null
        $answered = $true
        Write-Host "Answered the certificate dialog (screenshot: $env:TEMP\kiln-trust-dialog.png)."
    }
} until ($state -eq 'done' -or (Get-Date) -gt $deadline)

$report = Invoke-Command -Session $s { Get-Content C:\kiln-test\scenario.log -Raw; Unregister-ScheduledTask KilnScenario -Confirm:$false }
Remove-PSSession $s
$report
if ($state -ne 'done') { Write-Host 'TIMED OUT'; exit 3 }
if ($report -match '== PASSED') { exit 0 } else { exit 1 }
