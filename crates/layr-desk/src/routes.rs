//! The gateway's HTTP side (docs/design.md, "Gateway"): the site, the API of every vault in the
//! configuration, and the person records (docs/design.md, "Person key"). `serve` runs it with TLS
//! on the tailnet address only.

use crate::people::{People, INDEX_LIMIT};
use crate::vault::{fail, Body, Bucket, Vault, VaultError};
use anyhow::Result;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::{Method, Request, Response};
use percent_encoding::percent_decode;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

const SECURITY: [(&str, &str); 6] = [
    (
        "content-security-policy",
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
         connect-src 'self'; img-src 'self' blob: data:; media-src 'self' blob:; frame-src blob:; \
         object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
    ),
    ("strict-transport-security", "max-age=63072000"),
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
    ("cross-origin-opener-policy", "same-origin"),
];
const TYPES: [(&str, &str); 8] = [
    ("html", "text/html; charset=utf-8"),
    ("js", "text/javascript"),
    ("css", "text/css"),
    ("wasm", "application/wasm"),
    ("json", "application/json"),
    ("svg", "image/svg+xml"),
    ("png", "image/png"),
    ("ico", "image/x-icon"),
];
const BODY_LIMIT: usize = 16 * 1024;
/// A request with an index: its base64 and a little JSON.
const INDEX_BODY_LIMIT: usize = INDEX_LIMIT * 4 / 3 + 1024;

/// `/etc/vault-gateway/config.json`, which the gateway host writes.
#[derive(Deserialize, Default, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub listen: Option<String>,
    /// Each vault's `{bucket, region, role, recipient}`, read when the vault is used.
    #[serde(default)]
    pub projects: BTreeMap<String, Value>,
    /// Where the person records are; a gateway without them has no person keys.
    #[serde(default)]
    pub people: Option<PeopleConfig>,
}

/// A bucket of the gateway's own account and a prefix in it.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PeopleConfig {
    pub bucket: String,
    pub region: String,
    #[serde(default = "people_prefix")]
    pub prefix: String,
}

fn people_prefix() -> String {
    "people/".into()
}

pub type ConfigFn = Box<dyn Fn() -> Result<Arc<Config>> + Send + Sync>;
pub type VaultFn<B> = Box<dyn Fn(&str, &Config) -> Result<Option<Arc<Vault<B>>>> + Send + Sync>;
pub type PeopleFn<B> = Box<dyn Fn(&Config) -> Result<Option<Arc<People<B>>>> + Send + Sync>;
/// The tailnet login of a peer address (`whois::login`).
pub type WhoisFn =
    Box<dyn Fn(SocketAddr) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

pub struct Gateway<B> {
    pub config: ConfigFn,
    pub vault_for: VaultFn<B>,
    pub people_for: PeopleFn<B>,
    pub whois: WhoisFn,
    /// The folder of the built site.
    pub site: PathBuf,
}

/// One connection: the peer address, and the login of its device once a route needs it.
pub struct Caller {
    address: Option<SocketAddr>,
    login: tokio::sync::OnceCell<Option<String>>,
}

impl Caller {
    pub fn new(address: Option<SocketAddr>) -> Self {
        Caller {
            address,
            login: tokio::sync::OnceCell::new(),
        }
    }
}

fn full(data: impl Into<Bytes>) -> Body {
    Full::new(data.into())
        .map_err(|e| match e {})
        .boxed_unsync()
}

fn send(status: u16, body: Value) -> Response<Body> {
    let mut response = Response::builder().status(status);
    for (name, value) in SECURITY {
        response = response.header(name, value);
    }
    response
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(full(body.to_string()))
        .expect("valid headers")
}

fn error(status: u16, message: &str) -> Response<Body> {
    send(status, json!({ "error": message }))
}

/// The path as a URL parser gives it: `\` as `/`, and `.` and `..` segments (also as `%2e`)
/// resolved.
fn normalize(raw: &str) -> String {
    let raw = raw.replace('\\', "/");
    let segments: Vec<&str> = raw.split('/').skip(1).collect();
    let mut out: Vec<&str> = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match segment.to_ascii_lowercase().replace("%2e", ".").as_str() {
            ".." => {
                out.pop();
                if last {
                    out.push("");
                }
            }
            "." if last => out.push(""),
            "." => {}
            _ => out.push(segment),
        }
    }
    format!("/{}", out.join("/"))
}

