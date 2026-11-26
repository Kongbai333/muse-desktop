//! Local opt-in NDJSON. Only typed allowlisted events cross the producer boundary.
use muse_core::{AccountNetwork, BrowserIdentity};
use muse_proxy::trace::{self, Binding, Context, Event, Outcome, Source, Stage};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const MAX_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_EVENTS: u64 = 6000;
pub const MAX_FILES: usize = 4;
pub const QUEUE: usize = 512;
static NOTICE: OnceLock<String> = OnceLock::new();

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
struct Envelope {
    event: Event,
    ts_ms: u64,
    elapsed_ms: u64,
}
struct Control {
    stop: AtomicBool,
    dropped: AtomicU64,
}
pub struct Guard {
    control: Option<Arc<Control>>,
    done: Option<mpsc::Receiver<()>>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(control) = &self.control {
            trace::stop();
            control.stop.store(true, Ordering::Release);
            // Best-effort drain only; diagnostic I/O cannot delay app exit indefinitely.
            if let Some(done) = self.done.take() {
                let _ = done.recv_timeout(Duration::from_millis(250));
            }
        }
    }
}
pub fn notice() -> Option<String> {
    NOTICE.get().cloned()
}
pub fn start(root: &Path, enabled: bool) -> Guard {
    let inactive = || Guard {
        control: None,
        done: None,
    };
    if !enabled {
        return inactive();
    }
    let session = uuid::Uuid::new_v4().simple().to_string();
    let (file, path) = match create_file(root, &session) {
        Ok(pair) => pair,
        Err(()) => {
            let _ = NOTICE.set(
                "本地代理追踪未开启：目录不可写、已有 4 个活动文件或初始化忙；不影响网络".into(),
            );
            return inactive();
        }
    };
    let control = Arc::new(Control {
        stop: AtomicBool::new(false),
        dropped: AtomicU64::new(0),
    });
    let (sender, receiver) = mpsc::sync_channel::<Envelope>(QUEUE);
    let (done_tx, done_rx) = mpsc::channel();
    let writer_control = control.clone();
    if std::thread::Builder::new()
        .name("muse-proxy-trace".into())
        .spawn(move || {
            write_trace(file, receiver, &session, &writer_control);
            let _ = done_tx.send(());
        })
        .is_err()
    {
        let _ = NOTICE.set("本地代理追踪未开启：日志线程不可用；不影响网络".into());
        return inactive();
    }
    let started = Instant::now();
    let producer_control = control.clone();
    trace::install(Arc::new(move |event| {
        let envelope = Envelope {
            event,
            ts_ms: now_ms(),
            elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        };
        match sender.try_send(envelope) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                producer_control.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                trace::stop();
            }
        }
    }));
    let _ = NOTICE.set(format!(
        "本地代理追踪：{}（单文件 4 MiB / 6000 事件上限）",
        path.display()
    ));
    Guard {
        control: Some(control),
        done: Some(done_rx),
    }
}
fn own_filename(name: &str) -> bool {
    let Some(stem) = name
        .strip_prefix("proxy-trace-")
        .and_then(|s| s.strip_suffix(".ndjson"))
    else {
        return false;
    };
    let Some((stamp, id)) = stem.split_once('-') else {
        return false;
    };
    (13..=20).contains(&stamp.len())
        && stamp.bytes().all(|b| b.is_ascii_digit())
        && id.len() == 32
        && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn create_file(root: &Path, session: &str) -> Result<(File, PathBuf), ()> {
    let directory = root.join("diagnostics");
    std::fs::create_dir_all(&directory).map_err(|_| ())?;
    // A process-local logging failure never changes routing. Windows sharing
    // mode serializes retention/creation without a blocking interprocess wait.
    let _lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(directory.join("proxy-trace-retention.lock"))
        .map_err(|_| ())?;
    let entries = std::fs::read_dir(&directory)
        .map_err(|_| ())?
        .take(129)
        .collect::<Vec<_>>();
    if entries.len() > 128 {
        return Err(());
    }
    let mut ours = entries
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(own_filename)
                && entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    ours.sort();
    while ours.len() >= MAX_FILES {
        let Some(index) = ours
            .iter()
            .position(|path| std::fs::remove_file(path).is_ok())
        else {
            return Err(());
        };
        ours.remove(index);
    }
    let path = directory.join(format!("proxy-trace-{}-{session}.ndjson", now_ms()));
    // Readers may inspect the active log; no writer/delete sharing prevents
    // retention from removing another enabled process's active file.
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .share_mode(1)
        .open(&path)
        .map_err(|_| ())?;
    Ok((file, path))
}
fn write_line(file: &mut File, value: &serde_json::Value) -> std::io::Result<usize> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    file.write_all(&line)?;
    Ok(line.len())
}
fn write_trace(
    mut file: File,
    receiver: mpsc::Receiver<Envelope>,
    session: &str,
    control: &Control,
) {
    let start = serde_json::json!({"schema":1,"event":"trace_start","session":session,"ts_ms":now_ms(),
        "cap_bytes":MAX_BYTES,"cap_events":MAX_EVENTS,"queue_capacity":QUEUE,"retained_files":MAX_FILES});
    let Ok(mut bytes) = write_line(&mut file, &start).map(|n| n as u64) else {
        trace::stop();
        return;
    };
    let mut count = 0;
    let mut reason = "normal_exit";
    loop {
        let envelope = match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(envelope) => envelope,
            Err(mpsc::RecvTimeoutError::Timeout) if !control.stop.load(Ordering::Acquire) => {
                continue;
            }
            Err(_) => break,
        };
        let value = event_json(envelope, session);
        let needed = value.to_string().len() as u64 + 1;
        // Reserve room for the explicit stop/limit summary.
        if count >= MAX_EVENTS || bytes + needed + 1024 > MAX_BYTES {
            reason = if count >= MAX_EVENTS {
                "event_cap"
            } else {
                "byte_cap"
            };
            break;
        }
        match write_line(&mut file, &value) {
            Ok(written) => {
                bytes += written as u64;
                count += 1;
            }
            Err(_) => {
                trace::stop();
                return;
            }
        }
    }
    trace::stop();
    let summary = serde_json::json!({"schema":1,"event":if reason=="normal_exit"{"trace_stop"}else{"trace_limit"},
        "session":session,"ts_ms":now_ms(),"reason":reason,"events_written":count,
        "dropped_queue_events":control.dropped.load(Ordering::Relaxed)});
    let _ = write_line(&mut file, &summary);
    let _ = file.flush();
}
fn event_json(envelope: Envelope, session: &str) -> serde_json::Value {
    let event = envelope.event;
    let c = event.context;
    let mut value = serde_json::json!({"schema":1,"session":session,"ts_ms":envelope.ts_ms,"elapsed_ms":envelope.elapsed_ms,
        "event":event.stage.label(),"result":event.outcome.label()});
    let map = value.as_object_mut().unwrap();
    for (name, id) in [
        ("account", c.account),
        ("route", c.route),
        ("bridge", c.bridge),
        ("inbound", c.inbound),
        ("tab", c.tab),
        ("navigation", c.navigation),
        ("action", c.action),
        ("generation", c.generation),
        ("target", c.target),
    ] {
        if id != 0 {
            map.insert(name.into(), id.into());
        }
    }
    if let Some(v) = c.source {
        map.insert("source".into(), v.label().into());
    }
    if let Some(v) = c.binding {
        map.insert("binding".into(), v.label().into());
    }
    if let Some(v) = c.protocol {
        map.insert("protocol".into(), v.scheme().into());
    }
    if let Some(v) = c.route_path {
        map.insert("route_path".into(), v.label().into());
    }
    if let Some(v) = c.destination {
        map.insert("destination".into(), v.label().into());
    }
    if let Some(v) = c.proxy_required {
        map.insert("proxy_required".into(), v.into());
    }
    if let Some(v) = c.force_proxy {
        map.insert("force_proxy".into(), v.into());
    }
    if let Some(v) = event.duration_ms {
        map.insert("duration_ms".into(), v.into());
    }
    if let Some(v) = event.code {
        map.insert("os_code".into(), v.into());
    }
    if let Some(v) = event.http_status {
        map.insert("http_status".into(), v.into());
    }
    if let Some(v) = event.web_error {
        map.insert("web_error".into(), v.into());
    }
    if let Some(v) = event.redirects {
        map.insert("redirects".into(), v.into());
    }
    if let Some(v) = event.accepted {
        map.insert("completion_current".into(), v.into());
    }
    if let Some(v) = event.native_success {
        map.insert("native_success".into(), v.into());
    }
    if let Some(v) = event.pending_stage {
        map.insert("pending_stage".into(), v.label().into());
    }
    if let Some(v) = event.parse_failure {
        map.insert("parse_failure".into(), v.label().into());
    }
    if event.stage == Stage::RelayEnd {
        map.insert(
            "io_direction".into(),
            "not_identified_by_copy_bidirectional".into(),
        );
    }
    value
}

