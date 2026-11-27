mod process;
pub mod schema;

use ring::digest::{Context, SHA256};
use serde_json::json;
use std::{
    fs::OpenOptions,
    io::Read,
    os::windows::fs::OpenOptionsExt,
    path::Path,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroizing;

const HASH: &str = "b3db115379b866eeb1db722be0f31bf6bfa1610dd54a019fc748ecf9a733ab9d";
const BLOCKED: &str = "无法启动此账号的固定 Mihomo 内核；未使用其他出口";
pub use process::Core;

pub const NATIVE: &str = "muse-provider-v1:";

pub fn snapshot(stored: &str) -> Result<serde_json::Value, String> {
    if let Some(body) = stored.strip_prefix(NATIVE) {
        let node: serde_json::Value = serde_json::from_str(body).map_err(|_| BLOCKED)?;
        if node["name"] != "account" || node["skip-cert-verify"] != false {
            return Err(BLOCKED.into());
        }
        return Ok(node);
    }
    schema::for_route(stored)
}

fn image() -> Result<(std::fs::File, std::path::PathBuf), String> {
    let path = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("mihomo.exe")))
        .ok_or(BLOCKED)?;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .map_err(|_| "缺少随包提供的 mihomo.exe；此账号未连接")?;
    let mut hash = Context::new(&SHA256);
    let mut buffer = [0u8; 65536];
    loop {
        let size = file.read(&mut buffer).map_err(|_| BLOCKED)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    let digest = hash.finish();
    if digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        != HASH
    {
        return Err("Mihomo 版本或 SHA-256 不符；此账号未连接".into());
    }
    Ok((file, path))
}

pub fn provider(source: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let (_lock, path) = image()?;
    let input = Zeroizing::new(serde_json::to_vec(source).map_err(|_| BLOCKED)?);
    Core::provider(&path, &input)
        .map_err(|_| "Mihomo 读取订阅失败：下载未完成、格式无效或没有节点".into())
}

pub fn start(root: &Path, stored: &str) -> Result<(Core, muse_proxy::Upstream), String> {
    let node = snapshot(stored)?;
    let (_lock, path) = image()?;
    let mut trust = Vec::new();
    if node["server"]
        .as_str()
        .and_then(|host| host.parse::<std::net::IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback())
        && node["sni"]
            .as_str()
            .is_some_and(|name| name.ends_with(".test"))
    {
        let args = std::env::args_os().collect::<Vec<_>>();
        if let Some(pair) = args.windows(2).find(|args| args[0] == "--loopback-test-ca") {
            let certificate =
                std::fs::read_to_string(&pair[1]).map_err(|_| "无法读取指定的回环测试 CA")?;
            if certificate.len() > 16384
                || !certificate
                    .trim()
                    .starts_with("-----BEGIN CERTIFICATE-----")
            {
                return Err("回环测试 CA 无效".into());
            }
            trust.push(certificate);
        }
    }
    let key = Zeroizing::new(format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ));
    let config = Zeroizing::new(
        json!({"mode":"rule", "log-level":"silent", "allow-lan":false,
        "geox-url":{"mmdb":"", "geoip":"", "geosite":"", "asn":""}, "geo-auto-update":false,
        "find-process-mode":"off", "profile":{"store-selected":false,"store-fake-ip":false},
        "dns":{"enable":false,"fallback-filter":{"geoip":false}}, "tun":{"enable":false},
        "tls":{"custom-certifactes":trust},
        "listeners":[{"name":"local", "type":"shadowsocks", "listen":"127.0.0.1", "port":0,
            "cipher":"aes-256-gcm", "password":key.as_str(), "udp":false, "proxy":"account"}],
        "proxies":[node], "rules":["MATCH,REJECT"]})
        .to_string(),
    );
    let home = root.join(format!("core-{}", Uuid::new_v4()));
    let core = Core::spawn(&path, &home, config.as_bytes()).map_err(|_| BLOCKED)?;
    let until = Instant::now() + Duration::from_secs(3);
    while core.running() && Instant::now() < until {
        if let Some(port) = core.loopback_port() {
            let upstream = muse_proxy::Upstream::parse(&format!(
                "ss://aes-256-gcm:{}@127.0.0.1:{port}",
                key.as_str()
            ))
            .map_err(|_| BLOCKED)?;
            return Ok((core, upstream));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("内核拒绝了节点配置，或无法建立此账号的回环入口；未使用直连".into())
}
