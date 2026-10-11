//! The person key end to end: `layr-desk serve` runs against a fake AWS (STS and path-style S3
//! that check each signature and refuse a header sent twice) and a fake tailscaled that names the
//! caller. `person keys` registers Ann's and Bob's keys; `grant` gives them the vault, once with
//! the vault's paper key and once with Ann's person key; the gateway shows the vault only to
//! them; and `person vaults` checks the vault.json of the gateway against Ann's pin.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use bytes::Bytes;
use desk_core::person::{self, Index, Pin};
use desk_core::{keys, VaultInfo};
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response};
use percent_encoding::percent_decode_str;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};

const ANN: &str = "ann@example.com";
const BOB: &str = "bob@example.com";
const REGION: &str = "eu-central-1";
/// Key id, secret and session token of each caller of the fake AWS.
const GATEWAY: (&str, &str, Option<&str>) = ("AKIAGATEWAY", "gateway-secret", None);
const ROLE: (&str, &str, Option<&str>) = ("ASIAROLE", "role/secret", Some("role-token"));
const OWNER: (&str, &str, Option<&str>) = ("AKIAOWNER", "owner+secret", None);

type Objects = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

fn reply(status: u16, body: impl Into<Bytes>) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(body.into()));
    *response.status_mut() = hyper::StatusCode::from_u16(status).unwrap();
    response
}

fn decode(text: &str) -> String {
    percent_decode_str(text).decode_utf8().unwrap().into_owned()
}

/// The key id whose valid signature the request carries, or why not.
fn signer(parts: &hyper::http::request::Parts, body: &[u8]) -> Result<&'static str, String> {
    // AWS joins the values of a header sent twice, so such a request never matches its signature.
    for name in parts.headers.keys() {
        if parts.headers.get_all(name).iter().count() > 1 {
            return Err(format!("the header {name} is sent twice"));
        }
    }
    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    };
    let auth = header("authorization");
    let key_id = auth
        .split("Credential=")
        .nth(1)
        .and_then(|c| c.split('/').next())
        .unwrap_or_default();
    let (id, secret, token) = [GATEWAY, ROLE, OWNER]
        .into_iter()
        .find(|(id, _, _)| *id == key_id)
        .ok_or_else(|| format!("unknown key {key_id:?}"))?;
    let scope: Vec<&str> = auth.split('/').collect();
    let (region, service) = (scope[2], scope[3]);
    let names = auth
        .split("SignedHeaders=")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .unwrap_or_default();
    let added = [
        "host",
        "x-amz-date",
        "x-amz-content-sha256",
        "x-amz-security-token",
    ];
    let signed: Vec<(&str, &str)> = names
        .split(';')
        .filter(|n| !added.contains(n))
        .map(|n| (n, header(n)))
        .collect();
    let pairs: Vec<(String, String)> = parts
        .uri
        .query()
        .unwrap_or_default()
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();
    let query: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let d = header("x-amz-date");
    let unix = humantime::parse_rfc3339(&format!(
        "{}-{}-{}T{}:{}:{}Z",
        &d[0..4],
        &d[4..6],
        &d[6..8],
        &d[9..11],
        &d[11..13],
        &d[13..15]
    ))
    .map_err(|e| e.to_string())?
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_secs();
    let payload = hex::encode(Sha256::digest(body));
    let want = desk_core::aws::sign(
        &desk_core::aws::Request {
            method: parts.method.as_str(),
            host: header("host"),
            path: &decode(parts.uri.path()),
            query: &query,
            headers: &signed,
            payload_sha256: &payload,
        },
        &desk_core::aws::Credentials {
            key_id: id.into(),
            secret: secret.into(),
            token: token.map(str::to_string),
        },
        region,
        service,
        unix,
    );
    let ok = want.last().map(|(_, v)| v.as_str()) == Some(auth)
        && header("x-amz-security-token") == token.unwrap_or_default()
        && (service != "s3" || header("x-amz-content-sha256") == payload);
    if ok {
        Ok(id)
    } else {
        Err(format!(
            "signature mismatch for {} {}",
            parts.method, parts.uri
        ))
    }
}

