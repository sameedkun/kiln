# Clicks in a Hyper-V VM at a point of a screen.ps1 screenshot (640x480), scaled to the guest's
# resolution, with the synthetic mouse. For driving setup dialogs in test VMs.
#   click.ps1 -Name kiln-win11 -X 386 -Y 329 [-Double]
param(
    [Parameter(Mandatory)] [string] $Name,
    [Parameter(Mandatory)] [int] $X,
    [Parameter(Mandatory)] [int] $Y,
    [switch] $Double
)
$ErrorActionPreference = 'Stop'
$video = Get-VMVideo -VMName $Name
$cs = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
$mouse = Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_SyntheticMouse
$h = [int32]($X * $video.HorizontalResolution / 640)
$v = [int32]($Y * $video.VerticalResolution / 480)
Invoke-CimMethod -InputObject $mouse -MethodName SetAbsolutePosition -Arguments @{ horizontalPosition = $h; verticalPosition = $v } | Out-Null
Start-Sleep -Milliseconds 300
$clicks = if ($Double) { 2 } else { 1 }
for ($i = 0; $i -lt $clicks; $i++) {
    Invoke-CimMethod -InputObject $mouse -MethodName ClickButton -Arguments @{ buttonIndex = [int32]1 } | Out-Null
}
