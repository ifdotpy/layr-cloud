use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static NEXT_FOLDER: AtomicU64 = AtomicU64::new(0);
type Captured = (String, BTreeMap<String, String>, Vec<u8>);

struct Release(PathBuf);
impl Release {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "layr-deploy-{}-{}-{}",
            std::process::id(),
            NEXT_FOLDER.fetch_add(1, Ordering::SeqCst),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("deploy.json"),
            json!({"imageId":"ami-0123456789abcdef0", "gatewayHost":"demo-gateway.tail.test"})
                .to_string(),
        )
        .unwrap();
        // The hosts validate ELF contents. The deployer uploads these bytes unchanged.
        std::fs::write(root.join("gateway.tgz"), b"gateway-fixture").unwrap();
        std::fs::write(root.join("executor.tgz"), b"executor-fixture").unwrap();
        Self(root)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_layr-cloud"));
        command
            .args([
                "deploy",
                "--region",
                "eu-central-1",
                "--stack-prefix",
                "demo",
                "--release",
            ])
            .arg(&self.0);
        command
            .env("HOME", &self.0)
            .env("AWS_CONFIG_FILE", self.0.join("config"))
            .env("AWS_SHARED_CREDENTIALS_FILE", self.0.join("credentials"));
        command
            .env_remove("AWS_PROFILE")
            .env_remove("AWS_ACCESS_KEY_ID")
            .env_remove("AWS_SECRET_ACCESS_KEY")
            .env_remove("AWS_SESSION_TOKEN");
        command
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

struct Fake {
    url: String,
    requests: Arc<Mutex<Vec<Captured>>>,
    replies: Arc<Mutex<VecDeque<(u16, String)>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Fake {
    fn new(replies: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen, queue, done) = (requests.clone(), replies.clone(), stop.clone());
        let worker = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(at) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                assert!(headers
                    .to_lowercase()
                    .contains("authorization: aws4-hmac-sha256"));
                let length: usize = headers
                    .lines()
                    .find_map(|s| {
                        s.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .unwrap()
                    .parse()
                    .unwrap();
                while bytes.len() < header_end + length {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                let body = bytes[header_end..header_end + length].to_vec();
                let first = headers.lines().next().unwrap().to_string();
                let form = reqwest::Url::parse(&format!(
                    "http://test/?{}",
                    String::from_utf8_lossy(&body)
                ))
                .unwrap()
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
                seen.lock().unwrap().push((first, form, body));
                let (code, reply) = queue
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected AWS call");
                write!(socket, "HTTP/1.1 {code} Reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
            }
        });
        Self {
            url,
            requests,
            replies,
            stop,
            worker: Some(worker),
        }
    }
    fn run(&self, release: &Release) -> Value {
        let mut command = release.command();
        command
            .env("AWS_ACCESS_KEY_ID", "fake-id")
            .env("AWS_SECRET_ACCESS_KEY", "fake-secret");
        for service in ["CLOUDFORMATION", "S3", "STS", "EC2"] {
            command.env(format!("AWS_ENDPOINT_URL_{service}"), &self.url);
        }
        success(command.output().unwrap())
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let outcome = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            outcome.unwrap();
        }
    }
}
fn ok(body: &str) -> (u16, String) {
    (200, body.into())
}
fn error(message: &str) -> (u16, String) {
    (
        400,
        format!("<Error><Code>ValidationError</Code><Message>{message}</Message></Error>"),
    )
}
fn stack(status: &str, outputs: &[(&str, &str)]) -> (u16, String) {
    let outputs: String = outputs
        .iter()
        .map(|(key, value)| {
            format!(
                "<member><OutputKey>{key}</OutputKey><OutputValue>{value}</OutputValue></member>"
            )
        })
        .collect();
    ok(&format!("<Stacks><member><StackStatus>{status}</StackStatus><Outputs>{outputs}</Outputs></member></Stacks>"))
}
fn replies(update: bool, no_updates: bool) -> Vec<(u16, String)> {
    let mut replies = vec![ok("<GetCallerIdentityResponse><Account>111122223333</Account><Arn>arn:aws:iam::111122223333:user/test</Arn></GetCallerIdentityResponse>")];
    let outputs = [
        vec![("ArtifactsBucket", "artifacts")],
        vec![
            ("BucketName", "store"),
            ("StoreUrl", "s3://store/demo"),
            ("ReaderRoleArn", "reader"),
            ("WriterRoleArn", "writer"),
            ("GcRoleArn", "gc"),
        ],
        vec![("SubnetId", "subnet-1"), ("VpcId", "vpc-1")],
        vec![("InstanceId", "executor-1")],
    ];
    for (index, outputs) in outputs.iter().enumerate() {
        if index == 3 {
            replies.push(ok("<DescribeSubnetsResponse><subnetSet><item><availabilityZone>eu-central-1a</availabilityZone></item></subnetSet></DescribeSubnetsResponse>"));
        }
        replies.push(if update {
            stack("CREATE_COMPLETE", outputs)
        } else {
            error("Stack demo does not exist")
        });
        replies.push(if no_updates {
            error("No updates are to be performed.")
        } else {
            ok("<StackResponse/>")
        });
        replies.push(stack(
            if update && !no_updates {
                "UPDATE_COMPLETE"
            } else {
                "CREATE_COMPLETE"
            },
            outputs,
        ));
        if index == 0 {
            for _ in 0..4 {
                replies.push(ok(""));
            }
        }
    }
    replies
}

