//! A same-user helper transaction. Never terminates another process, elevates,
//! merges browser profiles or replaces an entire Windows installation folder.

use crate::{
    updater::{self, Result},
    updater_download::{self, Prepared},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

static LAUNCH_DIRECTORY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
pub fn remember_launch_directory(path: PathBuf) {
    let _ = LAUNCH_DIRECTORY.set(path);
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AppFile {
    pub relative: PathBuf,
    pub sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallPlan {
    version: String,
    platform: String,
    parent_pid: u32,
    current_exe: PathBuf,
    install: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    data_dir: PathBuf,
    working_dir: PathBuf,
    arguments: Vec<String>,
    files: Vec<AppFile>,
    background: bool,
}
fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|_| "无法核验应用文件")?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0_u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| "无法核验应用文件")?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn no_links(path: &Path, bound: &Path) -> Result<()> {
    let rel = path.strip_prefix(bound).map_err(|_| "安装路径越界")?;
    let mut current = bound.to_path_buf();
    for part in rel.components() {
        current.push(part);
        if let Ok(meta) = fs::symlink_metadata(&current) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("安装路径不允许连接点".into());
                }
            }
            if meta.file_type().is_symlink() {
                return Err("安装路径不允许链接".into());
            }
        }
    }
    Ok(())
}
fn inventory(root: &Path, current: &Path, files: &mut Vec<AppFile>) -> Result<()> {
    for entry in fs::read_dir(current).map_err(|_| "无法读取更新文件")? {
        let entry = entry.map_err(|_| "无法读取更新文件")?;
        let path = entry.path();
        no_links(&path, root)?;
        if files.len() >= 8192 {
            return Err("更新文件数量异常".into());
        }
        if path.is_dir() {
            inventory(root, &path, files)?;
        } else if path.is_file() {
            files.push(AppFile {
                relative: path
                    .strip_prefix(root)
                    .map_err(|_| "更新路径无效")?
                    .to_path_buf(),
                sha256: hash_file(&path)?,
            });
        } else {
            return Err("更新包包含特殊文件".into());
        }
    }
    Ok(())
}

