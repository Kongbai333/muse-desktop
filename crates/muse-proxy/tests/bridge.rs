use muse_proxy::{ProxyBridge, Upstream};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::Duration,
};

fn provider_url(address: impl std::fmt::Display) -> String {
    let secret = String::from_utf8(vec![112, 97, 115, 115]).unwrap();
    format!("http://user:{secret}@{address}")
}

fn read_head(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut result = Vec::new();
    while !result.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        result.push(byte[0]);
        assert!(result.len() < 65536);
    }
    String::from_utf8(result).unwrap()
}

#[test]
fn rejects_unsupported_protocols_and_invalid_credential_encodings() {
    for input in [
        "socks4://localhost:1080",
        "ftp://localhost:21",
        "http://user:%FF@localhost:8080",
        "http://localhost:0",
        "socks5://:secret@localhost",
        "socks5://localhost/path",
    ] {
        assert!(Upstream::parse(input).is_err());
    }
    assert!(Upstream::parse("http://user:p%40ss@127.0.0.1:8080").is_ok());
}

#[test]
fn socks_aliases_accept_a_token_without_a_password() {
    for scheme in ["socks", "socks5", "socks5h"] {
        for auth in ["fixture-token", "fixture-token:"] {
            let config = Upstream::parse(&format!("{scheme}://{auth}@127.0.0.1:7153"))
                .expect("token-only SOCKS5 address should parse");
            assert_eq!(config.protocol(), muse_proxy::ProxyProtocol::Socks5);
            assert_eq!(config.port(), 7153);
        }
    }
}

#[test]
fn token_only_socks5_preserves_the_username_and_supplies_a_placeholder_password() {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let config = Upstream::parse(&format!("socks5://fixture-token:@{address}")).unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut greeting = [0; 4];
        socket.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 2, 0, 2]);
        socket.write_all(&[5, 2]).unwrap();
        let expected = b"\x01\x0dfixture-token\x01x";
        let mut credentials = [0; 17];
        socket.read_exact(&mut credentials).unwrap();
        assert_eq!(&credentials, expected);
        socket.write_all(&[1, 0]).unwrap();
        let expected = b"\x05\x01\x00\x03\x07muse.ai\x01\xbb";
        let mut request = [0; 14];
        socket.read_exact(&mut request).unwrap();
        assert_eq!(&request, expected);
        socket.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert_eq!(runtime.block_on(config.probe()), Ok(()));
    provider.join().unwrap();
}

#[test]
fn supports_the_default_ports_and_ipv6_upstream_addresses() {
    for input in ["127.0.0.1:8080", "https://localhost", "socks5://[::1]"] {
        assert!(Upstream::parse(input).is_ok(), "rejected {input}");
    }
}

fn accept_socks5(socket: &mut TcpStream, authenticate: bool, target: &[u8]) {
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut greeting = [0; 3];
    socket.read_exact(&mut greeting).unwrap();
    let method = if authenticate { 2 } else { 0 };
    if authenticate {
        assert_eq!(greeting, [5, 2, 0]);
        let mut extra = [0];
        socket.read_exact(&mut extra).unwrap();
        assert_eq!(extra, [2]);
    } else {
        assert_eq!(greeting, [5, 1, 0]);
    }
    socket.write_all(&[5, method]).unwrap();
    if authenticate {
        let mut credentials = [0; 11];
        socket.read_exact(&mut credentials).unwrap();
        assert_eq!(&credentials, b"\x01\x04user\x04p@ss");
        socket.write_all(&[1, 0]).unwrap();
    }
    let mut request = vec![0; target.len()];
    socket.read_exact(&mut request).unwrap();
    assert_eq!(request, target);
    socket.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
}

