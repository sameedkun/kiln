# Spike 03 on the Windows test VM (docs/spikes/03-windows-cache.md): resets the VM to its
# `clean` checkpoint with Kiln's binaries, copies Garnet (SHA-256 checked) and the Laravel
# check command in, and runs guest.ps1 there. Prints its report. The VM stays changed; the
# next run.ps1 -Reset puts it back.
#   run-windows.ps1 [-Name kiln-win11]
param(
    [string] $Name = 'kiln-win11',
    [string] $Credential = 'E:\VMs\kiln-win11\vm-credential.xml'
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$zip = Join-Path $here '.cache\garnet-win-x64.zip'
# Garnet v2.2.3 win-x64-based-readytorun.zip, GitHub's asset digest.
$pinned = '5cd130686ab415b13c59d36d048a6faded81e707ffe6da5953f98beeb79f9f16'
if (-not (Test-Path $zip)) {
    New-Item -ItemType Directory -Force (Split-Path $zip) | Out-Null
    Invoke-WebRequest 'https://github.com/microsoft/garnet/releases/download/v2.2.3/win-x64-based-readytorun.zip' -OutFile $zip
}
if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $pinned) { throw "$zip doesn't match its pinned SHA-256" }
# Garnet's Windows build needs the .NET 10 runtime (found in the first run): Microsoft's
# runtime zip, unpacked privately and found through DOTNET_ROOT, installs nothing.
# SHA-512 from Microsoft's release metadata (release-metadata/10.0/releases.json).
$dotnet = Join-Path $here '.cache\dotnet-runtime-10.0.14-win-x64.zip'
$dotnetSha512 = 'a176095f1db69b42c81acc60940744078d6bf2d2ac643e418342aa9c81889f4bd13e42f4621b131f9e77e4bc781ad46e375956219df9a4f979031d2a5a8d93cd'
if (-not (Test-Path $dotnet)) {
    Invoke-WebRequest 'https://builds.dotnet.microsoft.com/dotnet/Runtime/10.0.14/dotnet-runtime-10.0.14-win-x64.zip' -OutFile $dotnet
}
if ((Get-FileHash $dotnet -Algorithm SHA512).Hash -ne $dotnetSha512) { throw "$dotnet doesn't match Microsoft's SHA-512" }

& (Join-Path $here '..\..\tests\vm\windows\run.ps1') -Name $Name -Reset -SetupOnly
$cred = Import-Clixml $Credential
$s = New-PSSession -VMName $Name -Credential $cred
Invoke-Command -Session $s { New-Item -ItemType Directory -Force C:\kiln-spike | Out-Null }
foreach ($file in $zip, $dotnet, (Join-Path $here 'KilnSpike.php'), (Join-Path $here 'guest.ps1')) {
    Copy-Item -ToSession $s -Path $file -Destination C:\kiln-spike\ -Force
}
# The VM's execution policy stops script files; run.ps1's scheduled task bypasses it the same way.
Invoke-Command -Session $s {
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\kiln-spike\guest.ps1 -Dir C:\kiln-spike
}
Remove-PSSession $s