/// A path segment decoded as `decodeURIComponent` does: a broken escape or UTF-8 is an error.
fn decode(segment: &str) -> Option<String> {
    let b = segment.as_bytes();
    for (i, c) in b.iter().enumerate() {
        if *c == b'%'
            && !(b.len() > i + 2 && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit())
        {
            return None;
        }
    }
    percent_decode(b).decode_utf8().ok().map(|s| s.into_owned())
}

/// The extension of the last part of a path, as Node's `extname`: none for `.name`.
fn has_extension(path: &str) -> bool {
    let base = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    match base.rfind('.') {
        Some(at) => !base[..at].bytes().all(|c| c == b'.'),
        None => false,
    }
}

impl<B: Bucket> Gateway<B> {
    pub async fn handle(
        self: Arc<Self>,
        request: Request<Incoming>,
        caller: Arc<Caller>,
    ) -> Result<Response<Body>, Infallible> {
        Ok(match self.route(request, &caller).await {
            Ok(response) => response,
            Err(e) => match e.downcast_ref::<VaultError>() {
                Some(v) => error(v.status, v.message),
                None => {
                    eprintln!("layr-desk serve: {e:#}");
                    error(500, "internal error")
                }
            },
        })
    }

    /// The caller's tailnet login: asked once per connection.
    async fn login(&self, caller: &Caller) -> Option<String> {
        caller
            .login
            .get_or_init(|| async {
                match caller.address {
                    Some(address) => (self.whois)(address).await,
                    None => None,
                }
            })
            .await
            .clone()
    }

    async fn route(&self, request: Request<Incoming>, caller: &Caller) -> Result<Response<Body>> {
        let config = (self.config)()?;
        // A name that resolves to the gateway is not enough: the host must be the configured one.
        if let Some(host) = config.host.as_deref().filter(|h| !h.is_empty()) {
            let given = request
                .headers()
                .get("host")
                .and_then(|h| h.to_str().ok())
                .unwrap_or_default();
            let given = match given.rsplit_once(':') {
                Some((name, port))
                    if !port.is_empty() && port.bytes().all(|c| c.is_ascii_digit()) =>
                {
                    name
                }
                _ => given,
            };
            if given != host {
                return Ok(error(421, "wrong host name"));
            }
        }
        let pathname = normalize(request.uri().path());
        let mut parts = Vec::new();
        for segment in pathname.split('/').filter(|s| !s.is_empty()) {
            match decode(segment) {
                Some(part) => parts.push(part),
                None => return Ok(error(400, "the path is not valid")),
            }
        }
        if parts.first().map(String::as_str) == Some("api") {
            return self.api(request, &config, caller, &parts[1..]).await;
        }
        let method = request.method();
        if method != Method::GET && method != Method::HEAD {
            return Ok(error(405, "the site is read-only"));
        }
        let head = method == Method::HEAD;
        // Each vault is a page of the site: /<project>/ shows the same index.html.
        if !has_extension(&pathname) {
            return self.file(head, &["index.html".to_string()], false).await;
        }
        if pathname.ends_with('/') {
            return Ok(error(404, "no such file"));
        }
        // Vite names the files below assets/ by their content.
        self.file(head, &parts, pathname.starts_with("/assets/"))
            .await
    }

    async fn file(&self, head: bool, parts: &[String], immutable: bool) -> Result<Response<Body>> {
        let unsafe_part = |p: &String| p == "." || p == ".." || p.contains(['/', '\\', '\0']);
        if parts.iter().any(unsafe_part) {
            return Ok(error(404, "no such file"));
        }
        let path: PathBuf = parts.iter().fold(self.site.clone(), |p, part| p.join(part));
        let length = match tokio::fs::metadata(&path).await {
            Ok(meta) if meta.is_file() => meta.len(),
            _ => return Ok(error(404, "no such file")),
        };
        let (data, length) = if head {
            (Bytes::new(), length)
        } else {
            let data = Bytes::from(tokio::fs::read(&path).await?);
            let length = data.len() as u64;
            (data, length)
        };
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let kind = TYPES
            .iter()
            .find(|(e, _)| *e == extension)
            .map_or("application/octet-stream", |(_, t)| t);
        let mut response = Response::builder().status(200);
        for (name, value) in SECURITY {
            response = response.header(name, value);
        }
        Ok(response
            .header("content-type", kind)
            .header("content-length", length)
            .header(
                "cache-control",
                if immutable {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-cache"
                },
            )
            .body(full(data))?)
    }

