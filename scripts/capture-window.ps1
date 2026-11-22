param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$OutputPath)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
if(-not ('MuseWindowCapture' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MuseWindowCapture {
    private delegate bool EnumProc(IntPtr h,IntPtr p);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc f,IntPtr p);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h);
    public static IntPtr Find(uint id) {
        IntPtr found=IntPtr.Zero;
        long largest=0;
        EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);
            if(owner==id && IsWindowVisible(h)) {
                RECT rect;
                if(GetWindowRect(h,out rect)) {
                    long area=(long)(rect.R-rect.L)*(rect.B-rect.T);
                    if(area>largest){found=h;largest=area;}
                }
            }return true;},IntPtr.Zero);
        return found;
    }
    [StructLayout(LayoutKind.Sequential)] public struct RECT {public int L,T,R,B;}
    [StructLayout(LayoutKind.Sequential)] public struct POINT {public int X,Y;}
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
}
'@
}
$process=Get-Process -Id $ProcessId
if($process.ProcessName -ne 'muse-desktop'){throw 'Only Muse Desktop can be captured by this script.'}
$hwnd=[MuseWindowCapture]::Find($ProcessId)
$deadline=(Get-Date).AddSeconds(3)
while($hwnd -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline){
    Start-Sleep -Milliseconds 100
    $process.Refresh()
    $hwnd=[MuseWindowCapture]::Find($ProcessId)
}
if($hwnd -eq [IntPtr]::Zero){throw 'Muse window is missing.'}
$rect=New-Object MuseWindowCapture+RECT
$client=New-Object MuseWindowCapture+RECT
$origin=New-Object MuseWindowCapture+POINT
$null=[MuseWindowCapture]::GetWindowRect($hwnd,[ref]$rect)
$null=[MuseWindowCapture]::GetClientRect($hwnd,[ref]$client)
$null=[MuseWindowCapture]::ClientToScreen($hwnd,[ref]$origin)
$bitmap=New-Object System.Drawing.Bitmap(($rect.R-$rect.L),($rect.B-$rect.T))
$graphics=[System.Drawing.Graphics]::FromImage($bitmap)
$dc=$graphics.GetHdc()
try {if(-not [MuseWindowCapture]::PrintWindow($hwnd,$dc,2)){throw 'Window capture failed.'}}
finally {$graphics.ReleaseHdc($dc);$graphics.Dispose()}
try {$bitmap.Save($OutputPath,[System.Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
[pscustomobject]@{file=$OutputPath;client_offset_x=$origin.X-$rect.L;client_offset_y=$origin.Y-$rect.T;client_width=$client.R;client_height=$client.B;scale=[MuseWindowCapture]::GetDpiForWindow($hwnd)/96.0}|ConvertTo-Json