/// STS for the gateway's key, and path-style S3: the vault bucket for the reader role and the
/// owner's key, the artifacts bucket (the person records) for the gateway's own key.
async fn aws(objects: Objects, request: Request<hyper::body::Incoming>) -> Response<Full<Bytes>> {
    let (parts, body) = request.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    let signer = match signer(&parts, &body) {
        Ok(id) => id,
        Err(e) => {
            return reply(
                403,
                format!("<Error><Code>SignatureDoesNotMatch</Code><Message>{e}</Message></Error>"),
            )
        }
    };
    let path = decode(parts.uri.path());
    if parts.method == hyper::Method::POST && path == "/" {
        let form = String::from_utf8_lossy(&body);
        if signer != GATEWAY.0 || !form.contains("Action=AssumeRole") {
            return reply(403, "<Error><Code>AccessDenied</Code></Error>");
        }
        return reply(
            200,
            format!(
                "<AssumeRoleResponse><AssumeRoleResult><Credentials><AccessKeyId>{}</AccessKeyId>\
                 <SecretAccessKey>{}</SecretAccessKey><SessionToken>{}</SessionToken>\
                 <Expiration>2099-01-01T00:00:00Z</Expiration></Credentials></AssumeRoleResult>\
                 </AssumeRoleResponse>",
                ROLE.0,
                ROLE.1,
                ROLE.2.unwrap()
            ),
        );
    }
    let (bucket, key) = path[1..].split_once('/').unwrap_or((&path[1..], ""));
    let allowed = match bucket {
        "vault-bucket" => [ROLE.0, OWNER.0].contains(&signer),
        "artifacts" => signer == GATEWAY.0,
        _ => false,
    };
    if !allowed {
        return reply(403, "<Error><Code>AccessDenied</Code></Error>");
    }
    let full = format!("{bucket}/{key}");
    let mut objects = objects.lock().unwrap();
    match parts.method.as_str() {
        "GET" if key.is_empty() => {
            let query: BTreeMap<String, String> = parts
                .uri
                .query()
                .unwrap_or_default()
                .split('&')
                .filter_map(|p| p.split_once('='))
                .map(|(k, v)| (decode(k), decode(v)))
                .collect();
            let prefix = format!("{bucket}/{}", query["prefix"]);
            let keys: String = objects
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .map(|k| format!("<Contents><Key>{}</Key></Contents>", &k[bucket.len() + 1..]))
                .collect();
            reply(200, format!("<ListBucketResult>{keys}</ListBucketResult>"))
        }
        "GET" => match objects.get(&full) {
            Some(data) => reply(200, data.clone()),
            None => reply(404, "<Error><Code>NoSuchKey</Code></Error>"),
        },
        "PUT" => {
            let none_match = parts.headers.get("if-none-match").is_some();
            if none_match && objects.contains_key(&full) {
                return reply(412, "<Error><Code>PreconditionFailed</Code></Error>");
            }
            objects.insert(full, body.to_vec());
            reply(200, "")
        }
        "DELETE" => {
            objects.remove(&full);
            reply(204, "")
        }
        _ => reply(405, ""),
    }
}

