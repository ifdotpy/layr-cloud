use crate::account::{Account, Parameters};
use layr_store::aws::Credentials;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

static ENVIRONMENT: Mutex<()> = Mutex::new(());
type Request = (String, String, String);
struct Fake {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    done: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    replies: Arc<Mutex<VecDeque<(u16, String)>>>,
}
impl Fake {
    fn new(replies: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let (seen, stop, queue) = (requests.clone(), done.clone(), replies.clone());
        let thread = thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let (header_end, length) = loop {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).unwrap();
                    assert_ne!(n, 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(at) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..at]);
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
                        break (at + 4, length);
                    }
                };
                while bytes.len() < header_end + length {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).unwrap();
                    assert_ne!(n, 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let body =
                    String::from_utf8_lossy(&bytes[header_end..header_end + length]).to_string();
                let path = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_string();
                seen.lock().unwrap().push((path, headers, body));
                let (status, body) = queue
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected request");
                write!(stream, "HTTP/1.1 {status} Reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            url,
            requests,
            done,
            thread: Some(thread),
            replies,
        }
    }
    fn account(&self) -> Account {
        // Tests hold ENVIRONMENT until Fake is dropped.
        std::env::set_var("AWS_ENDPOINT_URL_CLOUDFORMATION", &self.url);
        std::env::set_var("AWS_ENDPOINT_URL_S3", &self.url);
        let mut account = Account::new(
            Credentials {
                key_id: "example-id".into(),
                secret: "example-secret".into(),
                token: Some("example-token".into()),
            },
            "eu-central-1",
        )
        .unwrap();
        account.poll = Duration::from_millis(1);
        account
    }
    fn assert_done(&self) {
        assert!(self.replies.lock().unwrap().is_empty());
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        let outcome = self.thread.take().unwrap().join();
        if !std::thread::panicking() {
            outcome.unwrap();
        }
        std::env::remove_var("AWS_ENDPOINT_URL_CLOUDFORMATION");
        std::env::remove_var("AWS_ENDPOINT_URL_S3");
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
fn stack(status: &str) -> (u16, String) {
    ok(&format!("<DescribeStacksResponse><Stacks><member><StackStatus>{status}</StackStatus><Outputs><member><OutputKey>BucketName</OutputKey><OutputValue>test-bucket</OutputValue></member></Outputs></member></Stacks></DescribeStacksResponse>"))
}
fn form(body: &str) -> Parameters {
    reqwest::Url::parse(&format!("http://test/?{body}"))
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

#[test]
fn create_waits_and_sends_template_parameters_with_signature() {
    let _guard = ENVIRONMENT.lock().unwrap();
    let fake = Fake::new(vec![
        error("Stack demo does not exist"),
        ok("<CreateStackResponse><StackId>stack-id</StackId></CreateStackResponse>"),
        stack("CREATE_IN_PROGRESS"),
        stack("CREATE_COMPLETE"),
    ]);
    let outputs = fake
        .account()
        .deploy_stack(
            "demo",
            "Resources: {}",
            &Parameters::from([("Prefix".into(), "demo".into())]),
        )
        .unwrap();
    assert_eq!(outputs["BucketName"], "test-bucket");
    let requests = fake.requests.lock().unwrap();
    let create = form(&requests[1].2);
    assert_eq!(create["Action"], "CreateStack");
    assert_eq!(create["TemplateBody"], "Resources: {}");
    assert_eq!(create["Parameters.member.1.ParameterKey"], "Prefix");
    assert_eq!(create["Parameters.member.1.ParameterValue"], "demo");
    assert_eq!(create["Capabilities.member.1"], "CAPABILITY_NAMED_IAM");
    assert_eq!(create["OnFailure"], "ROLLBACK");
    assert_eq!(form(&requests[2].2)["StackName"], "stack-id");
    assert!(requests[1]
        .1
        .contains("/eu-central-1/cloudformation/aws4_request"));
    assert!(requests[1]
        .1
        .contains("x-amz-security-token: example-token"));
    fake.assert_done();
}

#[test]
fn update_waits_for_completion() {
    let _guard = ENVIRONMENT.lock().unwrap();
    let fake = Fake::new(vec![
        stack("CREATE_COMPLETE"),
        ok("<UpdateStackResponse/>"),
        stack("UPDATE_IN_PROGRESS"),
        stack("UPDATE_COMPLETE"),
    ]);
    fake.account()
        .deploy_stack("demo", "Resources: {}", &Parameters::new())
        .unwrap();
    let requests = fake.requests.lock().unwrap();
    assert_eq!(form(&requests[1].2)["Action"], "UpdateStack");
    assert!(!form(&requests[1].2).contains_key("OnFailure"));
    fake.assert_done();
}

#[test]
fn no_updates_is_success() {
    let _guard = ENVIRONMENT.lock().unwrap();
    let fake = Fake::new(vec![
        stack("CREATE_COMPLETE"),
        error("No updates are to be performed."),
        stack("CREATE_COMPLETE"),
    ]);
    assert_eq!(
        fake.account()
            .deploy_stack("demo", "Resources: {}", &Parameters::new())
            .unwrap()["BucketName"],
        "test-bucket"
    );
    fake.assert_done();
}

#[test]
fn failed_create_names_failed_resource_events() {
    let _guard = ENVIRONMENT.lock().unwrap();
    let fake = Fake::new(vec![error("Stack demo does not exist"), ok("<CreateStackResponse><StackId>failed-id</StackId></CreateStackResponse>"), stack("ROLLBACK_COMPLETE"), ok("<StackEvents><member><LogicalResourceId>GatewayInstance</LogicalResourceId><ResourceStatus>CREATE_FAILED</ResourceStatus><ResourceStatusReason>AMI is invalid</ResourceStatusReason></member><member><LogicalResourceId>Network</LogicalResourceId><ResourceStatus>CREATE_COMPLETE</ResourceStatus></member></StackEvents>")]);
    let message = fake
        .account()
        .deploy_stack("demo", "Resources: {}", &Parameters::new())
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("GatewayInstance: CREATE_FAILED: AMI is invalid"),
        "{message}"
    );
    assert_eq!(
        form(&fake.requests.lock().unwrap()[3].2)["StackName"],
        "failed-id"
    );
    fake.assert_done();
}

#[test]
fn wait_timeout_stops_describe_calls() {
    let _guard = ENVIRONMENT.lock().unwrap();
    let fake = Fake::new(vec![
        error("Stack demo does not exist"),
        ok("<CreateStackResponse/>"),
        stack("CREATE_IN_PROGRESS"),
    ]);
    let mut account = fake.account();
    account.wait = Duration::from_millis(100);
    account.poll = Duration::from_secs(1);
    let error = account
        .deploy_stack("demo", "Resources: {}", &Parameters::new())
        .unwrap_err();
    assert!(error.to_string().contains("wait timeout"), "{error}");
    assert_eq!(fake.requests.lock().unwrap().len(), 3);
    fake.assert_done();
}

#[test]
fn s3_upload_uses_override_and_store_sigv4() {
    let _guard = ENVIRONMENT.lock().unwrap();
    let fake = Fake::new(vec![ok("")]);
    fake.account()
        .put(
            "artifacts",
            "gateway/releases/test.tgz",
            b"archive".to_vec(),
            "application/gzip",
        )
        .unwrap();
    let requests = fake.requests.lock().unwrap();
    assert_eq!(requests[0].0, "/artifacts/gateway/releases/test.tgz");
    assert_eq!(requests[0].2, "archive");
    assert!(requests[0].1.contains("/eu-central-1/s3/aws4_request"));
    fake.assert_done();
}
