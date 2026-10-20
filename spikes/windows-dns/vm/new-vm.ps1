# Spike 02 Part 2: create a disposable Windows 11 test VM with an unattended install.
# Requires membership in "Hyper-V Administrators" (no elevation needed).
#   new-vm.ps1 -Iso D:\path\Win11.iso [-Root D:\VMs\kiln-spike02]
# Writes the generated test-user password to spikes\windows-dns\.cache\vm-credential.xml
# (git-ignored, DPAPI-protected for the current user). Nothing else on the host is changed.
param(
    [Parameter(Mandatory)] [string] $Iso,
    # Needs ~30 GB free; the first attempt on a nearly full D: had to be aborted.
    [string] $Root = 'E:\VMs\kiln-spike02',
    [string] $Name = 'kiln-spike02'
)
$ErrorActionPreference = 'Stop'
$cache = Join-Path $PSScriptRoot '..\.cache' | Resolve-Path -ErrorAction SilentlyContinue
if (-not $cache) { $cache = New-Item -ItemType Directory -Force (Join-Path $PSScriptRoot '..\.cache') }
$cache = "$cache"
if (Get-VM -Name $Name -ErrorAction SilentlyContinue) { throw "VM $Name already exists" }
New-Item -ItemType Directory -Force $Root | Out-Null

# --- test account (random password, DPAPI-protected on the host) ---
$user = 'kiln'
$bytes = New-Object byte[] 18; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
$plain = 'Kx!' + [Convert]::ToBase64String($bytes).Replace('/', 'a').Replace('+', 'b')
New-Object PSCredential($user, (ConvertTo-SecureString $plain -AsPlainText -Force)) |
    Export-Clixml (Join-Path $cache 'vm-credential.xml')

# --- autounattend.xml ---
$xml = @"
<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
  <settings pass="windowsPE">
    <component name="Microsoft-Windows-International-Core-WinPE" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <SetupUILanguage><UILanguage>en-US</UILanguage></SetupUILanguage>
      <InputLocale>en-US</InputLocale><SystemLocale>en-US</SystemLocale><UILanguage>en-US</UILanguage><UserLocale>en-US</UserLocale>
    </component>
    <component name="Microsoft-Windows-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <RunSynchronous>
        <RunSynchronousCommand wcm:action="add"><Order>1</Order><Path>reg add HKLM\SYSTEM\Setup\LabConfig /v BypassTPMCheck /t REG_DWORD /d 1 /f</Path></RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add"><Order>2</Order><Path>reg add HKLM\SYSTEM\Setup\LabConfig /v BypassSecureBootCheck /t REG_DWORD /d 1 /f</Path></RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add"><Order>3</Order><Path>reg add HKLM\SYSTEM\Setup\LabConfig /v BypassRAMCheck /t REG_DWORD /d 1 /f</Path></RunSynchronousCommand>
      </RunSynchronous>
      <DiskConfiguration>
        <Disk wcm:action="add">
          <DiskID>0</DiskID><WillWipeDisk>true</WillWipeDisk>
          <CreatePartitions>
            <CreatePartition wcm:action="add"><Order>1</Order><Type>EFI</Type><Size>300</Size></CreatePartition>
            <CreatePartition wcm:action="add"><Order>2</Order><Type>MSR</Type><Size>16</Size></CreatePartition>
            <CreatePartition wcm:action="add"><Order>3</Order><Type>Primary</Type><Extend>true</Extend></CreatePartition>
          </CreatePartitions>
          <ModifyPartitions>
            <ModifyPartition wcm:action="add"><Order>1</Order><PartitionID>1</PartitionID><Format>FAT32</Format><Label>System</Label></ModifyPartition>
            <ModifyPartition wcm:action="add"><Order>2</Order><PartitionID>3</PartitionID><Format>NTFS</Format><Label>Windows</Label><Letter>C</Letter></ModifyPartition>
          </ModifyPartitions>
        </Disk>
      </DiskConfiguration>
      <ImageInstall><OSImage><InstallTo><DiskID>0</DiskID><PartitionID>3</PartitionID></InstallTo></OSImage></ImageInstall>
      <UserData>
        <AcceptEula>true</AcceptEula>
        <!-- Microsoft's published generic installation key for Windows 11 Pro: installs, does not activate. -->
        <ProductKey><Key>VK7JG-NPHTM-C97JM-9MPGT-3V66T</Key><WillShowUI>OnError</WillShowUI></ProductKey>
      </UserData>
    </component>
  </settings>
  <settings pass="specialize">
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <ComputerName>KILNSPIKE02</ComputerName>
    </component>
  </settings>
  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-International-Core" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <InputLocale>en-US</InputLocale><SystemLocale>en-US</SystemLocale><UILanguage>en-US</UILanguage><UserLocale>en-US</UserLocale>
    </component>
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <OOBE>
        <HideEULAPage>true</HideEULAPage><HideOEMRegistrationScreen>true</HideOEMRegistrationScreen>
        <HideOnlineAccountScreens>true</HideOnlineAccountScreens><HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>
        <ProtectYourPC>3</ProtectYourPC>
      </OOBE>
      <UserAccounts><LocalAccounts>
        <LocalAccount wcm:action="add">
          <Name>$user</Name><Group>Administrators</Group>
          <Password><Value>$([Security.SecurityElement]::Escape($plain))</Value><PlainText>true</PlainText></Password>
        </LocalAccount>
      </LocalAccounts></UserAccounts>
      <AutoLogon><Enabled>true</Enabled><Username>$user</Username><LogonCount>999</LogonCount>
        <Password><Value>$([Security.SecurityElement]::Escape($plain))</Value><PlainText>true</PlainText></Password></AutoLogon>
      <TimeZone>Pakistan Standard Time</TimeZone>
    </component>
  </settings>
