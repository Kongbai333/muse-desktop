# Native input helpers for the project's own GUI regression fixtures.
if(-not ('MuseUiInput' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class MuseUiInput {
    [StructLayout(LayoutKind.Sequential)] public struct POINT {public int X,Y;}
    [StructLayout(LayoutKind.Sequential)] public struct RECT {public int L,T,R,B;}
    public delegate bool EnumProc(IntPtr h,IntPtr p);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
    [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h,ref POINT p);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h,EnumProc f,IntPtr p);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f,IntPtr p);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint id);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h,StringBuilder s,int n);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint dx,uint dy,uint data,UIntPtr extra);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h,uint m,IntPtr w,IntPtr l);
    public static int BrowserLeft(IntPtr h) {
        int left=-1;
        EnumChildWindows(h,(c,p)=>{
            var name=new StringBuilder(256);GetClassNameW(c,name,256);
            if(name.ToString()=="Chrome_WidgetWin_0" && IsWindowVisible(c)) {
                RECT r;GetWindowRect(c,out r);var origin=new POINT{X=r.L,Y=r.T};ScreenToClient(h,ref origin);left=origin.X;return false;
            }return true;
        },IntPtr.Zero);return left;
    }
    public static IntPtr Menu(uint id) {
        IntPtr menu=IntPtr.Zero;
        EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);var name=new StringBuilder(256);GetClassNameW(h,name,256);
            if(owner==id && name.ToString()=="#32768"){menu=h;return false;}return true;},IntPtr.Zero);return menu;
    }
}
'@
}
function Invoke-MuseMenuItem([int]$ProcessId,[int]$Item) {
    $menu=[MuseUiInput]::Menu($ProcessId)
    if($menu -eq [IntPtr]::Zero){throw 'Account context menu is missing.'}
    $rect=New-Object MuseUiInput+RECT
    $null=[MuseUiInput]::GetWindowRect($menu,[ref]$rect)
    $original=New-Object MuseUiInput+POINT
    $null=[MuseUiInput]::GetCursorPos([ref]$original)
    try {
        $null=[MuseUiInput]::SetCursorPos(($rect.L+70),($rect.T+14+($Item-1)*29))
        Start-Sleep -Milliseconds 120
        [MuseUiInput]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
        Start-Sleep -Milliseconds 80
        [MuseUiInput]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
        Start-Sleep -Milliseconds 300
    } finally {$null=[MuseUiInput]::SetCursorPos($original.X,$original.Y)}
}
function Invoke-MuseClick([IntPtr]$WindowHandle,[double]$X,[double]$Y,[switch]$Right) {
    $scale=[MuseUiInput]::GetDpiForWindow($WindowHandle)/96.0
    $point=New-Object MuseUiInput+POINT
    $point.X=[int]($X*$scale)
    $point.Y=[int]($Y*$scale)
    $null=[MuseUiInput]::ClientToScreen($WindowHandle,[ref]$point)
    $original=New-Object MuseUiInput+POINT
    $null=[MuseUiInput]::GetCursorPos([ref]$original)
    try {
        $null=[MuseUiInput]::SetForegroundWindow($WindowHandle)
        $null=[MuseUiInput]::SetCursorPos($point.X,$point.Y)
        Start-Sleep -Milliseconds 120
        $down=if($Right){8}else{2}
        $up=if($Right){16}else{4}
        [MuseUiInput]::mouse_event($down,0,0,0,[UIntPtr]::Zero)
        Start-Sleep -Milliseconds 80
        [MuseUiInput]::mouse_event($up,0,0,0,[UIntPtr]::Zero)
        Start-Sleep -Milliseconds 220
    } finally {$null=[MuseUiInput]::SetCursorPos($original.X,$original.Y)}
}
function Invoke-MuseDrag([IntPtr]$WindowHandle,[double]$FromX,[double]$ToX,[double]$Y) {
    $scale=[MuseUiInput]::GetDpiForWindow($WindowHandle)/96.0
    $point=New-Object MuseUiInput+POINT
    $point.X=[int]($FromX*$scale);$point.Y=[int]($Y*$scale)
    $null=[MuseUiInput]::ClientToScreen($WindowHandle,[ref]$point)
    $original=New-Object MuseUiInput+POINT
    $null=[MuseUiInput]::GetCursorPos([ref]$original)
    try {
        $null=[MuseUiInput]::SetForegroundWindow($WindowHandle)
        $null=[MuseUiInput]::SetCursorPos($point.X,$point.Y)
        Start-Sleep -Milliseconds 150
        [MuseUiInput]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
        Start-Sleep -Milliseconds 120
        foreach($step in 1..10) {
            $null=[MuseUiInput]::SetCursorPos([int]($point.X+($ToX-$FromX)*$scale*$step/10),$point.Y)
            Start-Sleep -Milliseconds 60
        }
    } finally {
        [MuseUiInput]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
        $null=[MuseUiInput]::SetCursorPos($original.X,$original.Y)
    }
    Start-Sleep -Milliseconds 400
}