#[derive(Default)]
struct Ids {
    accounts: HashMap<uuid::Uuid, u64>,
    tabs: HashMap<uuid::Uuid, u64>,
    actions: HashMap<uuid::Uuid, (u64, Instant)>,
}
static IDS: OnceLock<Mutex<Ids>> = OnceLock::new();
fn anon(map: &mut HashMap<uuid::Uuid, u64>, key: uuid::Uuid) -> u64 {
    if let Some(id) = map.get(&key) {
        return *id;
    }
    if map.len() >= 512 {
        return 0;
    }
    let id = trace::new_id();
    map.insert(key, id);
    id
}
pub fn identity(identity: BrowserIdentity) -> Context {
    if !trace::enabled() {
        return Context::default();
    }
    let Ok(mut ids) = IDS.get_or_init(|| Mutex::new(Ids::default())).try_lock() else {
        return Context::default();
    };
    Context {
        account: anon(&mut ids.accounts, identity.account_id),
        tab: anon(&mut ids.tabs, identity.tab_id),
        generation: identity.generation,
        action: ids
            .actions
            .get(&identity.tab_id)
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(2))
            .map(|(id, _)| *id)
            .unwrap_or(0),
        source: Some(Source::WebView),
        ..Context::default()
    }
}
pub fn account(id: uuid::Uuid) -> u64 {
    if !trace::enabled() {
        return 0;
    }
    let Ok(mut ids) = IDS.get_or_init(|| Mutex::new(Ids::default())).try_lock() else {
        return 0;
    };
    anon(&mut ids.accounts, id)
}
pub fn binding(network: AccountNetwork) -> Binding {
    match network {
        AccountNetwork::Direct => Binding::Direct,
        AccountNetwork::HttpProxy { .. } => Binding::Legacy,
        AccountNetwork::PoolProxy { .. } => Binding::Pool,
    }
}
pub fn action(identity: BrowserIdentity, mut context: Context, outcome: Outcome) {
    if !trace::enabled() {
        return;
    }
    let id = trace::new_id();
    if let Ok(mut ids) = IDS.get_or_init(|| Mutex::new(Ids::default())).try_lock()
        && (ids.actions.contains_key(&identity.tab_id) || ids.actions.len() < 512)
    {
        // A selection is observable but may not dispatch a navigation. Do not
        // attach it (or an indefinitely old command) to later page activity.
        if outcome != Outcome::Select {
            ids.actions.insert(identity.tab_id, (id, Instant::now()));
        }
    }
    context.action = id;
    context.navigation = 0;
    Event::new(context, Stage::ManualAction, outcome).emit();
}
fn take_action(identity: BrowserIdentity) -> u64 {
    let Ok(mut ids) = IDS.get_or_init(|| Mutex::new(Ids::default())).try_lock() else {
        return 0;
    };
    ids.actions
        .remove(&identity.tab_id)
        .filter(|(_, at)| at.elapsed() < Duration::from_secs(2))
        .map(|(id, _)| id)
        .unwrap_or(0)
}
pub struct Navigation {
    pub context: Context,
    native: u64,
    redirects: u32,
    started: Instant,
}
pub fn target(url: &str) -> (u64, Option<trace::Destination>) {
    if !trace::enabled() || url.len() > 16 * 1024 {
        return (0, None);
    }
    let Some(url) = url::Url::parse(url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
    else {
        return (0, None);
    };
    match (url.host_str(), url.port_or_known_default()) {
        (Some(host), Some(port)) => (
            trace::target_id(host, port),
            Some(trace::Destination::from_host(host)),
        ),
        _ => (0, None),
    }
}
impl Navigation {
    pub fn new(context: Context) -> Self {
        Self {
            context,
            native: 0,
            redirects: 0,
            started: Instant::now(),
        }
    }
    pub fn starting(
        &mut self,
        identity: BrowserIdentity,
        native: u64,
        redirected: bool,
        url: Option<&str>,
    ) {
        if !trace::enabled() {
            return;
        }
        (self.context.target, self.context.destination) = url.map(target).unwrap_or_default();
        if self.native != native {
            self.native = native;
            self.context.navigation = trace::new_id();
            self.redirects = 0;
            self.started = Instant::now();
            self.context.action = take_action(identity);
            Event::new(self.context, Stage::NativeStart, Outcome::Start).emit();
        }
        if redirected {
            self.redirects = self.redirects.saturating_add(1);
            let mut event = Event::new(self.context, Stage::NativeRedirect, Outcome::Start);
            event.redirects = Some(self.redirects);
            event.emit();
        }
    }
    pub fn completed(
        &self,
        native: u64,
        success: bool,
        status: i32,
        http: Option<i32>,
        accepted: bool,
    ) {
        if !trace::enabled() {
            return;
        }
        let mut context = self.context;
        if native != self.native {
            context.navigation = 0;
            context.action = 0;
            context.target = 0;
            context.destination = None;
        }
        let mut event = Event::new(
            context,
            Stage::NativeComplete,
            if !accepted {
                Outcome::Stale
            } else if native != self.native {
                Outcome::Uncorrelated
            } else if success {
                Outcome::Ok
            } else if status == 14 {
                Outcome::Cancelled
            } else {
                Outcome::Failed
            },
        );
        if native == self.native {
            event.duration_ms =
                Some(self.started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        }
        event.http_status = http;
        event.web_error = Some(status);
        if native == self.native {
            event.redirects = Some(self.redirects);
        }
        event.accepted = Some(accepted);
        event.native_success = Some(success);
        event.emit();
    }
    pub fn readiness(&self, stage: Stage, outcome: Outcome) {
        let mut event = Event::new(self.context, stage, outcome);
        event.duration_ms = Some(self.started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        event.emit();
    }
}