#[test]
fn socks5_connect_authenticates_and_passes_the_target_domain_without_local_dns() {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        accept_socks5(
            &mut socket,
            true,
            b"\x05\x01\x00\x03\x0cexample.test\x01\xbb",
        );
        socket.write_all(b"ready").unwrap();
        let mut payload = [0; 4];
        socket.read_exact(&mut payload).unwrap();
        assert_eq!(&payload, b"ping");
    });
    let config = Upstream::parse(&format!("socks5://user:p%40ss@{address}")).unwrap();
    let bridge = ProxyBridge::start(config).unwrap();
    let mut client = TcpStream::connect(bridge.address()).unwrap();
    write!(client, "CONNECT example.test:443 HTTP/1.1\r\nHost: example.test:443\r\nProxy-Authorization: {}\r\n\r\n", bridge.auth().authorization()).unwrap();
    assert!(read_head(&mut client).starts_with("HTTP/1.1 200"));
    let mut greeting = [0; 5];
    client.read_exact(&mut greeting).unwrap();
    assert_eq!(&greeting, b"ready");
    client.write_all(b"ping").unwrap();
    provider.join().unwrap();
}

#[test]
fn socks5_http_sends_origin_form_without_proxy_credentials() {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        accept_socks5(
            &mut socket,
            false,
            b"\x05\x01\x00\x01\x7f\x00\x00\x01\x1f\x91",
        );
        let head = read_head(&mut socket).to_ascii_lowercase();
        assert!(head.starts_with("post /upload?item=1 http/1.1\r\n"));
        assert!(head.contains("\r\nhost: 127.0.0.1:8081\r\n"));
        assert!(!head.contains("proxy-authorization"));
        let mut body = [0; 4];
        socket.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"data");
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc")
            .unwrap();
    });
    let bridge =
        ProxyBridge::start(Upstream::parse(&format!("socks5://{address}")).unwrap()).unwrap();
    let mut client = TcpStream::connect(bridge.address()).unwrap();
    write!(client, "POST http://127.0.0.1:8081/upload?item=1 HTTP/1.1\r\nHost: 127.0.0.1:8081\r\nContent-Length: 4\r\nProxy-Authorization: {}\r\n\r\ndata", bridge.auth().authorization()).unwrap();
    assert!(read_head(&mut client).starts_with("HTTP/1.1 200"));
    let mut body = [0; 3];
    client.read_exact(&mut body).unwrap();
    assert_eq!(&body, b"abc");
    provider.join().unwrap();
}

#[test]
fn socks5_authentication_failure_never_falls_back_to_the_target() {
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut greeting = [0; 4];
        socket.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 2, 0, 2]);
        socket.write_all(&[5, 0xff]).unwrap();
    });
    let bridge =
        ProxyBridge::start(Upstream::parse(&format!("socks5://user:p%40ss@{address}")).unwrap())
            .unwrap();
    let mut client = TcpStream::connect(bridge.address()).unwrap();
    let target_address = target.local_addr().unwrap();
    write!(client, "CONNECT {target_address} HTTP/1.1\r\nHost: {target_address}\r\nProxy-Authorization: {}\r\n\r\n", bridge.auth().authorization()).unwrap();
    assert!(read_head(&mut client).starts_with("HTTP/1.1 502"));
    assert!(target.accept().is_err());
    provider.join().unwrap();
}

#[test]
fn untrusted_local_requests_get_a_challenge_without_contacting_upstream() {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    upstream.set_nonblocking(true).unwrap();
    let bridge =
        ProxyBridge::start(Upstream::parse(&provider_url(upstream.local_addr().unwrap())).unwrap())
            .unwrap();
    let mut client = TcpStream::connect(bridge.address()).unwrap();
    client
        .write_all(b"CONNECT example.test:443 HTTP/1.1\r\nHost: example.test:443\r\n\r\n")
        .unwrap();
    let response = read_head(&mut client);
    assert!(response.starts_with("HTTP/1.1 407"));
    assert!(response.contains(bridge.auth().challenge()));
    assert!(upstream.accept().is_err());
}

#[test]
fn authenticated_connect_preserves_prefetched_bytes_and_bidirectional_data() {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        let request = read_head(&mut socket);
        assert!(request.starts_with("CONNECT example.test:443 HTTP/1.1"));
        assert!(request.contains("Proxy-Authorization: Basic dXNlcjpwYXNz\r\n"));
        socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\nready")
            .unwrap();
        let mut data = [0; 4];
        socket.read_exact(&mut data).unwrap();
        assert_eq!(&data, b"ping");
        socket.write_all(b"pong").unwrap();
    });
    let bridge = ProxyBridge::start(Upstream::parse(&provider_url(address)).unwrap()).unwrap();
    let mut client = TcpStream::connect(bridge.address()).unwrap();
    write!(client,"CONNECT example.test:443 HTTP/1.1\r\nHost: example.test:443\r\nProxy-Authorization: {}\r\n\r\n", bridge.auth().authorization()).unwrap();
    assert!(read_head(&mut client).starts_with("HTTP/1.1 200"));
    let mut greeting = [0; 5];
    client.read_exact(&mut greeting).unwrap();
    assert_eq!(&greeting, b"ready");
    client.write_all(b"ping").unwrap();
    let mut result = [0; 4];
    client.read_exact(&mut result).unwrap();
    assert_eq!(&result, b"pong");
    provider.join().unwrap();
}

