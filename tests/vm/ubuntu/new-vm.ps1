# Creates Kiln's Ubuntu 24.04 Desktop test VM (P1-04, P1-15) from Canonical's Hyper-V image:
# a real desktop with NetworkManager, systemd-resolved, polkit and Firefox.
#
#   new-vm.ps1 [-Root E:\VMs\kiln-ubuntu] [-Name kiln-ubuntu]
#
# Needs membership in "Hyper-V Administrators" (no elevation) and about 25 GB free on -Root's
# and -Cache's drive. The image (about 4.6 GB) is downloaded resumably over HTTPS and checked
# against Canonical's published SHA-256 before use. Nothing else on the host is changed.
# Remove everything again with: Remove-VM kiln-ubuntu -Force; Remove-Item -Recurse <Root>
param(
    [string] $Root = 'E:\VMs\kiln-ubuntu',
    [string] $Name = 'kiln-ubuntu',
    [string] $Cache = 'E:\VMs\cache',
    # Dynamic memory: this host has 16 GB and is often busy (I-35).
    [int64] $MaxMemoryGB = 4,
    [int] $Cpus = 4,
    [int64] $DiskGB = 64
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$base = 'https://partner-images.canonical.com/hyper-v/desktop/noble/current'
$zipName = 'ubuntu-noble-hyperv-amd64-ubuntu-desktop-hyperv.vhdx.zip'

if (Get-VM -Name $Name -ErrorAction SilentlyContinue) { throw "VM $Name already exists" }
New-Item -ItemType Directory -Force $Root, $Cache | Out-Null

# --- download and verify ---
# curl, not Invoke-WebRequest: PowerShell 5.1 returns this octet-stream as bytes, not text.
$sums = (& curl.exe -fsSL "$base/SHA256SUMS") -join "`n"
if ($LASTEXITCODE -ne 0) { throw "cannot fetch $base/SHA256SUMS (curl exit $LASTEXITCODE)" }
$line = ($sums -split "`n") | Where-Object { $_ -match ('\*' + [regex]::Escape($zipName) + '\s*$') }
if (-not $line) { throw "$zipName is not listed in $base/SHA256SUMS" }
$expected = ($line -split '\s+')[0].ToUpperInvariant()
$zip = Join-Path $Cache $zipName
if ((Test-Path $zip) -and (Get-FileHash $zip -Algorithm SHA256).Hash -ne $expected) {
    Write-Host "Cached $zipName is outdated or damaged; downloading again."
    Remove-Item $zip
}
if (-not (Test-Path $zip)) {
    $part = "$zip.part"
    Write-Host "Downloading $zipName (about 4.6 GB, resumable) ..."
    # -sS: no progress meter on stderr, which PowerShell 5.1 turns into an error when redirected.
    & curl.exe -fsSL --retry 5 -C - -o $part "$base/$zipName"
    if ($LASTEXITCODE -ne 0) { throw "download failed (curl exit $LASTEXITCODE); run again to resume" }
    $actual = (Get-FileHash $part -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
        Remove-Item $part
        throw "SHA-256 mismatch: expected $expected, got $actual (download removed)"
    }
    Move-Item $part $zip
}
Write-Host "Verified $zipName (SHA-256 $expected)."

# --- unpack the disk ---
$unpack = Join-Path $Root 'unpack'
New-Item -ItemType Directory -Force $unpack | Out-Null
& tar.exe -xf $zip -C $unpack
if ($LASTEXITCODE -ne 0) { throw "unpacking $zip failed" }
$vhdx = Get-ChildItem $unpack -Recurse -Filter *.vhdx | Select-Object -First 1
if (-not $vhdx) { throw "no .vhdx inside $zipName" }
$disk = Join-Path $Root "$Name.vhdx"
Move-Item $vhdx.FullName $disk
Remove-Item -Recurse $unpack
try {
    Resize-VHD -Path $disk -SizeBytes ($DiskGB * 1GB)
} catch {
    Write-Warning "Couldn't grow the disk to $DiskGB GB ($($_.Exception.Message)); it keeps the image's size."
}

# --- the VM ---
New-VM -Name $Name -Generation 2 -MemoryStartupBytes 2GB -VHDPath $disk -SwitchName 'Default Switch' -Path $Root | Out-Null
Set-VM -Name $Name -ProcessorCount $Cpus -DynamicMemory -MemoryMinimumBytes 1GB -MemoryMaximumBytes ($MaxMemoryGB * 1GB) `
    -AutomaticCheckpointsEnabled $false -CheckpointType Standard -EnhancedSessionTransportType HvSocket
# Ubuntu's boot loader is signed by Microsoft's third-party UEFI CA, not the Windows one.
Set-VMFirmware -VMName $Name -SecureBootTemplate MicrosoftUEFICertificateAuthority
Enable-VMIntegrationService -VMName $Name -Name 'Guest Service Interface'
Start-VM -Name $Name
Write-Host "Started $Name. Opening its console for Ubuntu's first-boot setup."
Start-Process vmconnect.exe -ArgumentList 'localhost', $Name