    async fn api(
        &self,
        request: Request<Incoming>,
        config: &Config,
        caller: &Caller,
        parts: &[String],
    ) -> Result<Response<Body>> {
        let method = request.method().clone();
        match parts.first().map(String::as_str) {
            Some("projects") if parts.len() == 1 && method == Method::GET => {
                return self.projects(config, caller).await;
            }
            Some("person") => return self.person(request, config, caller, &parts[1..]).await,
            Some("people") => return self.inbox(request, config, caller, &parts[1..]).await,
            _ => {}
        }
        let project = parts.get(1).map(String::as_str).unwrap_or_default();
        if parts.first().map(String::as_str) != Some("p") || !crate::vault::project_name(project) {
            return Ok(error(404, "no such route"));
        }
        let Some(vault) = (self.vault_for)(project, config)? else {
            return Ok(error(404, "no such vault"));
        };
        if parts.len() > 4 {
            return Ok(error(404, "no such route"));
        }
        let kind = parts.get(2).map(String::as_str);
        let name = parts.get(3).map(String::as_str);
        match (&method, kind, name) {
            (&Method::GET, Some("slots"), None) => {
                let login = self.login(caller).await;
                let Some(slots) = vault.slots(login.as_deref()).await? else {
                    return Ok(error(404, "no such vault"));
                };
                let slots: Vec<Value> = slots
                    .into_iter()
                    .map(|(name, data)| json!({ "name": name, "data": data }))
                    .collect();
                return Ok(send(200, json!({ "slots": slots })));
            }
            (&Method::GET, Some("challenge" | "vault"), None) => {
                let login = self.login(caller).await;
                if !vault.visible(login.as_deref()).await? {
                    return Ok(error(404, "no such vault"));
                }
                if kind == Some("challenge") {
                    return Ok(send(200, json!({ "challenge": vault.challenge()? })));
                }
                return Ok(send(200, serde_json::to_value(vault.info().await?)?));
            }
            _ => {}
        }

        let auth = request
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        match auth.strip_prefix("Bearer ") {
            Some(token) if vault.valid(token) => {}
            _ => return Ok(error(401, "the token is missing, wrong or expired")),
        }

        match (&method, kind, name) {
            (&Method::GET, Some("states"), None) => {
                Ok(send(200, json!({ "states": vault.states().await? })))
            }
            (&Method::GET, Some("states"), Some(name)) => {
                let object = vault.object(name).await?;
                let mut response = Response::builder().status(200);
                for (name, value) in SECURITY {
                    response = response.header(name, value);
                }
                response = response
                    .header("content-type", "application/octet-stream")
                    .header("cache-control", "private, no-store");
                if let Some(length) = object.length {
                    response = response.header("content-length", length);
                }
                Ok(response.body(object.body)?)
            }
            (&Method::PUT, Some("slots"), Some(name)) => {
                let body = read_json(request, BODY_LIMIT).await?;
                let data = body.get("data").and_then(Value::as_str).unwrap_or_default();
                vault.add_slot(name, data).await?;
                Ok(send(200, json!({ "added": name })))
            }
            (&Method::DELETE, Some("slots"), Some(name)) => {
                vault.remove_slot(name).await?;
                Ok(send(200, json!({ "removed": name })))
            }
            _ => Ok(error(404, "no such route")),
        }
    }

    /// The vaults that show to the caller (`Vault::visible`). A vault whose slots cannot be
    /// listed is left out, and the log says why.
    async fn projects(&self, config: &Config, caller: &Caller) -> Result<Response<Body>> {
        let login = self.login(caller).await;
        let mut checks = tokio::task::JoinSet::new();
        for name in config.projects.keys() {
            match (self.vault_for)(name, config) {
                Ok(Some(vault)) => {
                    let (name, login) = (name.clone(), login.clone());
                    checks.spawn(async move { (name, vault.visible(login.as_deref()).await) });
                }
                Ok(None) => {}
                Err(e) => eprintln!("layr-desk serve: the vault {name}: {e:#}"),
            }
        }
        let mut names = Vec::new();
        while let Some(done) = checks.join_next().await {
            match done? {
                (name, Ok(true)) => names.push(name),
                (_, Ok(false)) => {}
                (name, Err(e)) => {
                    eprintln!("layr-desk serve: the vault {name} is not in the list: {e:#}")
                }
            }
        }
        names.sort();
        Ok(send(200, json!({ "projects": names })))
    }