#[test]
fn command_creates_then_updates_stacks_and_uploads_archives() {
    let release = Release::new();
    for (update, no_updates) in [(false, false), (true, false), (true, true)] {
        let fake = Fake::new(replies(update, no_updates));
        let result = fake.run(&release);
        assert_eq!(
            result["outputs"]["gatewayUrl"],
            "https://demo-gateway.tail.test"
        );
        assert_eq!(result["outputs"]["artifactsBucket"], "artifacts");
        assert_eq!(result["outputs"]["storeBucket"], "store");
        let requests = fake.requests.lock().unwrap();
        let mutations: Vec<_> = requests
            .iter()
            .filter(|(_, form, _)| {
                form.get("Action")
                    .is_some_and(|s| s == "CreateStack" || s == "UpdateStack")
            })
            .collect();
        assert_eq!(
            mutations
                .iter()
                .map(|(_, form, _)| form["StackName"].as_str())
                .collect::<Vec<_>>(),
            [
                "demo-bootstrap",
                "demo-store",
                "demo-gateway",
                "demo-executor"
            ]
        );
        assert!(mutations.iter().all(
            |(_, form, _)| form["Action"] == if update { "UpdateStack" } else { "CreateStack" }
        ));
        let parameters = |request: &&Captured| -> BTreeMap<String, String> {
            request
                .1
                .iter()
                .filter_map(|(key, value)| {
                    key.strip_suffix(".ParameterKey").map(|prefix| {
                        (
                            value.clone(),
                            request.1[&format!("{prefix}.ParameterValue")].clone(),
                        )
                    })
                })
                .collect()
        };
        assert_eq!(parameters(&mutations[1])["GatewayRoleArns"], "arn:aws:iam::111122223333:role/demo-gateway,arn:aws:iam::111122223333:role/demo-executor");
        assert_eq!(
            parameters(&mutations[2])["StoreRoleArns"],
            "reader,writer,gc"
        );
        assert_eq!(
            parameters(&mutations[3])["AvailabilityZone"],
            "eu-central-1a"
        );
        let uploads: Vec<_> = requests
            .iter()
            .filter(|(first, _, _)| first.starts_with("PUT "))
            .collect();
        assert_eq!(uploads.len(), 4);
        assert_eq!(uploads[0].2, b"gateway-fixture");
        assert_eq!(uploads[2].2, b"executor-fixture");
        for index in [1, 3] {
            let descriptor: Value = serde_json::from_slice(&uploads[index].2).unwrap();
            assert!(uploads[index - 1]
                .0
                .contains(descriptor["key"].as_str().unwrap()));
        }
        assert!(fake.replies.lock().unwrap().is_empty());
    }
}