async fn listen_aws(objects: Objects) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let objects = objects.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |request| {
                    let objects = objects.clone();
                    async move { Ok::<_, std::convert::Infallible>(aws(objects, request).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    url
}

/// A fake tailscaled: every caller is `login` on an untagged device.
async fn fake_tailscaled(socket: &Path, login: Arc<Mutex<String>>) {
    let listener = tokio::net::UnixListener::bind(socket).unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let login = login.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |request: Request<_>| {
                    let login = login.lock().unwrap().clone();
                    async move {
                        let path = request.uri().path().to_string();
                        let host = request.headers().get("host").cloned();
                        let ok = path == "/localapi/v0/whois"
                            && host.as_ref().map(|h| h.as_bytes())
                                == Some(b"local-tailscaled.sock");
                        let reply = if ok {
                            reply(
                                200,
                                json!({"Node": {}, "UserProfile": {"LoginName": login}})
                                    .to_string(),
                            )
                        } else {
                            reply(403, "invalid localapi request")
                        };
                        Ok::<_, std::convert::Infallible>(reply)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    });
}

/// `layr-desk serve` on 127.0.0.1, stopped when dropped.
struct Gateway(Child);

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn start_gateway(root: &Path, aws: &str, socket: &Path, recipient: &str) -> (Gateway, String) {
    let config = root.join("config.json");
    std::fs::write(
        &config,
        json!({"listen": "127.0.0.1",
               "projects": {"demo": {"bucket": "vault-bucket", "region": REGION,
                   "role": "arn:aws:iam::1:role/studio-vault-reader-demo", "recipient": recipient}},
               "people": {"bucket": "artifacts", "region": REGION}})
        .to_string(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_layr-desk"))
        .args(["serve", "--tls", "none", "--port", "0", "--config"])
        .arg(&config)
        .arg("--site")
        .arg(root.join("site"))
        .arg("--tailscale-socket")
        .arg(socket)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("AWS_ACCESS_KEY_ID", GATEWAY.0)
        .env("AWS_SECRET_ACCESS_KEY", GATEWAY.1)
        .env("AWS_ENDPOINT_URL_S3", aws)
        .env("AWS_ENDPOINT_URL_STS", aws)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let site = loop {
        let line = lines.next().expect("serve stopped").unwrap();
        if let Some(rest) = line.split("listening on ").nth(1) {
            break rest.split_whitespace().next().unwrap().to_string();
        }
    };
    // Keep reading, so that the gateway never blocks on a full pipe.
    std::thread::spawn(move || lines.for_each(|line| eprintln!("serve: {}", line.unwrap())));
    (Gateway(child), site)
}

/// A home with the AWS profile `owner` (keys) and the endpoints of the fake AWS.
struct Home {
    root: PathBuf,
    aws: String,
}

impl Home {
    fn new(root: PathBuf, aws: &str) -> Home {
        std::fs::create_dir_all(root.join(".aws")).unwrap();
        std::fs::create_dir_all(root.join(".config/studio-vault")).unwrap();
        std::fs::write(
            root.join(".aws/credentials"),
            format!(
                "[owner]\naws_access_key_id = {}\naws_secret_access_key = {}\n",
                OWNER.0, OWNER.1
            ),
        )
        .unwrap();
        std::fs::write(
            root.join(".aws/config"),
            format!("[profile owner]\nregion = {REGION}\n"),
        )
        .unwrap();
        Home {
            root,
            aws: aws.to_string(),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_layr-desk"))
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.root)
            .env("AWS_ENDPOINT_URL_S3", &self.aws)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn file(&self, name: &str, text: &str) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    }
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// The 24 numbered words that a command showed, in their order.
fn words(shown: &str) -> String {
    let mut numbered: Vec<(usize, String)> = Vec::new();
    for line in shown.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        for pair in parts.chunks(2) {
            if let [n, word] = pair {
                if let Some(n) = n.strip_suffix('.').and_then(|n| n.parse().ok()) {
                    numbered.push((n, word.to_string()));
                }
            }
        }
    }
    numbered.sort();
    assert_eq!(numbered.len(), 24, "{shown}");
    numbered
        .into_iter()
        .map(|(_, w)| w)
        .collect::<Vec<_>>()
        .join(" ")
}

fn line_after<'a>(text: &'a str, start: &str) -> &'a str {
    text.lines()
        .find_map(|l| l.strip_prefix(start))
        .unwrap_or_else(|| panic!("no {start:?} in {text}"))
}

/// The identity in a local slot, opened with the words.
fn opened(slot: &Path, words: &str) -> String {
    let paper = keys::identity_from_paper(words).unwrap();
    keys::open_slot(&std::fs::read(slot).unwrap(), &paper).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_key_gets_vaults_by_grant_and_checks_their_pins() {
    let root = std::env::temp_dir().join(format!("desk-person-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let objects = Objects::default();
    let aws = listen_aws(objects.clone()).await;
    let login = Arc::new(Mutex::new(ANN.to_string()));
    let socket = root.join("ts.sock");
    fake_tailscaled(&socket, login.clone()).await;
    let as_login = |who: &str| *login.lock().unwrap() = who.to_string();

    // Ann owns the vault "demo": its keys, and what `deploy` made of it.
    let ann = Home::new(root.join("ann"), &aws);
    let vault_words = words(&ok(&ann.run(&["keys", "--project", "demo"])));
    let demo = ann.root.join(".config/studio-vault/demo");
    let vault_recipient = std::fs::read_to_string(demo.join("recipient.txt")).unwrap();
    let vault_recipient = vault_recipient.trim().to_string();
    let vault_identity = opened(&demo.join("paper.age"), &vault_words);
    let writer = B64.encode([9u8; 32]);
    objects.lock().unwrap().insert(
        "vault-bucket/demo/vault.json".into(),
        json!({"format": 1, "recipient": vault_recipient, "created": 1, "writer": writer,
               "signedFrom": 3})
        .to_string()
        .into_bytes(),
    );
    let (_gateway, site) = start_gateway(&root, &aws, &socket, &vault_recipient);
    std::fs::write(
        demo.join("cloud.json"),
        json!({"profile": "owner", "region": REGION, "bucket": "vault-bucket",
               "site": format!("{site}/demo/"), "stack": "studio-vault-demo"})
        .to_string(),
    )
    .unwrap();
    // Before any grant, the vault shows to every tailnet member.
    // The gateway asks tailscaled once per connection; a new login needs a new connection.
    let http = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();
    let projects = |who: &str| {
        as_login(who);
        let request = http.get(format!("{site}/api/projects")).send();
        async { request.await.unwrap().json::<Value>().await.unwrap()["projects"].clone() }
    };
    assert_eq!(projects("dave@example.com").await, json!(["demo"]));

    // 1. Ann makes her person key and registers it; the words open its paper slot.
    as_login(ANN);
    let shown = ok(&ann.run(&["person", "keys", "--site", &site]));
    let ann_words = words(&shown);
    let ann_recipient = line_after(&shown, "recipient: ").to_string();
    assert!(
        shown.contains(&format!("registered at {site} as {ANN}")),
        "{shown}"
    );
    let person_folder = ann.root.join(".config/studio-vault/.person");
    let ann_person = opened(&person_folder.join("paper.age"), &ann_words);
    assert_eq!(keys::recipient_of(&ann_person).unwrap(), ann_recipient);
    let record: Value = serde_json::from_slice(
        &objects.lock().unwrap()["artifacts/people/ann@example.com/person.json"],
    )
    .unwrap();
    assert_eq!(record["recipient"], ann_recipient);
    let again = ok(&ann.run(&["person", "keys", "--site", &site]));
    assert!(again.contains("already registered"), "{again}");
    assert!(
        !ann.run(&["person", "keys"]).status.success(),
        "one key only"
    );
    assert_eq!(ok(&ann.run(&["person", "show"])).trim(), ann_recipient);

    // 2. Bob, in a home of his own.
    let bob = Home::new(root.join("bob"), &aws);
    as_login(BOB);
    let shown = ok(&bob.run(&["person", "keys", "--site", &site]));
    let bob_recipient = line_after(&shown, "recipient: ").to_string();
    let bob_person = opened(
        &bob.root.join(".config/studio-vault/.person/paper.age"),
        &words(&shown),
    );

    // 3. Ann grants herself the vault with the vault's paper key.
    as_login(ANN);
    let vault_paper = ann.file("vault-words.txt", &vault_words);
    let granted: Value = serde_json::from_str(&ok(&ann.run(&[
        "grant",
        "--project",
        "demo",
        "--person",
        ANN,
        "--recipient",
        &ann_recipient,
        "--paper-file",
        &vault_paper,
    ])))
    .unwrap();
    assert_eq!(granted["slot"], "demo/slots/person-ann@example.com.age");
    let slot =
        objects.lock().unwrap()["vault-bucket/demo/slots/person-ann@example.com.age"].clone();
    assert_eq!(keys::open_slot(&slot, &ann_person).unwrap(), vault_identity);
    let pin = Pin {
        project: "demo".into(),
        recipient: vault_recipient.clone(),
        writer: Some(writer.clone()),
        signed_from: Some(3),
    };
    let inbox = |who: &str| -> Vec<Value> {
        let prefix = format!("artifacts/people/{who}/inbox/");
        let objects = objects.lock().unwrap();
        objects
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| serde_json::from_slice(v).unwrap())
            .collect()
    };
    let invites = inbox(ANN);
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0]["from"], ANN);
    let sealed = B64.decode(invites[0]["data"].as_str().unwrap()).unwrap();
    assert_eq!(person::open_invite(&ann_person, &sealed).unwrap(), pin);

    // 4. Ann grants Bob with her person key: its words and her person slot open the vault.
    let ann_paper = ann.file("person-words.txt", &ann_words);
    ok(&ann.run(&[
        "grant",
        "--project",
        "demo",
        "--person",
        BOB,
        "--recipient",
        &bob_recipient,
        "--paper-file",
        &ann_paper,
    ]));
    let slot =
        objects.lock().unwrap()["vault-bucket/demo/slots/person-bob@example.com.age"].clone();
    assert_eq!(keys::open_slot(&slot, &bob_person).unwrap(), vault_identity);
    let invites = inbox(BOB);
    assert_eq!((invites.len(), invites[0]["from"].clone()), (1, json!(ANN)));
    // Words that open neither, and a person without a key at the gateway.
    let wrong = ann.file(
        "wrong.txt",
        &keys::words_of_identity(&keys::generate_identity()).unwrap(),
    );
    let refused = ann.run(&[
        "grant",
        "--project",
        "demo",
        "--person",
        BOB,
        "--recipient",
        &bob_recipient,
        "--paper-file",
        &wrong,
    ]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("open neither"));
    let carol = ann.run(&[
        "grant",
        "--project",
        "demo",
        "--person",
        "carol@example.com",
        "--recipient",
        &bob_recipient,
        "--paper-file",
        &vault_paper,
    ]);
    assert!(!carol.status.success());
    assert!(String::from_utf8_lossy(&carol.stderr).contains("did not reach carol@example.com"));

    // 5. Now the vault shows only to the people with a person slot.
    assert_eq!(projects(ANN).await, json!(["demo"]));
    assert_eq!(projects(BOB).await, json!(["demo"]));
    assert_eq!(projects("dave@example.com").await, json!([]));

    // 6. Ann accepts the invite as the site does: an index with the pin, and her person token.
    as_login(ANN);
    let challenge: Value = http
        .get(format!("{site}/api/person/challenge"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sealed = B64
        .decode(challenge["challenge"].as_str().unwrap())
        .unwrap();
    let token = String::from_utf8(keys::decrypt(&ann_person, &sealed).unwrap()).unwrap();
    let index = Index { vaults: vec![pin] };
    let data = person::seal_index(&ann_person, &index).unwrap();
    let saved = http
        .put(format!("{site}/api/person/index"))
        .bearer_auth(&token)
        .json(&json!({"data": B64.encode(&data), "previous": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(saved.status(), 200);
    let checked = ok(&ann.run(&[
        "person",
        "vaults",
        "--site",
        &site,
        "--paper-file",
        &ann_paper,
    ]));
    assert!(checked.contains("demo ok"), "{checked}");
    assert!(
        checked.contains(&format!("1 invites wait on the site (from {ANN})")),
        "{checked}"
    );

    // 7. Someone changes the vault.json in the bucket: the pin refuses it.
    let changed = VaultInfo {
        recipient: vault_recipient.clone(),
        writer: Some(B64.encode([8u8; 32])),
        signed_from: Some(3),
    };
    objects.lock().unwrap().insert(
        "vault-bucket/demo/vault.json".into(),
        serde_json::to_vec(&changed).unwrap(),
    );
    let refused = ann.run(&[
        "person",
        "vaults",
        "--site",
        &site,
        "--paper-file",
        &ann_paper,
    ]);
    assert!(!refused.status.success());
    let out = String::from_utf8_lossy(&refused.stdout);
    assert!(
        out.contains("demo refused") && out.contains("(writer key)"),
        "{out}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
