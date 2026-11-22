//! Native-only test fixture: simulated upstream authentication, no real secrets.

use base64::{Engine, engine::general_purpose::STANDARD};
use muse_core::{AccountNetwork, Workspace};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

static REQUESTS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
static REJECTED: AtomicUsize = AtomicUsize::new(0);
static INVALID: AtomicUsize = AtomicUsize::new(0);
struct ProxyControl {
    address: std::net::SocketAddr,
    user: String,
    expected: Arc<RwLock<String>>,
}
static CONTROL: OnceLock<ProxyControl> = OnceLock::new();

pub fn rotate_credentials() -> Result<String, String> {
    let control = CONTROL.get().ok_or("proxy fixture is not running")?;
    let secret = uuid::Uuid::new_v4().simple().to_string();
    *control.expected.write().map_err(|_| "fixture auth lock")? = format!(
        "Basic {}",
        STANDARD.encode(format!("{}:{secret}", control.user))
    );
    Ok(format!(
        "http://{}:{secret}@{}",
        control.user, control.address
    ))
}

pub fn authenticated_requests() -> Option<usize> {
    REQUESTS.get().map(|count| count.load(Ordering::Relaxed))
}
pub fn diagnostics() -> serde_json::Value {
    serde_json::json!({"authenticated":authenticated_requests(),"auth_rejected":REJECTED.load(Ordering::Relaxed),"invalid_target":INVALID.load(Ordering::Relaxed)})
}

pub fn prepare(root: &Path, fixture_url: &str, pooled: bool) -> Result<(), String> {
    let target = url::Url::parse(fixture_url).map_err(|_| "invalid smoke fixture")?;
    let target_port = target.port().ok_or("missing fixture port")?;
    let target_host = target.host_str().ok_or("missing fixture host")?.to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "cannot bind fake upstream")?;
    let address = listener.local_addr().map_err(|_| "fake upstream address")?;
    let user = uuid::Uuid::new_v4().simple().to_string();
    let secret = uuid::Uuid::new_v4().simple().to_string();
    let expected = Arc::new(RwLock::new(format!(
        "Basic {}",
        STANDARD.encode(format!("{user}:{secret}"))
    )));
    let url = format!("http://{user}:{secret}@{address}");
    let requests = Arc::new(AtomicUsize::new(0));
    let _ = REQUESTS.set(requests.clone());
    let _ = CONTROL.set(ProxyControl {
        address,
        user,
        expected: expected.clone(),
    });
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let expected = expected.clone();
            let requests = requests.clone();
            let target_host = target_host.clone();
            std::thread::spawn(move || {
                let _ = forward(stream, &expected, &target_host, target_port, &requests);
            });
        }
    });
    let mut workspace = Workspace::new();
    let account = workspace.add_account("认证代理验证");
    workspace.accounts[0].network = AccountNetwork::HttpProxy { force_proxy: true };
    crate::secrets::ProtectedStore::new(root).save_proxy(account, &url)?;
    if pooled {
        crate::proxy_pool::migrate(root, &mut workspace)?;
        if workspace
            .account(account)
            .and_then(|entry| entry.network.proxy_id())
            .is_none()
        {
            return Err("proxy fixture was not migrated into the pool".into());
        }
        Ok(())
    } else {
        workspace.save(&root.join("workspace.json"))
    }
}

fn forward(
    mut browser: TcpStream,
    expected: &RwLock<String>,
    target_host: &str,
    target_port: u16,
    requests: &AtomicUsize,
) -> std::io::Result<()> {
    browser.set_read_timeout(Some(Duration::from_secs(8)))?;
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() > 32 * 1024 {
            return Ok(());
        }
        let mut byte = [0];
        browser.read_exact(&mut byte)?;
        header.push(byte[0]);
    }
    let header = String::from_utf8_lossy(&header);
    let auth = header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("proxy-authorization"))
        .map(|(_, v)| v.trim());
    let expected = expected
        .read()
        .map_err(|_| std::io::Error::other("fixture auth lock"))?;
    if auth != Some(expected.as_str()) {
        REJECTED.fetch_add(1, Ordering::Relaxed);
        browser.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    let mut lines = header.lines();
    let mut request = lines.next().unwrap_or_default().split_whitespace();
    let method = request.next().unwrap_or_default();
    let uri = request.next().unwrap_or_default();
    if method == "CONNECT" && uri == "muse.ai:443" {
        requests.fetch_add(1, Ordering::Relaxed);
        browser.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        return Ok(());
    }
    // Network.framework can tunnel even a plain HTTP origin through CONNECT.
    // The fixture permits only its exact loopback target, never arbitrary hosts.
    if method == "CONNECT" && uri == format!("{target_host}:{target_port}") {
        let mut target = TcpStream::connect(("127.0.0.1", target_port))?;
        requests.fetch_add(1, Ordering::Relaxed);
        browser.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        let mut browser_read = browser.try_clone()?;
        let mut target_write = target.try_clone()?;
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut browser_read, &mut target_write);
            let _ = target_write.shutdown(std::net::Shutdown::Write);
        });
        let _ = std::io::copy(&mut target, &mut browser);
        return Ok(());
    }
    let url = match url::Url::parse(uri) {
        Ok(url) => url,
        Err(_) => {
            INVALID.fetch_add(1, Ordering::Relaxed);
            browser.write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            return Ok(());
        }
    };
    if method != "GET" || url.host_str() != Some(target_host) || url.port() != Some(target_port) {
        INVALID.fetch_add(1, Ordering::Relaxed);
        browser.write_all(
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        return Ok(());
    }
    let mut target = TcpStream::connect(("127.0.0.1", target_port))?;
    requests.fetch_add(1, Ordering::Relaxed);
    // Preserve the synthetic account Cookie for the local navigation probe.
    // Proxy-Authorization remains private to this fake upstream hop.
    let cookie = header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
        .map(|(_, value)| format!("Cookie: {}\r\n", value.trim()))
        .collect::<String>();
    write!(
        target,
        "GET {} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\n{cookie}Connection: close\r\n\r\n",
        url.path()
    )?;
    std::io::copy(&mut target, &mut browser)?;
    Ok(())
}
