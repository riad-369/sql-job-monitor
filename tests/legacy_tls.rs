//! Exercises the `legacy-tls` connector against a local OpenSSL server that,
//! like an unpatched SQL Server 2012, offers only TLS 1.0 with a 1024-bit
//! SHA-1 self-signed certificate. Needs the `openssl` command-line tool
//! (OpenSSL 1.1 or 3; macOS's LibreSSL is skipped).
#![cfg(feature = "legacy-tls")]

use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Server {
    child: Child,
    port: u16,
    dir: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn openssl_cli() -> Option<String> {
    let output = Command::new("openssl").arg("version").output().ok()?;
    let version = String::from_utf8_lossy(&output.stdout).into_owned();
    version.starts_with("OpenSSL").then_some(version)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn tls10_server() -> Option<Server> {
    openssl_cli()?;
    let dir = std::env::temp_dir().join(format!("sjm-legacy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ok = Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:1024", "-sha1", "-nodes", "-days", "1",
        ])
        .args([
            "-subj",
            "/CN=sql2012.test",
            "-keyout",
            "key.pem",
            "-out",
            "cert.pem",
        ])
        .current_dir(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "openssl could not create the SHA-1 test certificate");

    let port = free_port();
    let child = Command::new("openssl")
        .args(["s_server", "-accept", &port.to_string(), "-tls1", "-www"])
        .args([
            "-cert",
            "cert.pem",
            "-key",
            "key.pem",
            "-cipher",
            "DEFAULT:@SECLEVEL=0",
        ])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let server = Server { child, port, dir };
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "s_server did not start");
        thread::sleep(Duration::from_millis(50));
    }
    Some(server)
}

fn session_report(stream: &mut impl ReadWrite) -> String {
    stream.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
    let mut body = String::new();
    let _ = stream.read_to_string(&mut body);
    body
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

fn connect(port: u16) -> TcpStream {
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    tcp
}

#[test]
fn patched_opentls_reaches_a_tls10_only_server() {
    let Some(server) = tls10_server() else {
        eprintln!("skipped: no OpenSSL command-line tool");
        return;
    };
    // The same settings tiberius applies for "Trust server certificate".
    let mut builder = opentls::sync_io::TlsConnector::builder();
    builder.danger_accept_invalid_certs(true);
    builder.danger_accept_invalid_hostnames(true);
    builder.use_sni(false);
    let connector = builder.build().unwrap();

    let mut stream = connector
        .connect("sql2012.test", connect(server.port))
        .expect("legacy connector completes a TLS 1.0 handshake");
    let report = session_report(&mut stream);
    assert!(
        report.contains("New, TLSv1.0,"),
        "server reported: {report}"
    );
}

#[test]
fn stock_openssl3_refuses_the_same_server() {
    let Some(server) = tls10_server() else {
        eprintln!("skipped: no OpenSSL command-line tool");
        return;
    };
    use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode, SslVersion};
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder
        .set_min_proto_version(Some(SslVersion::TLS1))
        .unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = builder.build();
    let mut config = connector.configure().unwrap();
    config.set_verify_hostname(false);
    let error = config
        .connect("sql2012.test", connect(server.port))
        .expect_err("security level 1 must refuse TLS 1.0");
    let text = format!("{error:?}").to_lowercase();
    assert!(
        text.contains("protocol") || text.contains("no ciphers") || text.contains("version"),
        "{text}"
    );
}

#[test]
fn reports_the_linked_openssl() {
    let version = sql_job_monitor::sql::tls_backend();
    assert!(version.starts_with("OpenSSL 3"), "{version}");
}