#[test]
fn ordinary_http_replaces_local_auth_and_preserves_request_and_response_bodies() {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        let request = read_head(&mut socket);
        let lower = request.to_ascii_lowercase();
        assert!(lower.starts_with("post http://example.test/upload http/1.1"));
        assert!(lower.contains("proxy-authorization: basic dxnlcjpwyxnz\r\n"));
        let mut body = [0; 4];
        socket.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"data");
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc")
            .unwrap();
    });
    let bridge = ProxyBridge::start(Upstream::parse(&provider_url(address)).unwrap()).unwrap();
    let mut client = TcpStream::connect(bridge.address()).unwrap();
    write!(client,"POST http://example.test/upload HTTP/1.1\r\nHost: example.test\r\nContent-Length: 4\r\nConnection: close\r\nProxy-Authorization: {}\r\n\r\ndata",bridge.auth().authorization()).unwrap();
    assert!(read_head(&mut client).starts_with("HTTP/1.1 200"));
    let mut body = [0; 3];
    client.read_exact(&mut body).unwrap();
    assert_eq!(&body, b"abc");
    provider.join().unwrap();
}

#[test]
fn rejected_upstream_authentication_never_falls_back_to_the_target() {
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        read_head(&mut socket);
        socket
            .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });
    let bridge = ProxyBridge::start(Upstream::parse(&provider_url(address)).unwrap()).unwrap();
    let mut client = TcpStream::connect(bridge.address()).unwrap();
    write!(
        client,
        "CONNECT {} HTTP/1.1\r\nHost: test\r\nProxy-Authorization: {}\r\n\r\n",
        target.local_addr().unwrap(),
        bridge.auth().authorization()
    )
    .unwrap();
    assert!(read_head(&mut client).starts_with("HTTP/1.1 502"));
    assert!(target.accept().is_err());
    provider.join().unwrap();
}

#[test]
fn dropping_a_bridge_closes_existing_tunnels_and_stops_accepting_connections() {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        read_head(&mut socket);
        socket.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
        let mut byte = [0];
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        assert_eq!(socket.read(&mut byte).unwrap(), 0);
    });
    let bridge = ProxyBridge::start(Upstream::parse(&provider_url(address)).unwrap()).unwrap();
    let local = bridge.address();
    let mut client = TcpStream::connect(local).unwrap();
    write!(client,"CONNECT example.test:443 HTTP/1.1\r\nHost: example.test\r\nProxy-Authorization: {}\r\n\r\n",bridge.auth().authorization()).unwrap();
    assert!(read_head(&mut client).starts_with("HTTP/1.1 200"));
    drop(bridge);
    let mut byte = [0];
    assert_eq!(client.read(&mut byte).unwrap(), 0);
    provider.join().unwrap();
    assert!(TcpStream::connect_timeout(&local, Duration::from_millis(200)).is_err());
}

#[test]
fn informational_connect_responses_do_not_drop_the_following_tunnel_response() {
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = upstream.local_addr().unwrap();
    let provider = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        read_head(&mut socket);
        socket
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\n\r\nhello")
            .unwrap();
    });
    let bridge = ProxyBridge::start(Upstream::parse(&provider_url(address)).unwrap()).unwrap();
    let mut client = TcpStream::connect(bridge.address()).unwrap();
    write!(client,"CONNECT example.test:443 HTTP/1.1\r\nHost: example.test\r\nProxy-Authorization: {}\r\n\r\n",bridge.auth().authorization()).unwrap();
    assert!(read_head(&mut client).starts_with("HTTP/1.1 200"));
    let mut bytes = [0; 5];
    client.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"hello");
    provider.join().unwrap();
}
