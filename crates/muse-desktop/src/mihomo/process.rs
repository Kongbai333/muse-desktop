use std::{
    ffi::{OsStr, c_void},
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Security::SECURITY_ATTRIBUTES,
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

pub struct Core {
    job: OwnedHandle,
    process: OwnedHandle,
    home: std::path::PathBuf,
    certificate: Arc<AtomicU8>,
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn checked(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

struct Attributes(LPPROC_THREAD_ATTRIBUTE_LIST);
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.0) };
    }
}

impl Core {
    pub fn spawn(exe: &Path, home: &Path, config: &[u8]) -> io::Result<Self> {
        let (core, output) = Self::launch(exe, home, config, false)?;
        let status = core.certificate.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                match line.as_str() {
                    "MUSE_TLS_NAME" => status.store(1, Ordering::Relaxed),
                    "MUSE_TLS_TRUST" => status.store(2, Ordering::Relaxed),
                    "MUSE_DIAL_OK" => status.store(0, Ordering::Relaxed),
                    _ => (),
                }
            }
        });
        Ok(core)
    }
    pub fn certificate_error(&self) -> Option<&'static str> {
        match self.certificate.load(Ordering::Relaxed) {
            1 => Some("固定节点 TLS 握手失败：证书名称不匹配；未使用其他出口"),
            2 => Some("固定节点 TLS 握手失败：证书不受信任或无效；未使用其他出口"),
            _ => None,
        }
    }
    pub fn provider(exe: &Path, config: &[u8]) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
        let (core, output) = Self::launch(exe, Path::new(""), config, true)?;
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut data = zeroize::Zeroizing::new(Vec::new());
            let read = output.take(8 * 1024 * 1024 + 1).read_to_end(&mut data);
            let _ = send.send(read.map(|_| data));
        });
        let data = receive
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| io::ErrorKind::TimedOut)??;
        let mut code = 1;
        unsafe {
            if WaitForSingleObject(core.process.as_raw_handle(), 1000) != WAIT_OBJECT_0
                || GetExitCodeProcess(core.process.as_raw_handle(), &mut code) == 0
                || code != 0
                || data.len() > 8 * 1024 * 1024
            {
                return Err(io::ErrorKind::InvalidData.into());
            }
        }
        Ok(data)
    }
    fn launch(exe: &Path, home: &Path, config: &[u8], provider: bool) -> io::Result<(Self, File)> {
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = OwnedHandle::from_raw_handle(job);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            checked(SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ))?;
            let security = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: null_mut(),
                bInheritHandle: 1,
            };
            let (mut reader, mut writer) = (null_mut(), null_mut());
            checked(CreatePipe(&mut reader, &mut writer, &security, 0))?;
            let reader = OwnedHandle::from_raw_handle(reader);
            let mut writer = File::from_raw_handle(writer);
            checked(SetHandleInformation(
                writer.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                0,
            ))?;
            let sink = OpenOptions::new().write(true).open("NUL")?;
            let (mut output, mut child_out) = (null_mut(), null_mut());
            checked(CreatePipe(&mut output, &mut child_out, &security, 0))?;
            let output = File::from_raw_handle(output);
            let child_out = OwnedHandle::from_raw_handle(child_out);
            checked(SetHandleInformation(
                output.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                0,
            ))?;
            checked(SetHandleInformation(
                sink.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            ))?;
            let mut bytes = 0;
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let attributes = storage.as_mut_ptr().cast();
            checked(InitializeProcThreadAttributeList(
                attributes, 2, 0, &mut bytes,
            ))?;
            let attributes = Attributes(attributes);
            let jobs = [job.as_raw_handle()];
            let handles = [
                reader.as_raw_handle(),
                sink.as_raw_handle(),
                child_out.as_raw_handle(),
            ];
            for (key, list) in [
                (PROC_THREAD_ATTRIBUTE_JOB_LIST, jobs.as_slice()),
                (PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles.as_slice()),
            ] {
                checked(UpdateProcThreadAttribute(
                    attributes.0,
                    0,
                    key as usize,
                    list.as_ptr().cast(),
                    std::mem::size_of_val(list),
                    null_mut(),
                    null(),
                ))?;
            }
            let mut startup: STARTUPINFOEXW = zeroed();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = reader.as_raw_handle();
            startup.StartupInfo.hStdOutput = child_out.as_raw_handle();
            startup.StartupInfo.hStdError = sink.as_raw_handle();
            startup.lpAttributeList = attributes.0;
            let mut info: PROCESS_INFORMATION = zeroed();
            let mut command = std::ffi::OsString::from("mihomo -f - -d \"");
            command.push(home);
            command.push("\"");
            if provider {
                command = "mihomo muse-provider".into();
            }
            let mut environment = std::ffi::OsString::from("SystemRoot=");
            environment.push(std::env::var_os("SystemRoot").ok_or(io::ErrorKind::NotFound)?);
            let mut environment = wide(&environment);
            if !provider {
                environment.extend(wide(OsStr::new("MUSE_FIXED_ACCOUNT=1")));
            }
            environment.push(0);
            checked(CreateProcessW(
                wide(exe.as_os_str()).as_ptr(),
                wide(&command).as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                wide(exe.parent().ok_or(io::ErrorKind::NotFound)?.as_os_str()).as_ptr(),
                &startup.StartupInfo,
                &mut info,
            ))?;
            drop(OwnedHandle::from_raw_handle(info.hThread));
            let core = Self {
                job,
                process: OwnedHandle::from_raw_handle(info.hProcess),
                home: home.into(),
                certificate: Default::default(),
            };
            drop(reader);
            writer.write_all(config)?;
            Ok((core, output))
        }
    }
    pub fn running(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) == WAIT_TIMEOUT }
    }
    pub fn loopback_port(&self) -> Option<u16> {
        use windows_sys::Win32::NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
        };
        unsafe {
            let mut size = 0;
            if GetExtendedTcpTable(null_mut(), &mut size, 0, 2, TCP_TABLE_OWNER_PID_LISTENER, 0)
                != ERROR_INSUFFICIENT_BUFFER
                || size > 8 * 1024 * 1024
            {
                return None;
            }
            let mut table = vec![0u32; (size as usize).div_ceil(4)];
            if GetExtendedTcpTable(
                table.as_mut_ptr().cast(),
                &mut size,
                0,
                2,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            ) != NO_ERROR
            {
                return None;
            }
            let (&count, data) = table.split_first()?;
            if count as usize > std::mem::size_of_val(data) / size_of::<MIB_TCPROW_OWNER_PID>() {
                return None;
            }
            let pid = GetProcessId(self.process.as_raw_handle());
            let rows = std::slice::from_raw_parts(
                data.as_ptr().cast::<MIB_TCPROW_OWNER_PID>(),
                count as usize,
            );
            let mut owned = rows.iter().filter(|row| row.dwOwningPid == pid);
            let row = owned.next()?;
            (owned.next().is_none() && row.dwLocalAddr.to_ne_bytes() == [127, 0, 0, 1])
                .then_some(u16::from_be(row.dwLocalPort as u16))
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            TerminateJobObject(self.job.as_raw_handle(), 0);
            WaitForSingleObject(self.process.as_raw_handle(), 2000);
        }
        if !self.home.as_os_str().is_empty() {
            let _ = std::fs::remove_file(self.home.join("cache.db"));
            let _ = std::fs::remove_dir(&self.home);
        }
    }
}
