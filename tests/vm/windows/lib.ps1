# Helpers shared by scenario.ps1 and after-reboot.ps1 (ADR 0024, ADR 0034), dot-sourced after
# they set $Kiln, $dir, $log and $status.

function Say($text) { Add-Content $log $text -Encoding utf8 }
function Status($text) { Set-Content $status $text -Encoding ascii }
function Check($desc, [scriptblock] $test) {
    Say "     ... $(Get-Date -Format HH:mm:ss) $desc"   # shows which step a stuck run is in
    $started = Get-Date
    try {
        $out = & $test 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne $null) { throw "exit code $LASTEXITCODE`n$out" }
        Say "PASS $desc ($([int]((Get-Date) - $started).TotalSeconds) s)"
    } catch {
        Say "FAIL $desc ($([int]((Get-Date) - $started).TotalSeconds) s)"
        ($_ | Out-String).Trim().Split("`n") | Select-Object -Last 8 | ForEach-Object { Say "     $_" }
        $script:failed = $true
    }
}
function Kiln {
    $out = & $Kiln @args 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw "kiln $args failed ($LASTEXITCODE): $($out.Trim())" }
    $out
}
function Get($url) { curl.exe -sS --ssl-revoke-best-effort $url 2>&1 | Out-String }
function Expect($text, $output) { if (-not $output.Contains($text)) { throw "expected '$text' in:`n$output" } }

# Windows asks before it adds a root to the user's store ("Security Warning") and before it
# deletes one ("Root Certificate Store"). Those dialogs open behind whatever window has focus
# (Windows doesn't let a background process take it), so keystrokes from the host can miss
# them, and their buttons are panes UI Automation can't press. So: find the dialog through UI
# Automation, minimize the windows that could cover it, and write where its Yes button is to
# C:\kiln-test\click, in screen pixels; run.ps1 clicks there with the VM's mouse. (Clicks made
# from inside this session by a scheduled task's process don't reach the dialog.)
function Start-Answerer($title) { Start-Job -ArgumentList $title {
    param($title)
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    Add-Type -Namespace KilnTest -Name Win -MemberDefinition '
        [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
        [DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr hwnd, int cmd);'
    [KilnTest.Win]::SetProcessDPIAware() | Out-Null   # positions in physical pixels
    $ae = [Windows.Automation.AutomationElement]
    $byName = { param($n) New-Object Windows.Automation.PropertyCondition($ae::NameProperty, $n) }
    $deadline = (Get-Date).AddMinutes(3)
    while ((Get-Date) -lt $deadline) {
        $dialog = $ae::RootElement.FindFirst('Children', (& $byName $title))
        if ($dialog) {
            foreach ($w in $ae::RootElement.FindAll('Children', [Windows.Automation.Condition]::TrueCondition)) {
                if ($w.Current.Name -ne $title -and $w.Current.ClassName -notin 'Shell_TrayWnd', 'Progman') {
                    [KilnTest.Win]::ShowWindow([IntPtr]$w.Current.NativeWindowHandle, 6) | Out-Null   # minimize
                }
            }
            Start-Sleep -Seconds 1
            $b = $dialog.FindFirst('Descendants', (& $byName 'Yes')).Current.BoundingRectangle
            Set-Content C:\kiln-test\click "$([int]($b.X + $b.Width / 2)) $([int]($b.Y + $b.Height / 2))"
            return "found; Yes is at $b"
        }
        Start-Sleep -Milliseconds 500
    }
    "no '$title' dialog seen"
} }

function SiteUrl($host_) { [regex]::Match($script:started, "https?://$([regex]::Escape($host_))[^\s]*").Value }

# Compares a snapshot with the first one; any difference fails the run and is listed.
function Compare-Snapshot($after, $what) {
    $diff = Compare-Object (Get-Content (Join-Path $dir 'before.txt')) (Get-Content (Join-Path $dir $after))
    if ($diff) {
        Say "FAIL the machine differs $what`:"
        $diff | ForEach-Object { Say "     $($_.SideIndicator) $($_.InputObject)" }
        $script:failed = $true
    } else {
        Say "PASS the machine is as it was $what`: $((Get-Content (Join-Path $dir 'before.txt')).Count) facts compared"
    }
}
