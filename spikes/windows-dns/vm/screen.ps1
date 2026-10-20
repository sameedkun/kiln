# Saves a 640x480 PNG of a Hyper-V VM's display (works without a console connection).
#   screen.ps1 [-Name kiln-spike02] [-Out path.png]
param([string] $Name = 'kiln-spike02', [string] $Out = (Join-Path $PSScriptRoot '..\.cache\vm-screen.png'))
$vm = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
$svc = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_VirtualSystemManagementService
$set = Get-CimAssociatedInstance -InputObject $vm -ResultClassName Msvm_VirtualSystemSettingData |
    Where-Object { $_.VirtualSystemType -eq 'Microsoft:Hyper-V:System:Realized' }
$w = 640; $h = 480
$r = Invoke-CimMethod -InputObject $svc -MethodName GetVirtualSystemThumbnailImage -Arguments @{
    TargetSystem = $set; WidthPixels = [uint16]$w; HeightPixels = [uint16]$h }
$data = [byte[]]$r.ImageData
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap $w, $h
for ($y = 0; $y -lt $h; $y++) {
    for ($x = 0; $x -lt $w; $x++) {
        $i = ($y * $w + $x) * 2
        $v = [int]$data[$i] -bor ([int]$data[$i + 1] -shl 8)   # RGB565, little-endian
        $bmp.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(
            (($v -shr 11) -band 31) * 255 / 31, (($v -shr 5) -band 63) * 255 / 63, ($v -band 31) * 255 / 31))
    }
}
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$Out
