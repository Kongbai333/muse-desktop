//! A local, read-only fixture server; no real website accounts are needed.

use gpui::AssetSource;
use std::{
    io::{Read, Write},
    net::TcpListener,
};

pub fn start() -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut header = Vec::new();
            let complete = loop {
                let mut buffer = [0u8; 4096];
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break false,
                    Ok(length) => header.extend_from_slice(&buffer[..length]),
                }
                if header.len() > 32 * 1024 {
                    break false;
                }
                if header.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    break true;
                }
            };
            if !complete {
                continue;
            }
            let request = String::from_utf8_lossy(&header);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            if path.starts_with("/icons/") {
                if let Ok(Some(bytes)) = crate::assets::Assets.load(path.trim_start_matches('/')) {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/svg+xml\r\nContent-Length: {}\r\nCache-Control: public, max-age=3600\r\nConnection: close\r\n\r\n",
                        bytes.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&bytes);
                } else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
                continue;
            }
            let title = if path.starts_with("/next") {
                "第二个页面"
            } else if path.starts_with("/popup") {
                "新标签页"
            } else {
                "本地诊断"
            };
            let body =
                include_str!("../../../assets/diagnostics.html").replace("{{PAGE_TITLE}}", title);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Ok(format!("http://{address}/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_http_headers_still_return_the_requested_page_without_resetting() {
        let home = start().unwrap();
        let url = url::Url::parse(&home).unwrap();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", url.port().unwrap())).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        stream.write_all(b"GET ").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(25));
        let second =
            stream.write_all(b"/next HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        assert!(
            second.is_ok(),
            "fixture closed before receiving the complete request"
        );
        let mut response = String::new();
        let result = stream.read_to_string(&mut response);
        assert!(
            result.is_ok(),
            "fixture reset the connection with unread headers"
        );
        assert!(response.contains("<title>第二个页面 · Muse</title>"));
    }
}
