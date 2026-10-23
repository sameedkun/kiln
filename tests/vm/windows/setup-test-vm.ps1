# Test-VM-only settings for Kiln's Windows integration scenario (ADR 0024 §3), run once from
# the host after new-vm.ps1, never on a real machine:
# - UAC elevates administrators without asking (it stays on, so Kiln still elevates through it);
# - a Herd test site (~\Herd\herdcheck) for the coexistence checks.
# Laravel Herd itself is installed by hand in the VM before this (its setup is a GUI).
#   setup-test-vm.ps1 [-Name kiln-win11]
param(
    [string] $Name = 'kiln-win11',
    [string] $Credential = 'E:\VMs\kiln-win11\vm-credential.xml'
)
$ErrorActionPreference = 'Stop'
$s = New-PSSession -VMName $Name -Credential (Import-Clixml $Credential)
Invoke-Command -Session $s {
    Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' `
        -Name ConsentPromptBehaviorAdmin -Value 0 -Type DWord
    New-Item -ItemType Directory -Force "$env:USERPROFILE\Herd\herdcheck" | Out-Null
    Set-Content "$env:USERPROFILE\Herd\herdcheck\index.php" '<?php echo "herd check ", PHP_VERSION;'
    "UAC: administrators elevate without a prompt (test VM only)."
    "Herd test site: $env:USERPROFILE\Herd\herdcheck"
    "Herd installed: $(Test-Path "$env:ProgramFiles\Herd")"
}
Remove-PSSession $s
