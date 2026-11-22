param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
if (-not $NoBuild) {
    $env:CARGO_HOME = Join-Path $projectRoot '.cargo-home'
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    cargo build --locked -p muse-desktop
    if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
}
if (-not ('MuseWindowRegression' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MuseWindowRegression {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd,out RECT rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd,out RECT rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd,ref POINT point);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd,IntPtr after,int x,int y,int w,int h,uint flags);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd,int command);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr hwnd,uint message,IntPtr wparam,IntPtr lparam);
    public static int HitTest(IntPtr hwnd,int x,int y) {
        var point=new POINT { X=x,Y=y };
        SendMessageW(hwnd,0x200,IntPtr.Zero,new IntPtr((y<<16)|(x&65535)));
        ClientToScreen(hwnd,ref point);
        return SendMessageW(hwnd,0x84,IntPtr.Zero,new IntPtr((point.Y<<16)|(point.X&65535))).ToInt32();
    }
}
'@
}
$fixtureRoot = Join-Path $projectRoot ('.runtime/window-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
$null = New-Item -ItemType Directory -Path $fixtureRoot
$binary = Join-Path $projectRoot 'target/debug/muse-desktop.exe'
$appArgs = @('--data-dir', $fixtureRoot)
$appProcess = $null
function Open-WindowFixture {
    $process = & (Join-Path $PSScriptRoot 'start-desktop.ps1') -Binary $binary -AppArguments $appArgs -WorkingDirectory $projectRoot
    $null = $process.WaitForInputIdle(10000)
    $deadline = (Get-Date).AddSeconds(10)
    do {
        $process.Refresh()
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) { return $process }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $deadline)
    throw 'Fixture window did not appear.'
}
function Read-WindowPreference {
    $deadline = (Get-Date).AddSeconds(6)
    do {
        $path = Join-Path $fixtureRoot 'window.json'
        if (Test-Path -LiteralPath $path) {
            try { return (Get-Content -LiteralPath $path -Raw | ConvertFrom-Json) } catch { }
        }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $deadline)
    throw 'Window preference was not written.'
}
try {
    $appProcess = Open-WindowFixture
    Start-Sleep -Milliseconds 500
    $scale = [MuseWindowRegression]::GetDpiForWindow($appProcess.MainWindowHandle) / 96.0
    $caption = [MuseWindowRegression]::HitTest($appProcess.MainWindowHandle,[int](500*$scale),[int](18*$scale))
    $button = [MuseWindowRegression]::HitTest($appProcess.MainWindowHandle,[int](22*$scale),[int](18*$scale))
    if ($caption -ne 2 -or $button -ne 1) { throw "Titlebar hit test failed: caption=$caption; button=$button" }
    & (Join-Path $PSScriptRoot 'check-drag.ps1') -ProcessId $appProcess.Id -ReportPath (Join-Path $fixtureRoot 'drag-result.json')
    . (Join-Path $PSScriptRoot 'desktop-input.ps1')
    $controlsClient = New-Object MuseWindowRegression+RECT
    $null=[MuseWindowRegression]::GetClientRect($appProcess.MainWindowHandle,[ref]$controlsClient)
    $clientWidth=$controlsClient.R/$scale
    foreach($offset in 23,69,115){
        if([MuseWindowRegression]::HitTest($appProcess.MainWindowHandle,[int](($clientWidth-$offset)*$scale),[int](18*$scale)) -ne 1){throw 'A Windows control incorrectly starts a caption drag.'}
    }
    Invoke-MuseClick $appProcess.MainWindowHandle ($clientWidth-69) 18
    if(-not [MuseUiInput]::IsZoomed($appProcess.MainWindowHandle)){throw 'Maximize button did not maximize.'}
    $null=[MuseWindowRegression]::GetClientRect($appProcess.MainWindowHandle,[ref]$controlsClient)
    Invoke-MuseClick $appProcess.MainWindowHandle ($controlsClient.R/$scale-69) 18
    if([MuseUiInput]::IsZoomed($appProcess.MainWindowHandle)){throw 'Restore button did not restore.'}
    $null=[MuseWindowRegression]::GetClientRect($appProcess.MainWindowHandle,[ref]$controlsClient)
    Invoke-MuseClick $appProcess.MainWindowHandle ($controlsClient.R/$scale-115) 18
    if(-not [MuseUiInput]::IsIconic($appProcess.MainWindowHandle)){throw 'Minimize button did not minimize.'}
    $null=[MuseUiInput]::ShowWindow($appProcess.MainWindowHandle,9)
    $null=[MuseWindowRegression]::SetWindowPos($appProcess.MainWindowHandle,[IntPtr]::Zero,160,100,[int](1216*$scale),[int](789*$scale),0x14)
    Start-Sleep -Milliseconds 900
    $normal = Read-WindowPreference
    $firstClient = New-Object MuseWindowRegression+RECT
    $null=[MuseWindowRegression]::GetClientRect($appProcess.MainWindowHandle,[ref]$firstClient)
    $firstRect = New-Object MuseWindowRegression+RECT
    $null=[MuseWindowRegression]::GetWindowRect($appProcess.MainWindowHandle,[ref]$firstRect)
    $null=$appProcess.CloseMainWindow()
    if (-not $appProcess.WaitForExit(10000)) { throw 'Fixture close failed.' }
    $appProcess = Open-WindowFixture
    Start-Sleep -Milliseconds 500
    $restoredClient = New-Object MuseWindowRegression+RECT
    $null=[MuseWindowRegression]::GetClientRect($appProcess.MainWindowHandle,[ref]$restoredClient)
    $restoredRect = New-Object MuseWindowRegression+RECT
    $null=[MuseWindowRegression]::GetWindowRect($appProcess.MainWindowHandle,[ref]$restoredRect)
    $sizeRestored = [math]::Abs(($firstClient.R-$firstClient.L)-($restoredClient.R-$restoredClient.L)) -le 1 -and [math]::Abs(($firstClient.B-$firstClient.T)-($restoredClient.B-$restoredClient.T)) -le 1
    $positionRestored = [math]::Abs($firstRect.L-$restoredRect.L) -le 1 -and [math]::Abs($firstRect.T-$restoredRect.T) -le 1
    if (-not $sizeRestored -or -not $positionRestored) { throw 'Native geometry did not restore.' }
    $null=[MuseWindowRegression]::ShowWindow($appProcess.MainWindowHandle,3)
    Start-Sleep -Milliseconds 900
    $maximized=Read-WindowPreference
    if (-not $maximized.maximized -or $maximized.width -ne $normal.width -or $maximized.height -ne $normal.height) { throw 'Maximized state lost normal restore size.' }
    $null=[MuseWindowRegression]::ShowWindow($appProcess.MainWindowHandle,6)
    Start-Sleep -Milliseconds 600
    $minimized=Read-WindowPreference
    if ($minimized.width -ne $normal.width -or $minimized.height -ne $normal.height) { throw 'Minimize corrupted restore size.' }
    $null=[MuseWindowRegression]::ShowWindow($appProcess.MainWindowHandle,9)
    $null=$appProcess.CloseMainWindow()
    if (-not $appProcess.WaitForExit(10000)) { throw 'Fixture close failed.' }
    $appProcess=Open-WindowFixture
    Start-Sleep -Milliseconds 500
    $reopenedMax=Read-WindowPreference
    if (-not $reopenedMax.maximized) { throw 'Maximized state did not restore.' }
    $report=[pscustomobject]@{passed=$true; windows_control_mouse_clicks_verified=$true; real_mouse_drag_verified=$true; real_caption_double_click_verified=$true; real_maximized_drag_verified=$true; caption_drag_hit_test=($caption -eq 2); window_buttons_remain_client=($button -eq 1); native_client_size_restored=$sizeRestored; native_position_restored=$positionRestored; maximized_restore_size_preserved=$true; minimize_preserves_geometry=$true; maximized_state_restored=$true}
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $fixtureRoot 'window-result.json')
    $report | ConvertTo-Json
    Write-Output "Report: $fixtureRoot/window-result.json"
} finally {
    if ($appProcess -and -not $appProcess.HasExited) {
        $null=$appProcess.CloseMainWindow()
        if (-not $appProcess.WaitForExit(5000)) { $appProcess.Kill() }
    }
}