#[test]
fn dry_run_prints_parameters_without_credentials_or_http_calls() {
    let release = Release::new();
    let fake = Fake::new(Vec::new());
    let mut command = release.command();
    command
        .arg("--dry-run")
        .env("AWS_ENDPOINT_URL_CLOUDFORMATION", &fake.url)
        .env("AWS_ENDPOINT_URL_S3", &fake.url)
        .env("AWS_ENDPOINT_URL_STS", &fake.url)
        .env("AWS_ENDPOINT_URL_EC2", &fake.url);
    let result = success(command.output().unwrap());
    assert_eq!(result["dryRun"], true);
    assert_eq!(result["stacks"].as_array().unwrap().len(), 4);
    assert_eq!(result["stacks"][1]["parameters"]["Prefix"], "demo");
    assert_eq!(
        result["stacks"][2]["parameters"]["ImageId"],
        "ami-0123456789abcdef0"
    );
    assert_eq!(
        result["stacks"][3]["parameters"]["SubnetId"],
        "<demo-gateway.SubnetId>"
    );
    assert_eq!(
        result["outputs"]["gatewayUrl"],
        "https://demo-gateway.tail.test"
    );
    assert!(fake.requests.lock().unwrap().is_empty());
}

#[test]
fn deploy_config_refuses_unknown_fields_before_http_calls() {
    let release = Release::new();
    std::fs::write(
        release.0.join("deploy.json"),
        json!({"imageId":"ami-0123456789abcdef0", "gatewayHost":"gateway.tail.test", "typo":true})
            .to_string(),
    )
    .unwrap();
    let output = release.command().arg("--dry-run").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown field `typo`"));
}

#[test]
fn sso_profile_exits_two_and_environment_credentials_take_precedence() {
    let release = Release::new();
    std::fs::write(
        release.0.join("config"),
        "[profile work]\nsso_session = work\n",
    )
    .unwrap();
    let output = release
        .command()
        .args(["--profile", "work"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("SSO profiles need layr remote login first"));
    // AWS_PROFILE is used when --profile is absent.
    let output = release
        .command()
        .env("AWS_PROFILE", "work")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let fake = Fake::new(replies(false, false));
    fake.run(&release);
}

#[test]
fn static_key_profile_uses_standard_credentials_file() {
    let release = Release::new();
    std::fs::write(release.0.join("credentials"), "[work]\naws_access_key_id = fake-id\naws_secret_access_key = fake-secret\naws_session_token = fake-token\n").unwrap();
    let fake = Fake::new(replies(false, false));
    let mut command = release.command();
    command.args(["--profile", "work"]);
    for service in ["CLOUDFORMATION", "S3", "STS", "EC2"] {
        command.env(format!("AWS_ENDPOINT_URL_{service}"), &fake.url);
    }
    success(command.output().unwrap());
}

#[test]
fn dry_run_supports_new_or_existing_dns_zone_and_ssm_names() {
    let release = Release::new();
    for zone in [None, Some("Z12345")] {
        let mut config = json!({"imageId":"ami-0123456789abcdef0", "gatewayHost":"demo.tail.test", "domain":"cloud.example.test", "gatewayAuthKeyParameter":"/demo/gateway", "executorAuthKeyParameter":"/demo/executor"});
        if let Some(zone) = zone {
            config["zoneId"] = zone.into();
        }
        std::fs::write(release.0.join("deploy.json"), config.to_string()).unwrap();
        let output = release.command().arg("--dry-run").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stderr).contains("creates a new Route 53 hosted zone cloud.example.test; delegate it from your registrar"), zone.is_none());
        let result = success(output);
        assert_eq!(
            result["outputs"]["gatewayUrl"],
            "https://cloud.example.test"
        );
        let stacks = result["stacks"].as_array().unwrap();
        assert_eq!(stacks.len(), if zone.is_some() { 4 } else { 5 });
        let gateway = stacks.iter().find(|v| v["name"] == "demo-gateway").unwrap();
        assert_eq!(gateway["parameters"]["Host"], "cloud.example.test");
        assert_eq!(gateway["parameters"]["AuthKeyParameter"], "/demo/gateway");
        assert_eq!(
            gateway["parameters"]["ZoneId"],
            zone.unwrap_or("<demo-dns.ZoneId>")
        );
    }
}