/// The extraction directory is never moved across volumes into an installation.
/// Copy and verify everything on the target volume before the commit renames.
pub fn replace_files(
    install: &Path,
    staged: &Path,
    backup: &Path,
    files: &[AppFile],
    launch: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let incoming = install.join(format!(".muse-update-incoming-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&incoming).map_err(|_| "应用目录不可写，请换到可写目录后更新")?;
    let mut changed: Vec<(PathBuf, bool)> = Vec::new();
    let result = (|| -> Result<()> {
        // Preflight every member before touching any installed file.
        for item in files {
            let text = item
                .relative
                .to_str()
                .ok_or("更新路径编码无效")?
                .replace('\\', "/");
            updater_download::safe_path(&text)?;
            if !updater_download::windows_app_file(&item.relative) {
                return Err("更新包包含非应用文件".into());
            }
            let target = install.join(&item.relative);
            let source = staged.join(&item.relative);
            no_links(&target, install)?;
            no_links(&source, staged)?;
            if target.exists() && !target.is_file() {
                return Err("安装文件与现有目录冲突".into());
            }
            if hash_file(&source)? != item.sha256 {
                return Err("安装前更新文件校验失败".into());
            }
            let next = incoming.join(&item.relative);
            fs::create_dir_all(next.parent().ok_or("更新路径无效")?)
                .map_err(|_| "无法准备应用更新")?;
            fs::copy(&source, &next).map_err(|_| "无法准备应用更新")?;
            if hash_file(&next)? != item.sha256 {
                return Err("暂存应用校验失败".into());
            }
        }
        fs::create_dir(backup).map_err(|_| "无法创建更新回退目录")?;
        for item in files {
            let target = install.join(&item.relative);
            let old = backup.join(&item.relative);
            fs::create_dir_all(target.parent().ok_or("更新路径无效")?)
                .map_err(|_| "无法创建应用目录")?;
            let existed = target.exists();
            if existed {
                fs::create_dir_all(old.parent().ok_or("更新路径无效")?)
                    .map_err(|_| "无法创建回退目录")?;
                fs::rename(&target, &old)
                    .map_err(|_| "应用文件仍被占用，旧版保持可用，请关闭相关实例后重试")?;
            }
            changed.push((item.relative.clone(), existed));
            fs::rename(incoming.join(&item.relative), &target).map_err(|_| "应用更新写入失败")?;
        }
        launch()
    })();
    if result.is_err() {
        let mut rollback_ok = true;
        for (relative, existed) in changed.iter().rev() {
            let target = install.join(relative);
            if target.is_file() && fs::remove_file(&target).is_err() {
                rollback_ok = false;
                continue;
            }
            if *existed && fs::rename(backup.join(relative), &target).is_err() {
                rollback_ok = false;
            }
        }
        if !rollback_ok {
            return Err("更新失败，自动回退未完成。旧应用文件保留在回退目录，请勿删除".into());
        }
    }
    let _ = fs::remove_dir_all(&incoming);
    result
}

fn validate_binary(path: &Path, platform: &str) -> Result<()> {
    let mut file = fs::File::open(path).map_err(|_| "更新程序不存在")?;
    let mut header = [0_u8; 64];
    file.read_exact(&mut header)
        .map_err(|_| "更新程序格式无效")?;
    if platform == "win-x64" {
        if &header[..2] != b"MZ" {
            return Err("更新程序不是Windows应用".into());
        }
        let offset = u32::from_le_bytes(header[60..64].try_into().unwrap());
        if offset > 1024 * 1024 {
            return Err("更新程序头无效".into());
        }
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|_| "更新程序头无效")?;
        let mut pe = [0_u8; 6];
        file.read_exact(&mut pe).map_err(|_| "更新程序头无效")?;
        if &pe[..4] != b"PE\0\0" || pe[4..6] != [0x64, 0x86] {
            return Err("更新程序架构不匹配".into());
        }
    } else {
        return Err("暂不支持此架构更新".into());
    }
    Ok(())
}

fn normalized_arguments(
    arguments: Vec<String>,
    cwd: &Path,
    data_dir: &Path,
) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut source = arguments.into_iter();
    while let Some(arg) = source.next() {
        if arg == "--update-health-file" {
            let _ = source.next();
        } else if arg != "--update-background" {
            result.push(arg);
        }
    }
    let mut found = false;
    let mut i = 0;
    while i < result.len() {
        if result[i] == "--data-dir" {
            let value = result.get_mut(i + 1).ok_or("原启动参数无效")?;
            let path = PathBuf::from(&*value);
            let path = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };
            *value = path.to_str().ok_or("数据目录编码无效")?.to_owned();
            found = true;
            i += 1;
        }
        i += 1;
    }
    if !found {
        result.extend([
            "--data-dir".into(),
            data_dir.to_str().ok_or("数据目录编码无效")?.to_owned(),
        ]);
    }
    Ok(result)
}