    /// `/api/person/…`: the caller's own person records.
    async fn person(
        &self,
        request: Request<Incoming>,
        config: &Config,
        caller: &Caller,
        parts: &[String],
    ) -> Result<Response<Body>> {
        let Some(people) = (self.people_for)(config)? else {
            return Ok(error(404, "this gateway keeps no person keys"));
        };
        let Some(login) = self.login(caller).await else {
            return Ok(error(
                403,
                "the gateway cannot tell who you are: use a device of a tailnet person, not a \
                 tagged one",
            ));
        };
        if parts.len() > 2 {
            return Ok(error(404, "no such route"));
        }
        let method = request.method().clone();
        let kind = parts.first().map(String::as_str);
        let name = parts.get(1).map(String::as_str);
        match (&method, kind, name) {
            (&Method::GET, None, None) => return Ok(send(200, people.view(&login).await?)),
            (&Method::PUT, None, None) => {
                let body = read_json(request, BODY_LIMIT).await?;
                let text = |field: &str| body.get(field).and_then(Value::as_str).unwrap_or("");
                let new = people
                    .register(&login, text("recipient"), text("paper"))
                    .await?;
                return Ok(send(200, json!({ "login": login, "new": new })));
            }
            (&Method::GET, Some("challenge"), None) => {
                return Ok(send(
                    200,
                    json!({ "challenge": people.challenge(&login).await? }),
                ));
            }
            _ => {}
        }

        let auth = request
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        match auth.strip_prefix("Bearer ") {
            Some(token) if people.valid(&login, token) => {}
            _ => return Ok(error(401, "the token is missing, wrong or expired")),
        }

        match (&method, kind, name) {
            (&Method::PUT, Some("index"), None) => {
                let body = read_json(request, INDEX_BODY_LIMIT).await?;
                let data = body.get("data").and_then(Value::as_str).unwrap_or_default();
                let previous = body.get("previous").and_then(Value::as_str);
                people.put_index(&login, data, previous).await?;
                Ok(send(200, json!({ "saved": "index" })))
            }
            (&Method::PUT, Some("slots"), Some(name)) => {
                let body = read_json(request, BODY_LIMIT).await?;
                let data = body.get("data").and_then(Value::as_str).unwrap_or_default();
                people.add_slot(&login, name, data).await?;
                Ok(send(200, json!({ "added": name })))
            }
            (&Method::DELETE, Some("slots"), Some(name)) => {
                people.remove_slot(&login, name).await?;
                Ok(send(200, json!({ "removed": name })))
            }
            (&Method::DELETE, Some("inbox"), Some(id)) => {
                people.remove_invite(&login, id).await?;
                Ok(send(200, json!({ "removed": id })))
            }
            _ => Ok(error(404, "no such route")),
        }
    }

    /// `POST /api/people/<login>/inbox`: any tailnet person drops an invite for another.
    async fn inbox(
        &self,
        request: Request<Incoming>,
        config: &Config,
        caller: &Caller,
        parts: &[String],
    ) -> Result<Response<Body>> {
        let to = parts.first().map(String::as_str).unwrap_or_default();
        if parts.len() != 2
            || parts[1] != "inbox"
            || request.method() != Method::POST
            || !desk_core::person::login_name(to)
        {
            return Ok(error(404, "no such route"));
        }
        let Some(people) = (self.people_for)(config)? else {
            return Ok(error(404, "this gateway keeps no person keys"));
        };
        let Some(from) = self.login(caller).await else {
            return Ok(error(403, "the gateway cannot tell who you are"));
        };
        let body = read_json(request, BODY_LIMIT).await?;
        let data = body.get("data").and_then(Value::as_str).unwrap_or_default();
        let id = people.drop_invite(to, &from, data).await?;
        Ok(send(200, json!({ "id": id })))
    }
}

async fn read_json(request: Request<Incoming>, limit: usize) -> Result<Value> {
    let bytes = match Limited::new(request.into_body(), limit).collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) if e.is::<http_body_util::LengthLimitError>() => {
            return fail(413, "the body is too large")
        }
        Err(e) => return Err(anyhow::anyhow!(e)),
    };
    if bytes.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(value),
        Err(_) => fail(400, "the body is not JSON"),
    }
}
