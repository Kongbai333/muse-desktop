use super::*;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
};

fn fixture() -> (TlsAcceptor, RootCertStore) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
    (TlsAcceptor::from(Arc::new(config)), roots)
}

async fn read_head(stream: &mut (impl AsyncRead + Unpin)) -> String {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        head.push(stream.read_u8().await.unwrap());
        assert!(head.len() < MAX_HEAD);
    }
    String::from_utf8(head).unwrap()
}

async fn send_local(bridge: &ProxyBridge, head: &str) -> TcpStream {
    let mut client = TcpStream::connect(bridge.address()).await.unwrap();
    let request = format!(
        "{head}\r\nProxy-Authorization: {}\r\n\r\n",
        bridge.auth().authorization()
    );
    client.write_all(request.as_bytes()).await.unwrap();
    client
}

#[tokio::test]
async fn https_proxy_uses_verified_tls_for_http_forwarding_and_connect_tunnels() {
    timeout(Duration::from_secs(10), async {
        let (acceptor, roots) = fixture();
        assert!(HTTPS_CONNECTOR.set(Some(tls_connector(roots))).is_ok());
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = upstream.local_addr().unwrap();
        let config = Upstream::parse(&format!("https://fixture:user@{address}")).unwrap();
        let bridge = ProxyBridge::start(config.clone()).unwrap();
        let provider = tokio::spawn(async move {
            let (stream, _) = upstream.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            let head = read_head(&mut stream).await.to_ascii_lowercase();
            assert!(head.starts_with("get http://example.test/path http/1.1\r\n"));
            assert!(head.contains("\r\nproxy-authorization: basic zml4dhvyztp1c2vy\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc")
                .await
                .unwrap();
            stream.shutdown().await.unwrap();
            let (stream, _) = upstream.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            let head = read_head(&mut stream).await;
            assert!(head.starts_with("CONNECT example.test:443 HTTP/1.1\r\n"));
            assert!(head.contains("\r\nProxy-Authorization: Basic Zml4dHVyZTp1c2Vy\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\nready")
                .await
                .unwrap();
            stream.flush().await.unwrap();
            let mut ping = [0; 4];
            stream.read_exact(&mut ping).await.unwrap();
            assert_eq!(&ping, b"ping");
            stream.write_all(b"pong").await.unwrap();
            stream.shutdown().await.unwrap();
            let (stream, _) = upstream.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            let head = read_head(&mut stream).await;
            assert!(head.starts_with("CONNECT muse.ai:443 HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();
        });
        let mut client = send_local(
            &bridge,
            "GET http://example.test/path HTTP/1.1\r\nHost: example.test",
        )
        .await;
        assert!(read_head(&mut client).await.starts_with("HTTP/1.1 200"));
        let mut body = [0; 3];
        client.read_exact(&mut body).await.unwrap();
        assert_eq!(&body, b"abc");
        let mut client = send_local(
            &bridge,
            "CONNECT example.test:443 HTTP/1.1\r\nHost: example.test:443",
        )
        .await;
        assert!(read_head(&mut client).await.starts_with("HTTP/1.1 200"));
        let mut greeting = [0; 5];
        client.read_exact(&mut greeting).await.unwrap();
        assert_eq!(&greeting, b"ready");
        client.write_all(b"ping").await.unwrap();
        let mut pong = [0; 4];
        client.read_exact(&mut pong).await.unwrap();
        assert_eq!(&pong, b"pong");
        assert_eq!(config.probe().await, Ok(()));
        provider.await.unwrap();

        let (untrusted, _) = fixture();
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = upstream.local_addr().unwrap();
        let provider = tokio::spawn(async move {
            let (stream, _) = upstream.accept().await.unwrap();
            assert!(untrusted.accept(stream).await.is_err());
        });
        let config = Upstream::parse(&format!("https://{address}")).unwrap();
        assert_eq!(config.probe().await, Err(ProxyError::Tls));
        provider.await.unwrap();
    })
    .await
    .unwrap();
}
