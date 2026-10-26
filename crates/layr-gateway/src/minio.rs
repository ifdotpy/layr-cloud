//! Opt-in integration test of the actual CLI against a local MinIO server.

use super::{auth, config};
use base64::Engine;
use layr_store::{aws::Credentials, s3::S3};
use reqwest::blocking::Client;
use reqwest::Method;
use rustls::pki_types::pem::PemObject;
use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Files(PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Bucket(S3);
impl Drop for Bucket {
    fn drop(&mut self) {
        if let Ok(objects) = self.0.list("") {
            for object in objects {
                let _ = self.0.delete(&object.key);
            }
        }
        let _ = self.0.request(
            Method::DELETE,
            "",
            &[],
            &[],
            Vec::new(),
            Duration::from_secs(10),
        );
    }
}
struct Whois {
    socket: PathBuf,
    stop: Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Whois {
    fn new(home: &Path) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let socket = home.join("whois.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o777)).unwrap();
        let login = home.join("caller-login");
        fs::write(&login, "owner@example.com").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            for incoming in listener.incoming() {
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                let mut stream = incoming.unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.starts_with("GET /localapi/v0/whois?addr="));
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                let login = fs::read_to_string(&login).unwrap();
                let body = json!({"Node":{},"UserProfile":{"LoginName":login}}).to_string();
                layr_store::http::response(
                    &mut stream,
                    200,
                    &[("Content-Type".into(), "application/json".into())],
                    body.as_bytes(),
                )
                .unwrap();
            }
        });
        Self {
            socket,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Whois {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = std::os::unix::net::UnixStream::connect(&self.socket);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn token(binary: &Path, config: &Path, gc: bool) -> String {
    let output = Command::new(binary)
        .args([
            "token",
            "example",
            if gc { "--gc" } else { "--write" },
            "--until",
            "1h",
            "--config",
        ])
        .arg(config)
        .output()
        .unwrap();
    assert!(output.status.success(), "token command failed");
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn raw(address: std::net::SocketAddr, certificate: &Path, request: &str) -> u16 {
    let socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls::pki_types::CertificateDer::pem_file_iter(certificate).unwrap() {
        roots.add(cert.unwrap()).unwrap();
    }
    let config = rustls::ClientConfig::builder_with_provider(super::tls::provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connection =
        rustls::ClientConnection::new(Arc::new(config), "localhost".try_into().unwrap()).unwrap();
    let mut stream = rustls::StreamOwned::new(connection, socket);
    stream.write_all(request.as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
#[ignore = "requires layr-dev root, a debug layr binary, and a loopback MinIO endpoint"]
fn actual_cli_minio_round_trip() {
    assert!(
        rustix::process::geteuid().is_root(),
        "run this test as root in layr-dev"
    );
    let endpoint = std::env::var("LAYR_MINIO_ENDPOINT")
        .expect("set LAYR_MINIO_ENDPOINT to a local test MinIO server");
    let url = reqwest::Url::parse(&endpoint).unwrap();
    assert_eq!(url.scheme(), "http");
    assert!(
        url.host_str()
            .unwrap()
            .parse::<std::net::IpAddr>()
            .unwrap()
            .is_loopback(),
        "MinIO must use loopback"
    );
    let credentials = Credentials {
        key_id: std::env::var("LAYR_MINIO_ACCESS_KEY").unwrap(),
        secret: std::env::var("LAYR_MINIO_SECRET_KEY").unwrap(),
        token: None,
    };
    let home = Files(
        std::env::temp_dir().join(format!("layr-gateway-minio-{:016x}", rand::random::<u64>())),
    );
    fs::create_dir(&home.0).unwrap();
    fs::set_permissions(&home.0, fs::Permissions::from_mode(0o755)).unwrap();
    let source = PathBuf::from(
        std::env::var_os("LAYR_GATEWAY_BINARY")
            .expect("set LAYR_GATEWAY_BINARY to target/debug/layr-gateway"),
    );
    let binary = home.0.join("layr-gateway");
    fs::copy(source, &binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let ca = home.0.join("ca.pem");
    let cert = home.0.join("server.pem");
    let key = home.0.join("server.key");
    fs::write(
        home.0.join("server.ext"),
        "subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n",
    )
    .unwrap();
    for arguments in [
        vec![
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=Layr Gateway Test CA",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
        vec![
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=localhost",
            "-keyout",
            "server.key",
            "-out",
            "server.csr",
        ],
        vec![
            "x509",
            "-req",
            "-in",
            "server.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-days",
            "1",
            "-extfile",
            "server.ext",
            "-out",
            "server.pem",
        ],
    ] {
        assert!(Command::new("openssl")
            .args(arguments)
            .current_dir(&home.0)
            .output()
            .unwrap()
            .status
            .success());
    }
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap(); // Disposable test key, read by the unprivileged server.
    let identity = age::x25519::Identity::generate();
    let mut config = config::sample();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    config.listen = address;
    let project = config.projects.get_mut("example").unwrap();
    project.bucket = format!("layr-gateway-test-{:016x}", rand::random::<u64>());
    project.region = "us-east-1".into();
    project.recipient = identity.to_public().to_string();
    let s3 = Bucket(
        S3::new(
            &project.bucket,
            &project.region,
            credentials.clone(),
            Some(&endpoint),
        )
        .unwrap(),
    );
    let response =
        s3.0.request(
            Method::PUT,
            "",
            &[],
            &[],
            Vec::new(),
            Duration::from_secs(10),
        )
        .unwrap();
    assert!(
        response.status().is_success(),
        "create MinIO test bucket failed"
    );
    let descriptor = json!({"format":2,"project":uuid::Uuid::now_v7().hyphenated().to_string(),"recipient":project.recipient});
    assert!(s3
        .0
        .put_new(
            &project.key("store.json"),
            descriptor.to_string().into_bytes()
        )
        .unwrap());
    config.people = Some(config::People {
        bucket: config.projects["example"].bucket.clone(),
        region: "us-east-1".into(),
        prefix: "people/".into(),
    });
    let whois = Whois::new(&home.0);
    let path = home.0.join("config.json");
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let no_store = home.0.join("must-not-create-store");
    let root = Command::new(&binary)
        .args(["serve", "--tls", "none", "--config"])
        .arg(&path)
        .env("LAYR_ROOT", &no_store)
        .output()
        .unwrap();
    assert!(!root.status.success());
    assert!(String::from_utf8(root.stderr)
        .unwrap()
        .contains("refuses to run as root"));
    let log = fs::File::create(home.0.join("server.log")).unwrap();
    let child = Command::new("setpriv")
        .args(["--reuid=nobody", "--regid=nogroup", "--clear-groups"])
        .arg(&binary)
        .args([
            "serve",
            "--tls",
            "files",
            "--test-static-credentials",
            "--cert",
        ])
        .arg(&cert)
        .arg("--key")
        .arg(&key)
        .arg("--config")
        .arg(&path)
        .arg("--tailscale-socket")
        .arg(&whois.socket)
        .env("LAYR_ROOT", &no_store)
        .env("AWS_ENDPOINT_URL_S3", &endpoint)
        .env("AWS_ACCESS_KEY_ID", &credentials.key_id)
        .env("AWS_SECRET_ACCESS_KEY", &credentials.secret)
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .unwrap();
    let _server = Server(child);
    let root = reqwest::Certificate::from_pem(&fs::read(&ca).unwrap()).unwrap();
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .add_root_certificate(root.clone())
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let origin = format!("https://{address}");
    let request = |method, target: &str| {
        client
            .request(method, format!("{origin}{target}"))
            .header("Host", "localhost")
            .header(
                "Content-Type",
                if target.ends_with("/token") {
                    "application/json"
                } else {
                    "application/octet-stream"
                },
            )
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if request(Method::GET, "/v1/p/example/challenge")
            .send()
            .is_ok()
        {
            break;
        }
        assert!(Instant::now() < deadline, "gateway did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        request(Method::GET, "/v1/p/example/store.json")
            .send()
            .unwrap()
            .json::<serde_json::Value>()
            .unwrap(),
        descriptor
    );
    assert_eq!(
        request(Method::GET, "/v1/p/example/objects?prefix=states/")
            .send()
            .unwrap()
            .status(),
        401
    );
    assert!(
        Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("{origin}/v1/p/example/store.json"))
            .send()
            .is_err(),
        "a private CA is not trusted by default"
    );
    let wrong = Client::builder()
        .no_proxy()
        .add_root_certificate(root)
        .resolve("wrong.example", address)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    assert!(
        wrong
            .get(format!(
                "https://wrong.example:{}/v1/p/example/store.json",
                address.port()
            ))
            .send()
            .is_err(),
        "hostname verification stays enabled"
    );
    let challenge: serde_json::Value = request(Method::GET, "/v1/p/example/challenge")
        .send()
        .unwrap()
        .json()
        .unwrap();
    let sealed = base64::engine::general_purpose::STANDARD
        .decode(challenge["challenge"].as_str().unwrap())
        .unwrap();
    assert!(age::decrypt(&age::x25519::Identity::generate(), &sealed).is_err());
    let proof = String::from_utf8(age::decrypt(&identity, &sealed).unwrap()).unwrap();
    let issued: serde_json::Value = request(Method::POST, "/v1/p/example/token")
        .body(json!({"challenge":proof}).to_string())
        .send()
        .unwrap()
        .json()
        .unwrap();
    let read = issued["token"].as_str().unwrap();
    let write = token(&binary, &path, false);
    let gc = token(&binary, &path, true);
    let bytes = age::encrypt(&identity.to_public(), b"the gateway cannot decrypt this").unwrap();
    let data_path = "/v1/p/example/objects/states/state/01234567.0001.data.age";
    assert_eq!(
        request(Method::PUT, data_path)
            .bearer_auth(&write)
            .body(bytes.clone())
            .send()
            .unwrap()
            .status(),
        201
    );
    assert_eq!(
        request(Method::PUT, data_path)
            .bearer_auth(&write)
            .body(b"overwrite".to_vec())
            .send()
            .unwrap()
            .status(),
        412
    );
    assert_eq!(
        request(Method::GET, data_path)
            .bearer_auth(&write)
            .send()
            .unwrap()
            .status(),
        401
    );
    let found = request(Method::GET, data_path)
        .bearer_auth(read)
        .send()
        .unwrap()
        .bytes()
        .unwrap();
    assert_eq!(found.as_ref(), bytes);
    let range = request(Method::GET, data_path)
        .bearer_auth(read)
        .header("Range", "bytes=1-5")
        .send()
        .unwrap();
    assert_eq!(range.status(), 206);
    assert_eq!(
        range.headers()["content-range"],
        format!("bytes 1-5/{}", bytes.len())
    );
    assert_eq!(range.bytes().unwrap().as_ref(), &bytes[1..=5]);
    assert_eq!(
        request(Method::GET, data_path)
            .bearer_auth(read)
            .header("Range", "bytes=9999999-")
            .send()
            .unwrap()
            .status(),
        416
    );
    let slot_path = "/v1/p/example/objects/slots/machine-new.age";
    assert_eq!(
        request(Method::PUT, slot_path)
            .bearer_auth(&write)
            .body(b"slot".to_vec())
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        request(Method::PUT, slot_path)
            .bearer_auth(read)
            .body(b"slot".to_vec())
            .send()
            .unwrap()
            .status(),
        201
    );
    assert_eq!(
        request(Method::PUT, "/v1/p/example/objects/store.json")
            .bearer_auth(&write)
            .body(b"{}".to_vec())
            .send()
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        request(Method::DELETE, slot_path)
            .bearer_auth(&gc)
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        request(Method::DELETE, data_path)
            .bearer_auth(&write)
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        request(Method::DELETE, data_path)
            .bearer_auth(&gc)
            .send()
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        request(Method::GET, data_path)
            .bearer_auth(read)
            .send()
            .unwrap()
            .status(),
        404
    );
    for (malformed, expected) in [
        ("POST /v1/p/example/token HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n", 415),
        ("POST /v1/p/example/token HTTP/1.1\r\nHost: localhost\r\nContent-Type: text/plain\r\nContent-Length: 1\r\n\r\n{", 415),
        ("POST /v1/p/example/token HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 1\r\n\r\n{", 400),
        ("GET /v1/p/example/objects/records/%2e%2e/store.json HTTP/1.1\r\nHost: localhost\r\n\r\n", 400),
        ("GET /v1/p/example/slots HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", 400),
    ] {
        assert_eq!(raw(address, &ca, malformed), expected, "request: {malformed:?}");
    }
    // Force ListObjectsV2 pagination through the actual HTTP endpoint.
    let bulk = Arc::new(
        S3::new(
            &config.projects["example"].bucket,
            "us-east-1",
            credentials,
            Some(&endpoint),
        )
        .unwrap(),
    );
    std::thread::scope(|scope| {
        for worker in 0..8 {
            let s3 = &bulk;
            scope.spawn(move || {
                for n in (worker..1005).step_by(8) {
                    assert!(s3
                        .put_new(&format!("example/records/bulk/{n:04}.age"), vec![1])
                        .unwrap());
                }
            });
        }
    });
    let names = request(Method::GET, "/v1/p/example/objects?prefix=records/")
        .bearer_auth(&write)
        .send()
        .unwrap();
    assert_eq!(names.status(), 401);
    assert_eq!(
        request(Method::GET, "/v1/p/example/objects?prefix=slots/")
            .bearer_auth(&write)
            .send()
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        request(Method::GET, "/v1/p/example/objects?prefix=")
            .bearer_auth(&write)
            .send()
            .unwrap()
            .status(),
        401
    );
    let listed: serde_json::Value = request(Method::GET, "/v1/p/example/objects?prefix=records/")
        .bearer_auth(read)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(listed["objects"].as_array().unwrap().len(), 1005);
    cli_store_round_trip(&binary, &home.0, &path, &ca, address, &endpoint, &s3.0);
    let mut changed = config::read(&path).unwrap();
    changed
        .projects
        .get_mut("example")
        .unwrap()
        .write_token_hashes
        .clear();
    fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert_eq!(
        request(Method::PUT, data_path)
            .bearer_auth(&write)
            .body(bytes)
            .send()
            .unwrap()
            .status(),
        403
    );
    assert!(!no_store.exists(), "gateway opened a local project store");
    let persisted = fs::read_to_string(&path).unwrap();
    assert!(!persisted.contains(&write));
    assert!(!persisted.contains(&gc));
    assert!(persisted.contains(&auth::hash(&gc)));
}

fn cli_store_round_trip(
    gateway: &Path,
    home: &Path,
    config_path: &Path,
    ca: &Path,
    address: std::net::SocketAddr,
    endpoint: &str,
    s3: &S3,
) {
    let layr =
        PathBuf::from(std::env::var_os("LAYR_BIN").expect("set LAYR_BIN to the layr binary"));
    let binary = layr.as_path();
    let store = PathBuf::from(
        std::env::var_os("LAYR_GATEWAY_TEST_STORE")
            .expect("run tests/t27-gateway.sh for a btrfs test store"),
    );
    let source = home.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("notes.txt"), b"first state\n").unwrap();
    let run = |root: &Path, cwd: &Path, args: &[&str], token: Option<&Path>| {
        let mut command = Command::new(binary);
        command
            .args(args)
            .current_dir(cwd)
            .env("LAYR_ROOT", root)
            .env_remove("LAYR_PROJECT")
            .env_remove("LAYR_LINE")
            .env_remove("LAYR_PAPER_KEY_FILE")
            .env_remove("LAYR_GATEWAY_TOKEN_FILE")
            .env("LAYR_GATEWAY_CA_FILE", ca)
            .env("AWS_ENDPOINT_URL_S3", endpoint)
            .env(
                "AWS_ACCESS_KEY_ID",
                std::env::var("LAYR_MINIO_ACCESS_KEY").unwrap(),
            )
            .env(
                "AWS_SECRET_ACCESS_KEY",
                std::env::var("LAYR_MINIO_SECRET_KEY").unwrap(),
            );
        if let Some(token) = token {
            command.env("LAYR_GATEWAY_TOKEN_FILE", token);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "layr {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    run(
        &store,
        home,
        &[
            "init",
            "gateway-project",
            "--from",
            source.to_str().unwrap(),
            "-q",
        ],
        None,
    );
    let line = store.join("projects/gateway-project/lines/main");
    let mut conf = config::read(config_path).unwrap();
    let bucket = conf.projects["example"].bucket.clone();
    let owner_url = format!("s3://{bucket}/cli");
    let paper = home.join("owner-paper");
    run(
        &store,
        &line,
        &[
            "remote",
            "add",
            "owner",
            &owner_url,
            "--paper-key-out",
            paper.to_str().unwrap(),
        ],
        None,
    );
    run(&store, &line, &["push", "owner", "-q"], None);
    let descriptor: serde_json::Value =
        serde_json::from_slice(&s3.get("cli/store.json").unwrap().unwrap()).unwrap();
    let mut project = conf.projects["example"].clone();
    project.prefix = "cli".into();
    project.recipient = descriptor["recipient"].as_str().unwrap().into();
    project.write_token_hashes.clear();
    project.gc_token_hashes.clear();
    conf.projects.insert("cli".into(), project);
    fs::write(config_path, serde_json::to_vec(&conf).unwrap()).unwrap();
    let issue = |gc: bool| {
        let output = Command::new(gateway)
            .args([
                "token",
                "cli",
                if gc { "--gc" } else { "--write" },
                "--config",
            ])
            .arg(config_path)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let write = issue(false);
    let gc = issue(true);
    let write_file = home.join("write-token");
    let gc_file = home.join("gc-token");
    for (path, token) in [(&write_file, &write), (&gc_file, &gc)] {
        fs::write(path, token).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let url = format!("layr+https://localhost:{}/cli", address.port());
    run(
        &store,
        &line,
        &["remote", "add", "gateway", &url],
        Some(&write_file),
    );
    run(&store, &line, &["push", "gateway", "-q"], Some(&write_file));
    // Standalone WebDAV readers open the gateway and S3 without a local project.
    for (mount_url, with_aws) in [(&url, false), (&owner_url, true)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mount_address = listener.local_addr().unwrap();
        drop(listener);
        let mount_home = home.join(if with_aws {
            "mount-s3-home"
        } else {
            "mount-gateway-home"
        });
        fs::create_dir(&mount_home).unwrap();
        let mount_root = mount_home.join("no-project-store");
        let mount_log = fs::File::create(mount_home.join("mount.log")).unwrap();
        let mut command = Command::new(binary);
        command
            .args([
                "mount",
                mount_url,
                "--listen",
                &mount_address.to_string(),
                "--paper-key-file",
            ])
            .arg(&paper)
            .env("HOME", &mount_home)
            .env("LAYR_ROOT", &mount_root)
            .env("LAYR_GATEWAY_CA_FILE", ca)
            .env("LAYR_GATEWAY_TOKEN_FILE", home.join("absent-write-grant"))
            .env_remove("AWS_ACCESS_KEY_ID")
            .env_remove("AWS_SECRET_ACCESS_KEY")
            .env_remove("AWS_SESSION_TOKEN")
            .stdout(Stdio::from(mount_log.try_clone().unwrap()))
            .stderr(Stdio::from(mount_log));
        if with_aws {
            command
                .env(
                    "AWS_ACCESS_KEY_ID",
                    std::env::var("LAYR_MINIO_ACCESS_KEY").unwrap(),
                )
                .env(
                    "AWS_SECRET_ACCESS_KEY",
                    std::env::var("LAYR_MINIO_SECRET_KEY").unwrap(),
                )
                .env("AWS_ENDPOINT_URL_S3", endpoint);
        }
        let mount = command.spawn().unwrap();
        let mount = Server(mount);
        let deadline = Instant::now() + Duration::from_secs(30);
        let reader = Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_millis(200))
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let payload = loop {
            if let Ok(response) = reader
                .get(format!("http://{mount_address}/main/notes.txt"))
                .send()
            {
                if response.status().as_u16() == 200 {
                    break response.bytes().unwrap();
                }
            }
            assert!(
                Instant::now() < deadline,
                "standalone store mount did not serve its state"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(payload.as_ref(), b"first state\n");
        assert!(!mount_root.exists(), "mount created a local project store");
        drop(mount);
    }
    let replica = store.join("gateway-replica");
    let own: serde_json::Value =
        serde_json::from_str(&run(&replica, home, &["remote", "self", "--json"], None)).unwrap();
    run(
        &store,
        &line,
        &[
            "remote",
            "trust",
            "gateway",
            own["id"].as_str().unwrap(),
            "--public-key",
            own["public_key"].as_str().unwrap(),
        ],
        None,
    );
    run(
        &replica,
        home,
        &["clone", &url, "copy", "-q"],
        Some(&write_file),
    );
    let recovery = store.join("gateway-recovery");
    let recovered = Command::new(binary)
        .args(["clone", &url, "recovery", "-q"])
        .current_dir(home)
        .env("LAYR_ROOT", &recovery)
        .env("LAYR_GATEWAY_CA_FILE", ca)
        .env("LAYR_PAPER_KEY_FILE", &paper)
        .env_remove("LAYR_PROJECT")
        .env_remove("LAYR_LINE")
        .env_remove("LAYR_GATEWAY_TOKEN_FILE")
        .output()
        .unwrap();
    assert!(
        recovered.status.success(),
        "paper-slot clone failed: {}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    let recovered_line = recovery.join("projects/recovery/lines/main");
    run(&recovery, &recovered_line, &["fetch", "origin", "-q"], None);
    let copy = replica.join("projects/copy/lines/main");
    assert_eq!(fs::read(copy.join("notes.txt")).unwrap(), b"first state\n");
    run(&replica, &copy, &["fetch", "origin", "-q"], None);
    run(
        &replica,
        &copy,
        &["fsck", "--remote", "origin", "--files"],
        None,
    );
    fs::write(copy.join("notes.txt"), b"second state\n").unwrap();
    run(&replica, &copy, &["add", "-A"], None);
    run(
        &replica,
        &copy,
        &["save", "-m", "gateway second state", "-q"],
        None,
    );
    // A seeded machine can publish with only the write grant, without its slot.
    let slot = format!("cli/slots/machine-{}.age", own["id"].as_str().unwrap());
    s3.delete(&slot).unwrap();
    run(
        &replica,
        &copy,
        &["push", "origin", "-q"],
        Some(&write_file),
    );
    run(&store, &line, &["fetch", "gateway", "-q"], None);
    run(
        &store,
        &line,
        &[
            "remote",
            "trust",
            "gateway",
            own["id"].as_str().unwrap(),
            "--public-key",
            own["public_key"].as_str().unwrap(),
        ],
        None,
    );
    run(&replica, &copy, &["fetch", "origin", "-q"], None);
    run(
        &replica,
        &copy,
        &["gc", "--remote", "origin", "--json"],
        Some(&gc_file),
    );
    run(
        &replica,
        &copy,
        &["fsck", "--remote", "origin", "--files"],
        None,
    );
    fs::set_permissions(&write_file, fs::Permissions::from_mode(0o644)).unwrap();
    let denied = Command::new(binary)
        .args(["push", "origin", "-q"])
        .current_dir(&copy)
        .env("LAYR_ROOT", &replica)
        .env("LAYR_GATEWAY_CA_FILE", ca)
        .env("LAYR_GATEWAY_TOKEN_FILE", &write_file)
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("mode 0600"));
    for token in [&write, &gc] {
        assert!(!String::from_utf8_lossy(&denied.stderr).contains(token));
        for database in [
            store.join("projects/gateway-project/meta/db/layr.sqlite"),
            replica.join("projects/copy/meta/db/layr.sqlite"),
        ] {
            let data = fs::read(database).unwrap();
            assert!(
                !data
                    .windows(token.len())
                    .any(|window| window == token.as_bytes()),
                "a project database contains a gateway token"
            );
        }
    }
    person_gateway_round_trip(binary, &store, &line, &url, home, ca);
    fs::write(home.join("caller-login"), "owner@example.com").unwrap();
    fs::set_permissions(&write_file, fs::Permissions::from_mode(0o600)).unwrap();
    {
        // A reader and a machine with only the write token use the same published layer key.
        let owner: serde_json::Value =
            serde_json::from_str(&run(&store, &line, &["remote", "self", "--json"], None)).unwrap();
        let reader_root = store.join("overlay-reader");
        let reader: serde_json::Value = serde_json::from_str(&run(
            &reader_root,
            home,
            &["remote", "self", "--json"],
            None,
        ))
        .unwrap();
        let writer_root = PathBuf::from(
            std::env::var_os("LAYR_GATEWAY_TEST_WRITER_ROOT")
                .expect("run tests/t27-gateway.sh for a separate writer filesystem"),
        );
        let other_root = store.join("overlay-nonreader");
        let writer: serde_json::Value = serde_json::from_str(&run(
            &writer_root,
            home,
            &["remote", "self", "--json"],
            None,
        ))
        .unwrap();
        let other: serde_json::Value =
            serde_json::from_str(&run(&other_root, home, &["remote", "self", "--json"], None))
                .unwrap();
        for machine in [&reader, &writer, &other] {
            run(
                &store,
                &line,
                &[
                    "remote",
                    "trust",
                    "gateway",
                    machine["id"].as_str().unwrap(),
                    "--public-key",
                    machine["public_key"].as_str().unwrap(),
                ],
                None,
            );
        }
        let readers = format!(
            "machine:{},machine:{}",
            owner["id"].as_str().unwrap(),
            reader["id"].as_str().unwrap()
        );
        run(
            &store,
            &line,
            &["overlay", "create", "secrets", "--readers", &readers, "-q"],
            None,
        );
        fs::write(line.join(".env"), b"PRIVATE_TEST_VALUE=gateway-owner\n").unwrap();
        run(
            &store,
            &line,
            &["overlay", "move", "secrets", ".env", "--purge", "-q"],
            None,
        );
        run(&store, &line, &["save", "-am", "Owner overlay", "-q"], None);
        run(&store, &line, &["push", "gateway", "-q"], Some(&write_file));
        let objects = s3.list("cli/overlays/").unwrap();
        let key = objects
            .iter()
            .find(|object| object.key.contains("/keys/"))
            .expect("owner did not publish an overlay key");
        let published: serde_json::Value =
            serde_json::from_slice(&s3.get(&key.key).unwrap().unwrap()).unwrap();
        let data = &published["r"]["data"];
        let epoch_prefix = format!(
            "cli/overlays/{}/epochs/{}/",
            data["overlay"].as_str().unwrap(),
            data["epoch"].as_str().unwrap()
        );
        let states_before = s3.list(&format!("{epoch_prefix}states/")).unwrap().len();
        assert!(states_before >= 2);
        assert!(s3
            .get(&format!(
                "{epoch_prefix}slots/machine-{}.age",
                reader["id"].as_str().unwrap()
            ))
            .unwrap()
            .is_some());
        assert!(s3
            .get(&format!(
                "{epoch_prefix}slots/machine-{}.age",
                writer["id"].as_str().unwrap()
            ))
            .unwrap()
            .is_none());
        run(
            &store,
            &line,
            &[
                "remote",
                "add",
                "overlay-writer",
                "ssh://test/gateway-project",
            ],
            None,
        );
        let peers = json!([reader["id"], writer["id"]]).to_string();
        run(
            &store,
            &line,
            &["config", "overlay.secrets.peers", &peers],
            None,
        );
        run(
            &reader_root,
            home,
            &["clone", &url, "overlay-copy", "-q"],
            None,
        );
        let reader_line = reader_root.join("projects/overlay-copy/lines/main");
        assert_eq!(
            fs::read(reader_line.join(".env")).unwrap(),
            b"PRIVATE_TEST_VALUE=gateway-owner\n"
        );
        let writer_env = format!("LAYR_ROOT={}", writer_root.display());
        run(
            &store,
            &line,
            &[
                "sync",
                "overlay-writer",
                "--command",
                "--",
                "env",
                &writer_env,
                binary.to_str().unwrap(),
                "serve-peer",
                "gateway-project",
            ],
            None,
        );
        run(
            &writer_root,
            home,
            &["--project", "gateway-project", "adopt", "main"],
            None,
        );
        let writer_line = writer_root.join("projects/gateway-project/lines/main");
        run(
            &writer_root,
            &writer_line,
            &["overlay", "use", "secrets", "-q"],
            None,
        );
        fs::write(
            writer_line.join(".env"),
            b"PRIVATE_TEST_VALUE=gateway-write-only\n",
        )
        .unwrap();
        run(
            &writer_root,
            &writer_line,
            &["save", "-am", "Write-only overlay", "-q"],
            None,
        );
        run(
            &writer_root,
            &writer_line,
            &["remote", "add", "gateway", &url],
            Some(&write_file),
        );
        s3.delete(&format!(
            "cli/slots/machine-{}.age",
            writer["id"].as_str().unwrap()
        ))
        .unwrap();
        run(
            &writer_root,
            &writer_line,
            &["push", "gateway", "-q"],
            Some(&write_file),
        );
        assert!(s3.list(&format!("{epoch_prefix}states/")).unwrap().len() > states_before);
        assert!(s3
            .get(&format!(
                "{epoch_prefix}slots/machine-{}.age",
                writer["id"].as_str().unwrap()
            ))
            .unwrap()
            .is_none());
        run(&reader_root, &reader_line, &["fetch", "origin", "-q"], None);
        run(
            &reader_root,
            &reader_line,
            &["overlay", "use", "secrets", "-q"],
            None,
        );
        assert_eq!(
            fs::read(reader_line.join(".env")).unwrap(),
            b"PRIVATE_TEST_VALUE=gateway-write-only\n"
        );
        let report: serde_json::Value = serde_json::from_str(&run(
            &other_root,
            home,
            &["clone", &url, "shared-only", "--json"],
            None,
        ))
        .unwrap();
        assert!(report["omitted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some("overlay secrets: no key on this machine")));
        let other_line = other_root.join("projects/shared-only/lines/main");
        assert!(other_line.join("notes.txt").exists());
        assert!(!other_line.join(".env").exists());
        assert!(s3
            .get(&format!(
                "{epoch_prefix}slots/machine-{}.age",
                other["id"].as_str().unwrap()
            ))
            .unwrap()
            .is_none());
        println!("PASS gateway overlay owner, write-only machine, reader fetch, and non-reader boundaries");
        // A non-reader can save shared files without a local overlay project.
        fs::write(other_line.join("notes.txt"), b"non-reader shared edit\n").unwrap();
        run(
            &other_root,
            &other_line,
            &["save", "-am", "Non-reader shared files", "-q"],
            None,
        );
        assert_eq!(
            fs::read(other_line.join("notes.txt")).unwrap(),
            b"non-reader shared edit\n"
        );
        assert!(!other_line.join(".env").exists());
        println!("PASS a non-reader clone can save shared files");
    }
}

fn person_gateway_round_trip(
    binary: &Path,
    owner_root: &Path,
    line: &Path,
    url: &str,
    home: &Path,
    ca: &Path,
) {
    let site = url.strip_prefix("layr+").unwrap();
    let owner_dir = home.join("owner-person");
    let owner_words = home.join("owner-words");
    let carol_dir = home.join("carol-person");
    let carol_words = home.join("carol-words");
    let carol_root = owner_root.join("person-carol-root");
    let call = |who: &str, root: &Path, dir: &Path, words: &Path, args: &[&str]| {
        fs::write(home.join("caller-login"), who).unwrap();
        Command::new(binary)
            .args(args)
            .current_dir(line)
            .env("LAYR_ROOT", root)
            .env("LAYR_PERSON_DIR", dir)
            .env("LAYR_PERSON_KEY_FILE", words)
            .env("LAYR_PERSON_LOGIN", "forged@example.com")
            .env("LAYR_GATEWAY_CA_FILE", ca)
            .env_remove("LAYR_PAPER_KEY_FILE")
            .env_remove("LAYR_PROJECT")
            .env_remove("LAYR_LINE")
            .env_remove("LAYR_GATEWAY_TOKEN_FILE")
            .env_remove("AWS_ACCESS_KEY_ID")
            .env_remove("AWS_SECRET_ACCESS_KEY")
            .output()
            .unwrap()
    };
    let ok = |output: std::process::Output| {
        assert!(
            output.status.success(),
            "person CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    for (who, root, dir, words) in [
        ("owner@example.com", owner_root, &owner_dir, &owner_words),
        (
            "carol@example.com",
            carol_root.as_path(),
            &carol_dir,
            &carol_words,
        ),
    ] {
        ok(call(
            who,
            root,
            dir,
            words,
            &[
                "person",
                "keys",
                "--site",
                site,
                "--paper-key-out",
                words.to_str().unwrap(),
            ],
        ));
    }
    let owner: serde_json::Value = serde_json::from_str(&ok(call(
        "owner@example.com",
        owner_root,
        &owner_dir,
        &owner_words,
        &["person", "show", "--site", site, "--json"],
    )))
    .unwrap();
    let carol: serde_json::Value = serde_json::from_str(&ok(call(
        "carol@example.com",
        &carol_root,
        &carol_dir,
        &carol_words,
        &["person", "show", "--site", site, "--json"],
    )))
    .unwrap();
    assert_eq!(owner["login"], "owner@example.com");
    assert_eq!(carol["login"], "carol@example.com");
    let owner_recipient = owner["recipient"].as_str().unwrap();
    let carol_recipient = carol["recipient"].as_str().unwrap();
    let refused = call(
        "owner@example.com",
        owner_root,
        &owner_dir,
        &owner_words,
        &[
            "remote",
            "grant",
            "gateway",
            "--person",
            "carol@example.com",
            "--recipient",
            carol_recipient,
        ],
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("grant yourself first"));
    ok(call(
        "owner@example.com",
        owner_root,
        &owner_dir,
        &owner_words,
        &[
            "remote",
            "grant",
            "gateway",
            "--person",
            "owner@example.com",
            "--recipient",
            owner_recipient,
        ],
    ));
    let client = Client::builder()
        .no_proxy()
        .add_root_certificate(reqwest::Certificate::from_pem(&fs::read(ca).unwrap()).unwrap())
        .build()
        .unwrap();
    let origin = reqwest::Url::parse(url.strip_prefix("layr+").unwrap()).unwrap();
    let view = || {
        client
            .get(origin.join("/api/person").unwrap())
            .send()
            .unwrap()
            .json::<serde_json::Value>()
            .unwrap()
    };
    let record = view();
    let id = record["inbox"][0]["id"].as_str().unwrap();
    ok(call(
        "owner@example.com",
        owner_root,
        &owner_dir,
        &owner_words,
        &["person", "accept", "--site", site, "--invite", id],
    ));
    ok(call(
        "owner@example.com",
        owner_root,
        &owner_dir,
        &owner_words,
        &[
            "remote",
            "grant",
            "gateway",
            "--person",
            "carol@example.com",
            "--recipient",
            carol_recipient,
        ],
    ));
    let again = ok(call(
        "owner@example.com",
        owner_root,
        &owner_dir,
        &owner_words,
        &[
            "remote",
            "grant",
            "gateway",
            "--person",
            "carol@example.com",
            "--recipient",
            carol_recipient,
        ],
    ));
    assert!(again.contains("already granted"));
    fs::write(home.join("caller-login"), "carol@example.com").unwrap();
    let record = view();
    assert_eq!(record["inbox"].as_array().unwrap().len(), 2);
    let id = record["inbox"][0]["id"].as_str().unwrap();
    ok(call(
        "carol@example.com",
        &carol_root,
        &carol_dir,
        &carol_words,
        &["person", "accept", "--site", site, "--invite", id],
    ));
    ok(call(
        "carol@example.com",
        &carol_root,
        &carol_dir,
        &carol_words,
        &["clone", url, "person-copy", "-q"],
    ));
    assert_eq!(
        fs::read(carol_root.join("projects/person-copy/lines/main/notes.txt")).unwrap(),
        b"second state\n"
    );
    fs::write(home.join("caller-login"), "stranger@example.com").unwrap();
    for action in ["store.json", "slots", "challenge", "objects", "other/path"] {
        let target = origin.join(&format!("/v1/p/cli/{action}")).unwrap();
        assert_eq!(client.get(target).send().unwrap().status(), 404);
    }
    assert_eq!(
        client
            .post(origin.join("/api/people/carol@example.com/inbox").unwrap())
            .header("Origin", "https://other.example")
            .json(&json!({"data":"AA=="}))
            .send()
            .unwrap()
            .status(),
        403
    );
}
