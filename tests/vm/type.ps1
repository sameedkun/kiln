# Types text into a Hyper-V VM's keyboard, as if typed at its console (works without
# clipboard or enhanced session). With -Enter, presses Enter afterwards.
#   type.ps1 -Name kiln-ubuntu -Text 'echo hello' [-Enter]
# Needs membership in "Hyper-V Administrators". US keyboard layout in the guest.
param(
    [Parameter(Mandatory)] [string] $Name,
    [Parameter(Mandatory)] [string] $Text,
    [switch] $Enter
)
$ErrorActionPreference = 'Stop'
$vm = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
if (-not $vm) { throw "no VM named $Name" }
$keyboard = Get-CimAssociatedInstance -InputObject $vm -ResultClassName Msvm_Keyboard
$result = Invoke-CimMethod -InputObject $keyboard -MethodName TypeText -Arguments @{ AsciiText = $Text }
if ($result.ReturnValue -ne 0) { throw "TypeText failed ($($result.ReturnValue))" }
if ($Enter) {
    # 0x0D: VK_RETURN
    $result = Invoke-CimMethod -InputObject $keyboard -MethodName TypeKey -Arguments @{ KeyCode = [uint32]13 }
    if ($result.ReturnValue -ne 0) { throw "TypeKey failed ($($result.ReturnValue))" }
}
