param(
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string[]]$AppArguments,
    [Parameter(Mandatory)][string]$WorkingDirectory
)
$ErrorActionPreference = 'Stop'
if (-not ('MuseDesktopLaunch' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class MuseDesktopLaunch {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct STARTUPINFO {
        public uint cb;
        public string reserved, desktop, title;
        public uint x,y,width,height,xChars,yChars,fill,flags;
        public ushort show,reservedSize;
        public IntPtr reservedBytes,input,output,error;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct STARTUPINFOEX { public STARTUPINFO startup; public IntPtr attributes; }
    [StructLayout(LayoutKind.Sequential)]
    private struct PROCESS_INFORMATION { public IntPtr process,thread; public uint id,threadId; }
    [DllImport("kernel32.dll",SetLastError=true)]
    private static extern IntPtr OpenProcess(uint access,bool inherit,uint id);
    [DllImport("kernel32.dll",SetLastError=true)]
    private static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,uint flags,ref IntPtr size);
    [DllImport("kernel32.dll",SetLastError=true)]
    private static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,IntPtr size,IntPtr previous,IntPtr returned);
    [DllImport("kernel32.dll")]
    private static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
    private static extern bool CreateProcessW(string app,StringBuilder command,IntPtr processSecurity,IntPtr threadSecurity,bool inheritHandles,uint flags,IntPtr environment,string directory,ref STARTUPINFOEX startup,out PROCESS_INFORMATION process);
    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll",SetLastError=true)]
    public static extern bool IsProcessInJob(IntPtr process,IntPtr job,out bool result);

    // Accept raw argument values and apply Windows' argv quotation rules.
    private static string Quote(string value) {
        var result=new StringBuilder("\"");
        int slashes=0;
        foreach(char ch in value) {
            if(ch=='\\') { slashes++; continue; }
            if(ch=='"') result.Append('\\',slashes*2+1).Append(ch);
            else result.Append('\\',slashes).Append(ch);
            slashes=0;
        }
        return result.Append('\\',slashes*2).Append('"').ToString();
    }
    public static uint Start(uint parentId,string executable,string[] arguments,string directory) {
        IntPtr parent=OpenProcess(0x1080,false,parentId);
        if(parent==IntPtr.Zero) throw new InvalidOperationException("Win32 error " + Marshal.GetLastWin32Error());
        IntPtr size=IntPtr.Zero,list=IntPtr.Zero,parentValue=IntPtr.Zero;
        bool initialized=false;
        try {
            InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref size);
            list=Marshal.AllocHGlobal(size);
            if(!InitializeProcThreadAttributeList(list,1,0,ref size)) throw new InvalidOperationException("Win32 error " + Marshal.GetLastWin32Error());
            initialized=true;
            parentValue=Marshal.AllocHGlobal(IntPtr.Size);
            Marshal.WriteIntPtr(parentValue,parent);
            if(!UpdateProcThreadAttribute(list,0,new IntPtr(0x20000),parentValue,new IntPtr(IntPtr.Size),IntPtr.Zero,IntPtr.Zero)) throw new InvalidOperationException("Win32 error " + Marshal.GetLastWin32Error());
            var startup=new STARTUPINFOEX();
            startup.startup.cb=(uint)Marshal.SizeOf(typeof(STARTUPINFOEX));
            startup.startup.flags=1;
            startup.startup.show=1;
            startup.attributes=list;
            var command=new StringBuilder(Quote(executable));
            foreach(string argument in arguments) command.Append(' ').Append(Quote(argument));
            PROCESS_INFORMATION process;
            // Extended startup information, breakaway from the task job, new group.
            if(!CreateProcessW(executable,command,IntPtr.Zero,IntPtr.Zero,false,0x01080200,IntPtr.Zero,directory,ref startup,out process)) throw new InvalidOperationException("Win32 error " + Marshal.GetLastWin32Error());
            try { return process.id; }
            finally { CloseHandle(process.thread); CloseHandle(process.process); }
        } finally {
            if(parentValue!=IntPtr.Zero) Marshal.FreeHGlobal(parentValue);
            if(initialized) DeleteProcThreadAttributeList(list);
            if(list!=IntPtr.Zero) Marshal.FreeHGlobal(list);
            CloseHandle(parent);
        }
    }
}
'@
}
$interactiveSession = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
$desktopParent = Get-Process -Name explorer -ErrorAction Stop | Where-Object {
    $inJob = $true
    $_.SessionId -eq $interactiveSession -and
        [MuseDesktopLaunch]::IsProcessInJob($_.Handle,[IntPtr]::Zero,[ref]$inJob) -and -not $inJob
} | Select-Object -First 1
if (-not $desktopParent) { throw 'No independent interactive Windows Explorer process is available. Open 启动 Muse.lnk from Explorer.' }
$desktopProcessId = [MuseDesktopLaunch]::Start($desktopParent.Id,$Binary,$AppArguments,$WorkingDirectory)
$desktopProcess = [System.Diagnostics.Process]::GetProcessById($desktopProcessId)
$inTaskJob = $true
if (-not [MuseDesktopLaunch]::IsProcessInJob($desktopProcess.Handle,[IntPtr]::Zero,[ref]$inTaskJob) -or $inTaskJob) {
    throw 'Muse was started but independent process lifetime could not be verified.'
}
$desktopProcess