pub enum LaunchOutcome {
    Started(PathBuf),
}
struct CheckedInstall {
    current_exe: PathBuf,
    cwd: PathBuf,
    data_dir: PathBuf,
    install: PathBuf,
    backup_parent: PathBuf,
    files: Vec<AppFile>,
}
fn checked_data_root(install: &Path, data: &Path) -> Result<PathBuf> {
    fs::canonicalize(install).map_err(|_| "无法核验原应用目录")?;
    let data = fs::canonicalize(data).map_err(|_| "无法核验原数据目录，未安装更新")?;
    Ok(data)
}
// Read-only safety gate before the Windows application-file transaction.
fn readonly_preflight(prepared: &Prepared, data_dir: &Path) -> Result<CheckedInstall> {
    let current_exe = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|_| "无法确定当前应用路径")?;
    let cwd = LAUNCH_DIRECTORY
        .get()
        .cloned()
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)
        .map_err(|_| "无法确定启动目录")?;
    let data = if data_dir.is_absolute() {
        data_dir.to_path_buf()
    } else {
        cwd.join(data_dir)
    };
    if current_exe.file_name().is_none_or(|s| s != "Muse.exe") {
        return Err("请从解压后的正式Muse.exe运行更新".into());
    }
    let install = current_exe.parent().ok_or("应用路径无效")?.to_path_buf();
    let data_dir = checked_data_root(&install, &data)?;
    let mut files = Vec::new();
    inventory(&prepared.payload, &prepared.payload, &mut files)?;
    #[cfg(windows)]
    validate_binary(
        &prepared.payload.join("Muse.exe"),
        &prepared.update.platform,
    )?;
    for file in &files {
        if data_dir != install && install.join(&file.relative).starts_with(&data_dir) {
            return Err("更新文件与数据目录重叠，已停止安装".into());
        }
    }
    let backup_parent = install.clone();
    Ok(CheckedInstall {
        current_exe,
        cwd,
        data_dir,
        install,
        backup_parent,
        files,
    })
}
fn permission_only(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
    )
}
fn writable(parent: &Path) -> Result<bool> {
    let path = parent.join(format!(".muse-update-write-{}", uuid::Uuid::new_v4()));
    match fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
    {
        Ok(file) => {
            drop(file);
            fs::remove_file(path).map_err(|_| "无法清理写入权限检查文件")?;
            Ok(true)
        }
        Err(error) if permission_only(error.kind()) => Ok(false),
        Err(_) => Err("无法准备应用更新，请检查安装位置和磁盘空间".into()),
    }
}
fn controlled_stage(attempt: &Path) -> Result<PathBuf> {
    let attempt = fs::canonicalize(attempt).map_err(|_| "更新尝试目录不存在")?;
    let stage = attempt.parent().ok_or("更新尝试目录无效")?;
    let valid_id = |path: &Path, prefix: &str| {
        path.file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix(prefix))
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
    };
    if !valid_id(&attempt, "attempt-")
        || !valid_id(stage, "muse-update-")
        || stage.parent()
            != Some(
                fs::canonicalize(std::env::temp_dir())
                    .map_err(|_| "临时目录无效")?
                    .as_path(),
            )
    {
        return Err("更新尝试不在受控临时目录".into());
    }
    Ok(stage.to_path_buf())
}
fn new_attempt(root: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root).map_err(|_| "更新暂存目录不存在")?;
    let attempt = root.join(format!("attempt-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&attempt).map_err(|_| "无法创建新的更新尝试")?;
    controlled_stage(&attempt)?;
    Ok(attempt)
}

pub fn launch_helper(
    prepared: &mut Prepared,
    data_dir: &Path,
    background: bool,
) -> Result<LaunchOutcome> {
    #[cfg(windows)]
    let background = {
        let _ = background;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        }
        pid != std::process::id()
    };
    let checked = readonly_preflight(prepared, data_dir)?;
    if !writable(&checked.backup_parent)? {
        #[cfg(windows)]
        return Err(
            "应用位置不可写，请将应用解压到有写入权限的目录后更新；账号数据目录应保留".into(),
        );
    }
    let arguments = normalized_arguments(
        std::env::args().skip(1).collect(),
        &checked.cwd,
        &checked.data_dir,
    )?;
    let plan = InstallPlan {
        version: prepared.update.version.clone(),
        platform: prepared.update.platform.clone(),
        parent_pid: std::process::id(),
        current_exe: checked.current_exe.clone(),
        install: checked.install,
        staged: prepared.payload.clone(),
        backup: checked
            .backup_parent
            .join(format!(".muse-update-backup-{}", uuid::Uuid::new_v4())),
        data_dir: checked.data_dir,
        working_dir: checked.cwd,
        arguments,
        files: checked.files,
        background,
    };
    // Every try has new plan/ready/authorization/health files. A timed-out old
    // helper can only see its own missing authorization and cannot commit later.
    let attempt = new_attempt(&prepared.root)?;
    let path = attempt.join("install.json");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|_| "无法准备更新安装")?;
    file.write_all(&serde_json::to_vec(&plan).map_err(|_| "无法准备更新安装")?)
        .map_err(|_| "无法准备更新安装")?;
    file.sync_all().map_err(|_| "无法保存安装计划")?;
    drop(file);
    let helper = attempt.join("Muse-update-helper.exe");
    fs::copy(&checked.current_exe, &helper).map_err(|_| "无法准备更新助手")?;
    let mut command = Command::new(helper);
    command.arg("--apply-update").arg(&path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| "无法启动更新助手")?;
    prepared.keep = true; // An unconfirmed live attempt must retain its own files.
    let start = Instant::now();
    loop {
        if attempt.join("helper-ready").is_file() {
            prepared.keep = true;
            return Ok(LaunchOutcome::Started(attempt));
        }
        if child
            .try_wait()
            .map_err(|_| "无法确认更新助手状态")?
            .is_some()
        {
            return Err("更新助手未就绪，原应用保持运行，可重试创建新的安装尝试".into());
        }
        if start.elapsed() > Duration::from_secs(30) {
            prepared.keep = true;
            return Err("更新助手响应超时，旧尝试未授权。可重试创建新的安装尝试".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(windows)]
fn wait_parent(plan: &InstallPlan, root: &Path) -> Result<()> {
    use windows::{
        Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
            },
        },
        core::PWSTR,
    };
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            plan.parent_pid,
        )
        .map_err(|_| "原应用进程已变化，停止更新")?;
        let result = (|| {
            let mut buffer = vec![0_u16; 32768];
            let mut size = buffer.len() as u32;
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            )
            .map_err(|_| "无法核验原应用进程")?;
            let path = PathBuf::from(String::from_utf16_lossy(&buffer[..size as usize]));
            if fs::canonicalize(path).ok().as_ref() != Some(&plan.current_exe) {
                return Err("原应用进程路径不匹配".into());
            }
            fs::write(root.join("helper-ready"), b"ready").map_err(|_| "更新助手无法就绪")?;
            if WaitForSingleObject(handle, 180_000) != WAIT_OBJECT_0 {
                return Err("原应用尚未安全退出，未强制关闭进程或安装更新".into());
            }
            Ok(())
        })();
        let _ = CloseHandle(handle);
        result
    }
}
fn restart_command(plan: &InstallPlan) -> Command {
    #[cfg(windows)]
    let mut command = Command::new(&plan.current_exe);
    command.args(&plan.arguments).current_dir(&plan.working_dir);
    if plan.background {
        command.arg("--update-background");
    }
    command
}
fn restart(plan: &InstallPlan) -> Result<()> {
    restart_command(plan)
        .spawn()
        .map(|_| ())
        .map_err(|_| "应用无法启动".into())
}
fn restart_updated(plan: &InstallPlan, root: &Path) -> Result<()> {
    let mut command = restart_command(plan);
    command
        .arg("--update-health-file")
        .arg(root.join("application-ready"));
    let mut child = command
        .spawn()
        .map_err(|_| "新版无法启动，已尝试回退原应用")?;
    let start = Instant::now();
    loop {
        if root.join("application-ready").is_file() {
            return Ok(());
        }
        if child
            .try_wait()
            .map_err(|_| "无法确认新版启动状态")?
            .is_some()
        {
            return Err("新版启动提前退出，已尝试回退原应用".into());
        }
        // A still-running app may take longer. Never overwrite/kill it merely
        // because startup confirmation is late; retain the full rollback copy.
        if start.elapsed() > Duration::from_secs(30) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn run_helper(path: &Path) -> Result<()> {
    let path = fs::canonicalize(path).map_err(|_| "更新计划不存在")?;
    let root = path.parent().ok_or("更新目录无效")?;
    let stage = controlled_stage(root)?;
    if path.file_name().is_none_or(|s| s != "install.json") {
        return Err("更新计划路径无效".into());
    }
    if fs::metadata(&path).map_err(|_| "更新计划不存在")?.len() > 2 * 1024 * 1024 {
        return Err("更新计划过大".into());
    }
    let plan: InstallPlan =
        serde_json::from_slice(&fs::read(&path).map_err(|_| "无法读取更新计划")?)
            .map_err(|_| "更新计划无效")?;
    let staged = fs::canonicalize(&plan.staged).map_err(|_| "暂存更新不存在")?;
    let install = fs::canonicalize(&plan.install).map_err(|_| "原应用不存在")?;
    let backup_parent = &install;
    if plan.platform != updater::platform()
        || !staged.starts_with(stage.join("extracted"))
        || install != plan.install
        || plan.backup.parent() != Some(backup_parent)
        || !plan
            .backup
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with(".muse-update-backup-"))
        || plan.backup.exists()
        || plan.files.is_empty()
        || plan.files.len() > 8192
    {
        return Err("安装计划路径或平台无效".into());
    }
    for item in &plan.files {
        let text = item
            .relative
            .to_str()
            .ok_or("更新路径无效")?
            .replace('\\', "/");
        updater_download::safe_path(&text)?;
        no_links(&staged.join(&item.relative), &staged)?;
        if hash_file(&staged.join(&item.relative))? != item.sha256 {
            return Err("安装前校验失败".into());
        }
    }
    #[cfg(windows)]
    {
        if plan.current_exe != install.join("Muse.exe") {
            return Err("目标程序路径不匹配".into());
        }
        validate_binary(&staged.join("Muse.exe"), &plan.platform)?;
    }
    wait_parent(&plan, root)?;
    if fs::read(root.join("install-authorized")).ok().as_deref() != Some(b"install") {
        return Err("原应用未确认安装，更新已取消".into());
    }
    #[cfg(windows)]
    let result = replace_files(&install, &staged, &plan.backup, &plan.files, || {
        restart_updated(&plan, root)
    });
    let report = serde_json::json!({"installed":result.is_ok(),"startup_confirmed":root.join("application-ready").is_file(),"error":result.as_ref().err(),"backup":plan.backup,"data_preserved":true,"background_restart":plan.background});
    let _ = fs::write(root.join("install-result.json"), report.to_string());
    let _ = crate::account_cache::write(
        &plan.data_dir,
        "update-result.json",
        report.to_string().as_bytes(),
    );
    let _ = fs::remove_file(path); // Drop persisted launch arguments after use.
    if result.is_err() {
        let _ = restart(&plan);
    }
    result
}

/// Return before constructing GPUI, profiles, crash logs or any Muse network.
pub fn entry(arguments: &[String]) -> Option<i32> {
    if arguments.get(1).is_none_or(|s| s != "--apply-update") {
        return None;
    }
    if arguments.len() != 3 {
        return Some(2);
    }
    match run_helper(Path::new(&arguments[2])) {
        Ok(()) => Some(0),
        Err(error) => {
            if let Some(root) = Path::new(&arguments[2]).parent()
                && controlled_stage(root).is_ok()
            {
                let _ = fs::write(
                    root.join("helper-error.json"),
                    serde_json::json!({"error":error}).to_string(),
                );
            }
            Some(2)
        }
    }
}

pub fn record_startup(arguments: &[String]) {
    let Some(path) = arguments
        .windows(2)
        .find(|p| p[0] == "--update-health-file")
        .map(|p| PathBuf::from(&p[1]))
    else {
        return;
    };
    let Some(root) = path.parent() else {
        return;
    };
    if path.file_name().is_some_and(|s| s == "application-ready") && controlled_stage(root).is_ok()
    {
        let _ = fs::write(path, std::process::id().to_string().as_bytes());
    }
}

/// GPUI 0.2.2 Windows ignores WindowOptions::focus when applying placement.
/// Show an updated background window explicitly without activation or input.
#[cfg(windows)]
pub fn show_background(hwnd: windows::Win32::Foundation::HWND, bounds: gpui::WindowBounds) {
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::{
        Foundation::{LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{
            GetClientRect, GetWindowRect, HWND_BOTTOM, PostMessageW, SWP_NOACTIVATE,
            SWP_SHOWWINDOW, SetWindowPos, WM_SIZE,
        },
    };
    unsafe {
        let mut outer = RECT::default();
        let mut client = RECT::default();
        let mut origin = POINT::default();
        if GetWindowRect(hwnd, &mut outer).is_err()
            || GetClientRect(hwnd, &mut client).is_err()
            || !ClientToScreen(hwnd, &mut origin).as_bool()
        {
            return;
        }
        let scale = GetDpiForWindow(hwnd) as f32 / 96.;
        let rect = bounds.get_bounds();
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            (f32::from(rect.origin.x) * scale) as i32 - (origin.x - outer.left),
            (f32::from(rect.origin.y) * scale) as i32 - (origin.y - outer.top),
            (f32::from(rect.size.width) * scale) as i32 + (outer.right - outer.left - client.right),
            (f32::from(rect.size.height) * scale) as i32
                + (outer.bottom - outer.top - client.bottom),
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        // Notify GPUI after its application callback returns; no input/focus.
        if GetClientRect(hwnd, &mut client).is_ok() {
            let _ = PostMessageW(
                Some(hwnd),
                WM_SIZE,
                WPARAM(0),
                LPARAM(
                    ((((client.bottom as u32) & 0xffff) << 16) | ((client.right as u32) & 0xffff))
                        as isize,
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_helper_attempt_never_blocks_or_authorizes_a_retry() {
        let root = std::env::temp_dir().join(format!("muse-update-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let first = new_attempt(&root).unwrap();
        fs::write(first.join("install.json"), b"failed helper plan").unwrap();
        fs::write(first.join("install-authorized"), b"install").unwrap();
        let second = new_attempt(&root).unwrap();
        assert_ne!(first, second);
        assert!(!second.join("install.json").exists());
        assert!(!second.join("install-authorized").exists());
        assert!(!second.join("helper-ready").exists());
        assert_eq!(
            controlled_stage(&second).unwrap(),
            fs::canonicalize(&root).unwrap()
        );
        fs::write(second.join("install.json"), b"retry plan").unwrap();
        assert!(first.join("install.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn only_permission_or_readonly_failure_is_classified_as_nonwritable() {
        assert!(permission_only(std::io::ErrorKind::PermissionDenied));
        assert!(permission_only(std::io::ErrorKind::ReadOnlyFilesystem));
        assert!(!permission_only(std::io::ErrorKind::NotFound));
        assert!(!permission_only(std::io::ErrorKind::StorageFull));
    }
    #[test]
    fn application_only_commit_preserves_data_and_rolls_back_failed_restart() {
        let root =
            std::env::temp_dir().join(format!("muse-installer-test-{}", uuid::Uuid::new_v4()));
        let install = root.join("install space");
        let staged = root.join("staged");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir(&staged).unwrap();
        fs::write(install.join("Muse.exe"), b"old").unwrap();
        fs::write(staged.join("Muse.exe"), b"new").unwrap();
        fs::write(install.join("workspace.json"), b"synthetic data").unwrap();
        let files = vec![AppFile {
            relative: PathBuf::from("Muse.exe"),
            sha256: crate::updater::digest(b"new"),
        }];
        let mut attempts = 0;
        let result = replace_files(&install, &staged, &root.join("rollback"), &files, || {
            attempts += 1;
            Err("synthetic launch failure".into())
        });
        assert!(result.is_err());
        assert_eq!(attempts, 1);
        assert_eq!(fs::read(install.join("Muse.exe")).unwrap(), b"old");
        assert_eq!(
            fs::read(install.join("workspace.json")).unwrap(),
            b"synthetic data"
        );
        replace_files(&install, &staged, &root.join("backup"), &files, || Ok(())).unwrap();
        assert_eq!(fs::read(install.join("Muse.exe")).unwrap(), b"new");
        assert_eq!(fs::read(root.join("backup/Muse.exe")).unwrap(), b"old");
        assert_eq!(
            fs::read(install.join("workspace.json")).unwrap(),
            b"synthetic data"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn checksum_change_and_directory_collision_leave_original_untouched() {
        let root =
            std::env::temp_dir().join(format!("muse-installer-test-{}", uuid::Uuid::new_v4()));
        let install = root.join("app");
        let staged = root.join("stage");
        fs::create_dir_all(install.join("Muse.exe")).unwrap();
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("Muse.exe"), b"new").unwrap();
        let files = vec![AppFile {
            relative: "Muse.exe".into(),
            sha256: crate::updater::digest(b"new"),
        }];
        assert!(replace_files(&install, &staged, &root.join("backup"), &files, || Ok(())).is_err());
        assert!(install.join("Muse.exe").is_dir());
        fs::remove_dir(install.join("Muse.exe")).unwrap();
        fs::write(install.join("Muse.exe"), b"old").unwrap();
        fs::write(staged.join("Muse.exe"), b"tamper").unwrap();
        assert!(
            replace_files(&install, &staged, &root.join("backup2"), &files, || Ok(())).is_err()
        );
        assert_eq!(fs::read(install.join("Muse.exe")).unwrap(), b"old");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn relaunch_preserves_unicode_working_directory_and_explicit_data_directory() {
        let cwd = PathBuf::from("C:/synthetic install/中文");
        let data = cwd.join("data");
        let args = normalized_arguments(
            vec![
                "--data-dir".into(),
                "relative data".into(),
                "--diagnostics".into(),
                "--update-health-file".into(),
                "old-health".into(),
                "--update-background".into(),
            ],
            &cwd,
            &data,
        )
        .unwrap();
        assert_eq!(
            args,
            vec![
                "--data-dir",
                cwd.join("relative data").to_str().unwrap(),
                "--diagnostics"
            ]
        );
        let args = normalized_arguments(vec!["--diagnostics".into()], &cwd, &data).unwrap();
        assert_eq!(
            args,
            vec!["--diagnostics", "--data-dir", data.to_str().unwrap()]
        );
    }
}