</unattend>
"@
$work = Join-Path $Root 'unattend'
New-Item -ItemType Directory -Force $work | Out-Null
Set-Content (Join-Path $work 'autounattend.xml') $xml -Encoding UTF8

# --- pack autounattend.xml into a small ISO with the built-in IMAPI2 COM API ---
$unattendIso = Join-Path $Root 'unattend.iso'
if (-not ('IsoWriter' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.IO; using System.Runtime.InteropServices; using System.Runtime.InteropServices.ComTypes;
public static class IsoWriter {
    public static void Save(object stream, string path) {
        IStream s = (IStream)stream; byte[] buf = new byte[65536]; IntPtr read = Marshal.AllocHGlobal(4);
        try { using (FileStream fs = File.Create(path)) {
            while (true) { s.Read(buf, buf.Length, read); int n = Marshal.ReadInt32(read); if (n == 0) break; fs.Write(buf, 0, n); } } }
        finally { Marshal.FreeHGlobal(read); }
    }
}
'@
}
$fsi = New-Object -ComObject IMAPI2FS.MsftFileSystemImage
$fsi.FileSystemsToCreate = 3   # ISO9660 + Joliet
$fsi.VolumeName = 'UNATTEND'
$fsi.Root.AddTree($work, $false)
[IsoWriter]::Save($fsi.CreateResultImage().ImageStream, $unattendIso)
Remove-Item -Recurse -Force $work   # the plain-text password only lives inside unattend.iso now

# --- VM ---
New-VM -Name $Name -Generation 2 -MemoryStartupBytes 4GB -Path $Root `
    -NewVHDPath (Join-Path $Root "$Name.vhdx") -NewVHDSizeBytes 64GB -SwitchName 'Default Switch' | Out-Null
Set-VMProcessor -VMName $Name -Count 4
Set-VMMemory -VMName $Name -DynamicMemoryEnabled $false
Set-VM -Name $Name -AutomaticCheckpointsEnabled $false -CheckpointType Standard
Enable-VMIntegrationService -VMName $Name -Name 'Guest Service Interface'
$dvd = Add-VMDvdDrive -VMName $Name -Path $Iso -Passthru
Add-VMDvdDrive -VMName $Name -Path $unattendIso | Out-Null
# Boot order: install DVD, then the disk. No network (PXE) entry: after Setup's reboots the
# DVD prompt times out and the VM must fall through to the disk, not stall on PXE.
$disk = Get-VMHardDiskDrive -VMName $Name
Set-VMFirmware -VMName $Name -BootOrder $dvd, $disk -EnableSecureBoot On -SecureBootTemplate MicrosoftWindows

Start-VM -Name $Name
# Gen2 DVD boot waits for "Press any key to boot from CD or DVD"; tap Enter for ~40 s.
$cs = Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter "ElementName='$Name'"
$kb = Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_Keyboard
for ($i = 0; $i -lt 40; $i++) { Invoke-CimMethod -InputObject $kb -MethodName TypeKey -Arguments @{ keyCode = 13 } | Out-Null; Start-Sleep -Seconds 1 }
"VM $Name started; unattended install running."
