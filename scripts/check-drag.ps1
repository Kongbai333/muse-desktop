param([Parameter(Mandatory)][int]$ProcessId,[string]$ReportPath)
$ErrorActionPreference='Stop'
if(-not ('MusePhysicalDragTest' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MusePhysicalDragTest {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd,out RECT rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd,out RECT rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd,ref POINT point);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd,int command);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT point);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint dx,uint dy,uint data,UIntPtr extra);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr hwnd,uint msg,IntPtr wparam,IntPtr lparam);
    public static int HitTest(IntPtr hwnd,POINT screenPoint) {
        return SendMessageW(hwnd,0x84,IntPtr.Zero,new IntPtr((screenPoint.Y<<16)|(screenPoint.X&65535))).ToInt32();
    }
}
'@
}
$process=Get-Process -Id $ProcessId -ErrorAction Stop
$process.Refresh()
if($process.ProcessName -ne 'muse-desktop'){throw 'This regression only targets Muse Desktop.'}
$hwnd=$process.MainWindowHandle
if($hwnd -eq [IntPtr]::Zero){
    $deadline=(Get-Date).AddSeconds(3)
    do {
        Start-Sleep -Milliseconds 100
        $process.Refresh()
        $hwnd=$process.MainWindowHandle
    } while($hwnd -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline)
}
if($hwnd -eq [IntPtr]::Zero){throw 'Muse window is missing.'}
$null=[MusePhysicalDragTest]::ShowWindow($hwnd,9)
$null=[MusePhysicalDragTest]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 250
$rect=New-Object MusePhysicalDragTest+RECT
$null=[MusePhysicalDragTest]::GetWindowRect($hwnd,[ref]$rect)
$client=New-Object MusePhysicalDragTest+RECT
$null=[MusePhysicalDragTest]::GetClientRect($hwnd,[ref]$client)
$scale=[MusePhysicalDragTest]::GetDpiForWindow($hwnd)/96.0
$start=New-Object MusePhysicalDragTest+POINT
$start.X=[int](($client.R-$client.L)/2)
$start.Y=[int](18*$scale)
$null=[MusePhysicalDragTest]::ClientToScreen($hwnd,[ref]$start)
$originalPointer=New-Object MusePhysicalDragTest+POINT
$null=[MusePhysicalDragTest]::GetCursorPos([ref]$originalPointer)
$coldHit=[MusePhysicalDragTest]::HitTest($hwnd,$start)
if($coldHit -ne 2){throw "Cold caption hit was $coldHit instead of HTCAPTION."}
try {
    $null=[MusePhysicalDragTest]::SetCursorPos($start.X,$start.Y)
    Start-Sleep -Milliseconds 180
    [MusePhysicalDragTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    Start-Sleep -Milliseconds 180
    for($step=1;$step -le 6;$step++) {
        $null=[MusePhysicalDragTest]::SetCursorPos(($start.X+$step*10),($start.Y+$step*5))
        Start-Sleep -Milliseconds 60
    }
} finally {
    [MusePhysicalDragTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    $null=[MusePhysicalDragTest]::SetCursorPos($originalPointer.X,$originalPointer.Y)
}
Start-Sleep -Milliseconds 250
$after=New-Object MusePhysicalDragTest+RECT
$null=[MusePhysicalDragTest]::GetWindowRect($hwnd,[ref]$after)
$dx=$after.L-$rect.L
$dy=$after.T-$rect.T
$passed=[math]::Abs($dx-60) -le 3 -and [math]::Abs($dy-30) -le 3
if(-not $passed){throw 'Actual mouse dragging did not move the window as expected.'}
Start-Sleep -Milliseconds 650
$doublePoint=New-Object MusePhysicalDragTest+POINT
$doublePoint.X=$start.X+60
$doublePoint.Y=$start.Y+30
try {
    $null=[MusePhysicalDragTest]::SetCursorPos($doublePoint.X,$doublePoint.Y)
    foreach($click in 1..2) {
        [MusePhysicalDragTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [MusePhysicalDragTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
        Start-Sleep -Milliseconds 90
    }
    Start-Sleep -Milliseconds 350
    $doubleClickMax=[MusePhysicalDragTest]::IsZoomed($hwnd)
    if(-not $doubleClickMax){throw 'Actual titlebar double-click did not maximize.'}
    Start-Sleep -Milliseconds 650
    $maxClient=New-Object MusePhysicalDragTest+RECT
    $null=[MusePhysicalDragTest]::GetClientRect($hwnd,[ref]$maxClient)
    $maxPoint=New-Object MusePhysicalDragTest+POINT
    $maxPoint.X=[int](($maxClient.R-$maxClient.L)/2)
    $maxPoint.Y=[int](18*$scale)
    $null=[MusePhysicalDragTest]::ClientToScreen($hwnd,[ref]$maxPoint)
    $null=[MusePhysicalDragTest]::SetCursorPos($maxPoint.X,$maxPoint.Y)
    Start-Sleep -Milliseconds 100
    [MusePhysicalDragTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    for($step=1;$step -le 8;$step++) {
        $null=[MusePhysicalDragTest]::SetCursorPos(($maxPoint.X+$step*8),($maxPoint.Y+$step*8))
        Start-Sleep -Milliseconds 60
    }
} finally {
    [MusePhysicalDragTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    $null=[MusePhysicalDragTest]::SetCursorPos($originalPointer.X,$originalPointer.Y)
}
Start-Sleep -Milliseconds 250
$maxDragRestored=-not [MusePhysicalDragTest]::IsZoomed($hwnd)
if(-not $maxDragRestored){throw 'Dragging a maximized caption did not restore and move.'}
$result=[pscustomobject]@{passed=$true;real_mouse_drag=$true;drag_moved_x=$dx;drag_moved_y=$dy;cold_caption_hit=$coldHit;real_double_click_maximized=$doubleClickMax;real_maximized_drag_restored=$maxDragRestored;process_id=$ProcessId}
if($ReportPath){$result|ConvertTo-Json|Set-Content -LiteralPath $ReportPath}
$result|ConvertTo-Json
