//! Gateway routes and grants. Storage only receives validated relative object names.

use super::{
    auth,
    config::{ConfigFile, Project},
};
use anyhow::{bail, Context, Result};
use base64::Engine;
use layr_store::http;
use serde_json::json;
use std::sync::Arc;

pub const OBJECT_LIMIT: usize = 32 * 1024 * 1024;
pub const SLOT_LIMIT: usize = 16 * 1024;
pub const JSON_LIMIT: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Reader,
    Writer,
    Gc,
}

pub struct Object {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_range: Option<String>,
}

pub struct Entry {
    pub key: String,
    pub modified: i64,
}

pub trait Storage: Send + Sync {
    fn people(&self, _: &super::config::People) -> Result<Box<dyn super::person_support::Bucket>> {
        bail!("people storage is not configured")
    }
    fn get(
        &self,
        project: &Project,
        key: &str,
        range: Option<http::Range>,
        limit: usize,
    ) -> Result<Option<Object>>;
    fn list(&self, project: &Project, prefix: &str, role: Role) -> Result<Vec<Entry>>;
    fn put(&self, project: &Project, key: &str, body: Vec<u8>) -> Result<bool>;
    fn delete(&self, project: &Project, key: &str) -> Result<()>;
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn bytes(status: u16, body: Vec<u8>, media: &str) -> Self {
        Self {
            status,
            body,
            headers: vec![
                ("Content-Type".into(), media.into()),
                ("Cache-Control".into(), "no-store".into()),
                (
                    "Content-Security-Policy".into(),
                    "default-src 'none'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'".into(),
                ),
                ("Strict-Transport-Security".into(), "max-age=63072000".into()),
                ("X-Content-Type-Options".into(), "nosniff".into()),
                ("X-Frame-Options".into(), "DENY".into()),
                ("Referrer-Policy".into(), "no-referrer".into()),
                ("Cross-Origin-Opener-Policy".into(), "same-origin".into()),
            ],
        }
    }
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self::bytes(status, value.to_string().into_bytes(), "application/json")
    }
    pub fn error(status: u16, message: &str) -> Self {
        Self::json(status, json!({"error":message}))
    }
}

pub struct Gateway {
    pub config: Arc<ConfigFile>,
    pub proof: auth::Proof,
    pub storage: Arc<dyn Storage>,
    pub listen: std::net::SocketAddr,
    pub origin: Option<super::origin::Origin>,
    pub person_secret: Arc<[u8; 32]>,
    pub person_writing: Arc<std::sync::Mutex<()>>,
    pub public_limit: super::public::Limiter,
    pub sessions: super::session::Sessions,
    pub bootstrap_limit: super::public::Limiter,
}

pub(super) fn decode(raw: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut input = raw.as_bytes().iter().copied();
    while let Some(b) = input.next() {
        bytes.push(if b == b'%' {
            let a = (input.next()? as char).to_digit(16)?;
            let b = (input.next()? as char).to_digit(16)?;
            (a * 16 + b) as u8
        } else {
            b
        });
    }
    String::from_utf8(bytes).ok()
}

pub(super) fn safe_part(part: &str) -> bool {
    !part.is_empty()
        && !matches!(part, "." | "..")
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn store_object(name: &str) -> bool {
    matches!(name.split('/').next(), Some("records" | "states" | "slots"))
        && name.contains('/')
        && name
            .split('/')
            .all(|part| safe_part(part) || person_slot_part(part))
}

fn machine_slot(name: &str) -> bool {
    name.strip_prefix("machine-")
        .and_then(|name| name.strip_suffix(".age"))
        .is_some_and(layr_store::format::is_uuid)
}

fn overlay_key(name: &str) -> bool {
    let parts: Vec<_> = name.split('/').collect();
    parts.len() == 4
        && parts[0] == "overlays"
        && layr_store::format::is_uuid(parts[1])
        && parts[2] == "keys"
        && safe_part(parts[3])
        && parts[3].ends_with(".json")
}

fn hidden_overlay_key(name: &str) -> bool {
    // layr b1a23c1: store_remote::overlays::Published::object. All three IDs
    // are opaque UUIDs; the encrypted manifest never puts an overlay name here.
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        let id = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
        regex::Regex::new(&format!(r"\Aoverlays/{id}/keys/{id}-{id}\.age\z")).unwrap()
    });
    PATTERN.is_match(name)
}

fn overlay_keys_prefix(prefix: &str) -> bool {
    let parts: Vec<_> = prefix.split('/').collect();
    parts.len() == 4
        && parts[0] == "overlays"
        && layr_store::format::is_uuid(parts[1])
        && parts[2] == "keys"
        && parts[3].is_empty()
}

fn index_object(name: &str) -> bool {
    if layr_store::index::parse_heads_name(name).is_some_and(|(_, seq)| seq > 0) {
        return true;
    }
    let parts: Vec<_> = name.split('/').collect();
    matches!(parts.as_slice(), ["index", "paths" | "graph", file]
        if file.strip_suffix(".age").is_some_and(layr_store::format::is_uuid))
}

fn index_prefix(name: &str) -> bool {
    let parts: Vec<_> = name.strip_suffix('/').unwrap_or(name).split('/').collect();
    matches!(
        parts.as_slice(),
        ["index"] | ["index", "heads" | "paths" | "graph"]
    ) || matches!(parts.as_slice(), ["index", "heads", id] if layr_store::format::is_uuid(id))
        || index_object(name)
}

fn overlay_epoch(name: &str) -> Option<&str> {
    let mut parts = name.splitn(5, '/');
    if parts.next()? != "overlays"
        || !layr_store::format::is_uuid(parts.next()?)
        || parts.next()? != "epochs"
        || !layr_store::format::is_uuid(parts.next()?)
    {
        return None;
    }
    parts.next()
}

pub fn object_name(name: &str) -> bool {
    name.len() <= 1024
        && (matches!(name, "store.json" | "public.json")
            || store_object(name)
            || index_object(name)
            || super::epochs::discovery(name)
            || name
                .strip_prefix("history/")
                .and_then(|s| s.strip_suffix(".age"))
                .is_some_and(|e| e == "root" || layr_store::format::is_uuid(e))
            || super::requests::path(name).is_some()
            || overlay_key(name)
            || hidden_overlay_key(name)
            || overlay_epoch(name).is_some_and(store_object))
}

pub fn gc_name(name: &str) -> bool {
    object_name(name)
        && name.starts_with("states/")
        && name.split('/').count() == 3
        && name.ends_with(".data.age")
}

fn store_prefix(prefix: &str) -> bool {
    matches!(
        prefix.split('/').next(),
        Some("records" | "states" | "slots")
    ) && prefix
        .strip_suffix('/')
        .unwrap_or(prefix)
        .split('/')
        .all(|part| safe_part(part) || person_slot_part(part))
}

fn list_prefix(prefix: &str) -> bool {
    if prefix.len() > 1024 {
        return false;
    }
    if prefix.is_empty()
        || store_prefix(prefix)
        || index_prefix(prefix)
        || prefix == "keys/"
        || prefix == "history/"
        || prefix == "files/"
        || matches!(prefix, "claims/" | "claims")
        || matches!(
            prefix,
            "requests" | "requests/" | "requests/claims" | "requests/claims/"
        )
        || super::requests::path(prefix).is_some()
    {
        return true;
    }
    let parts: Vec<_> = prefix
        .strip_suffix('/')
        .unwrap_or(prefix)
        .split('/')
        .collect();
    if parts.first() != Some(&"overlays") {
        return false;
    }
    match parts.as_slice() {
        ["overlays"] => true,
        ["overlays", id] => layr_store::format::is_uuid(id),
        ["overlays", id, "keys" | "epochs"] => layr_store::format::is_uuid(id),
        ["overlays", id, "keys", file] => layr_store::format::is_uuid(id) && safe_part(file),
        ["overlays", id, "epochs", epoch] => {
            layr_store::format::is_uuid(id) && layr_store::format::is_uuid(epoch)
        }
        _ => overlay_epoch(prefix).is_some_and(store_prefix),
    }
}

impl Gateway {
    pub fn client_address(
        &self,
        headers: &std::collections::BTreeMap<String, String>,
        peer: std::net::IpAddr,
    ) -> std::result::Result<std::net::IpAddr, Reply> {
        match &self.origin {
            Some(origin) => origin.client(headers),
            None => Ok(peer),
        }
    }

    #[cfg(test)]
    pub fn handle(&self, request: http::Request, login: Option<&str>) -> Reply {
        self.handle_from(request, login, std::net::Ipv4Addr::LOCALHOST.into())
    }

    pub fn handle_from(
        &self,
        request: http::Request,
        login: Option<&str>,
        peer: std::net::IpAddr,
    ) -> Reply {
        match self.route(request, login, peer) {
            Ok(reply) => reply,
            Err(error) => {
                if let Some(error) = error.downcast_ref::<http::Error>() {
                    return Reply::error(error.status, error.message);
                }
                eprintln!("layr-gateway: request failed: {error:#}");
                Reply::error(502, "storage request failed")
            }
        }
    }

    fn route(
        &self,
        request: http::Request,
        _login: Option<&str>,
        peer: std::net::IpAddr,
    ) -> Result<Reply> {
        let peer = match self.client_address(&request.headers, peer) {
            Ok(peer) => peer,
            Err(reply) => return Ok(reply),
        };
        let authenticated = self.sessions.login(bearer(&request), auth::now());
        let login = authenticated.as_deref();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let config = self.config.get()?;
        if !self.public_limit.allow(peer, std::time::Instant::now()) {
            let mut reply = Reply::error(429, "request rate exceeded");
            reply.headers.push(("Retry-After".into(), "60".into()));
            return Ok(reply);
        }
        if config.listen != self.listen {
            return Ok(Reply::error(
                503,
                "restart the gateway after a listen change",
            ));
        }
        // The edge replaces Host with the origin name. Browser Origin must still
        // match the explicitly configured public name, after edge authentication.
        let public_host = self
            .origin
            .as_ref()
            .and_then(|_| config.extra.get("publicHost"))
            .and_then(|v| v.as_str());
        let host = request.header("host").unwrap_or("");
        let given = host
            .split_once(':')
            .map(|(name, port)| (name, port.parse::<u16>().ok()));
        let host_ok = match given {
            Some((name, Some(port))) => {
                name.eq_ignore_ascii_case(&config.host) && port == self.listen.port()
            }
            Some(_) => false,
            None => host.eq_ignore_ascii_case(&config.host),
        };
        if !host_ok {
            return Ok(Reply::error(421, "wrong host name"));
        }
        let (path, query) = request
            .target
            .split_once('?')
            .unwrap_or((&request.target, ""));
        let parts = path
            .strip_prefix('/')
            .unwrap_or("")
            .split('/')
            .map(decode)
            .collect::<Option<Vec<_>>>();
        let Some(mut parts) = parts else {
            return Ok(Reply::error(400, "invalid path encoding"));
        };
        if parts.first().is_some_and(|p| p == "p") {
            return self.public_read(&request, &config, &parts, query, peer);
        }
        if parts.first().is_some_and(|p| p == "api") {
            if request.method != "GET" && request.method != "HEAD" {
                if let Some(reply) = other_site(&request, true, public_host) {
                    return Ok(reply);
                }
            }
            if let Some(reply) = self.session_api(&request, &config, &parts[1..], query, peer)? {
                return Ok(reply);
            }
            if !query.is_empty() {
                return Ok(Reply::error(400, "unexpected query"));
            }
            return self.api(&request, &config, login, &parts[1..]);
        }
        if parts.first().is_some_and(|p| p != "v1") {
            if let Some(root) = config.extra.get("siteDir").and_then(|v| v.as_str()) {
                return Ok(super::site::serve(
                    std::path::Path::new(root),
                    &request.method,
                    &parts,
                ));
            }
        }
        let mut scoped_project = None;
        let mut epoch_chain = None;
        let mut namespace = String::from("root");
        let mut scoped_query = None;
        let mut scoped_read = false;
        if parts.len() >= 3 && parts[0] == "v1" && parts[1] == "p" {
            let Some(project) = config.projects.get(&parts[2]) else {
                return Ok(Reply::error(404, "no such project"));
            };
            let token = bearer(&request);
            let at = auth::now();
            let grant = auth::grant(token, project, false, at);
            let gc_grant = auth::grant(token, project, true, at);
            let chain = match super::epochs::Chain::load(self, project) {
                Ok(chain) => chain,
                Err(_) => return Ok(Reply::error(404, "no such project")),
            };
            // Validate segments before rewriting: decoding must never turn one segment
            // containing a slash into a different canonical route.
            let canonical = parts
                .iter()
                .all(|part| safe_part(part) || person_slot_part(part));
            if canonical {
                let offset = if parts.get(3).is_some_and(|p| p == "objects") {
                    4
                } else {
                    3
                };
                let object = parts[offset..].join("/");
                if let Some((epoch, inner)) = super::epochs::split(&object) {
                    namespace = epoch.to_owned();
                    let inner = inner.split('/').map(str::to_owned).collect::<Vec<_>>();
                    parts.truncate(offset);
                    parts.extend(inner);
                    if offset == 4
                        && parts.len() == 5
                        && parts[4] == "store.json"
                        && request.method == "GET"
                    {
                        parts[3] = "store.json".into();
                        parts.truncate(4);
                    }
                } else if parts.len() == 4
                    && matches!(parts[3].as_str(), "slots" | "challenge" | "token")
                {
                    if let Some(epoch) = query
                        .strip_prefix("epoch=")
                        .filter(|e| layr_store::format::is_uuid(e))
                    {
                        namespace = epoch.to_owned();
                        scoped_query = Some(String::new());
                    }
                } else if parts.len() == 4 && parts[3] == "objects" {
                    if let Some(prefix) = query
                        .strip_prefix("prefix=")
                        .filter(|_| !query.contains('&'))
                        .and_then(decode)
                    {
                        if let Some((epoch, inner)) = super::epochs::split(&prefix) {
                            namespace = epoch.to_owned();
                            let mut url = reqwest::Url::parse("https://gateway.invalid/")?;
                            url.query_pairs_mut().append_pair("prefix", inner);
                            scoped_query = url.query().map(str::to_owned);
                        }
                    }
                }
            }
            let Some(view) = chain.project(&namespace).or_else(|| {
                (grant
                    && request.method == "PUT"
                    && parts.get(3).is_some_and(|p| p == "objects")
                    && layr_store::format::is_uuid(&namespace))
                .then(|| chain.staging(&namespace))
            }) else {
                return Ok(Reply::error(404, "no such project"));
            };
            scoped_read = chain.reads(&self.proof, &parts[2], &namespace, token, at);
            let discovery = (parts.len() == 4
                && parts[3] == "objects"
                && !query.contains('&')
                && query.strip_prefix("prefix=").and_then(decode).as_deref() == Some("keys/"))
                || (parts.len() > 4
                    && parts[3] == "objects"
                    && super::epochs::discovery(&parts[4..].join("/")));
            let bootstrap = (parts.len() == 4
                && matches!(parts[3].as_str(), "store.json" | "challenge" | "token"))
                || (parts.len() == 5 && parts[3] == "slots" && machine_slot(&parts[4]))
                || discovery;
            let create = request.method == "PUT" && parts.get(3).is_some_and(|p| p == "objects");
            let current = chain
                .project(chain.current())
                .expect("current epoch exists");
            if !scoped_read
                && !((grant || gc_grant) && bootstrap)
                && !(grant && create)
                && !(chain.public(self).unwrap_or(false)
                    || self
                        .visible_in(&current, login, chain.current() != "root")
                        .unwrap_or(false))
            {
                return Ok(Reply::error(404, "no such project"));
            }
            scoped_project = Some(view);
            epoch_chain = Some(chain);
        }
        let query = scoped_query.as_deref().unwrap_or(query);
        if request.method != "GET" && request.method != "HEAD" {
            let json = parts.get(3).is_some_and(|p| p == "token");
            if let Some(reply) = other_site(&request, json, public_host) {
                return Ok(reply);
            }
        }
        if matches!(request.method.as_str(), "GET" | "DELETE") && !request.body.is_empty() {
            return Ok(Reply::error(400, "this method has no body"));
        }
        if parts.iter().any(|p| !safe_part(p) && !person_slot_part(p)) {
            return Ok(Reply::error(400, "invalid path"));
        }
        if parts.len() < 4 || parts[0] != "v1" || parts[1] != "p" {
            return Ok(Reply::error(404, "no such route"));
        }
        let name = &parts[2];
        let Some(project) = scoped_project
            .as_ref()
            .or_else(|| config.projects.get(name))
        else {
            return Ok(Reply::error(404, "no such project"));
        };
        let action = &parts[3];
        let root = parts.len() == 4;
        let binding = project.binding(name);
        let token = request
            .header("authorization")
            .and_then(|s| s.strip_prefix("Bearer "))
            .unwrap_or("");
        let at = auth::now();
        let read = scoped_read;
        let write = auth::grant(token, project, false, at);
        let gc = auth::grant(token, project, true, at);
        let empty = |status| Reply::bytes(status, Vec::new(), "application/json");
        if action != "objects" && !query.is_empty() {
            return Ok(Reply::error(400, "unexpected query"));
        }
        if request.method == "GET" && action == "slots" && parts.len() == 5 {
            let name = &parts[4];
            if machine_slot(name) && (write || gc || read) {
                return Ok(
                    match self
                        .storage
                        .get(project, &format!("slots/{name}"), None, SLOT_LIMIT)?
                    {
                        Some(object) => Reply::bytes(200, object.body, "application/octet-stream"),
                        None => Reply::error(404, "no such object"),
                    },
                );
            }
            if layr_store::person::slot_login(name) != login || login.is_none() {
                return Ok(Reply::error(404, "no such object"));
            }
            return Ok(
                match self
                    .storage
                    .get(project, &format!("slots/{name}"), None, 4096)?
                {
                    Some(object) => Reply::bytes(200, object.body, "application/octet-stream"),
                    None => Reply::error(404, "no such object"),
                },
            );
        }
        match (request.method.as_str(), action.as_str(), root) {
            ("GET", "store.json", true) => {
                return Ok(
                    match self.storage.get(project, "store.json", None, JSON_LIMIT)? {
                        Some(object) => Reply::bytes(200, object.body, "application/json"),
                        None => Reply::error(404, "store is not initialized"),
                    },
                );
            }
            ("GET", "slots", true) => {
                let entries = self.storage.list(project, "slots/", Role::Reader)?;
                if entries.len() > 1024 {
                    return Ok(Reply::error(502, "too many store slots"));
                }
                let mut slots = Vec::new();
                let mut size = 0;
                for entry in entries {
                    if entry.key.starts_with("slots/person-")
                        && login.is_none_or(|login| {
                            layr_store::person::slot_login(
                                entry.key.strip_prefix("slots/").unwrap_or(""),
                            ) != Some(login)
                        })
                    {
                        continue;
                    }
                    if std::time::Instant::now() >= deadline {
                        return Ok(Reply::error(408, "request timed out"));
                    }
                    if !object_name(&entry.key) || !entry.key.starts_with("slots/") {
                        bail!("invalid slot key from storage");
                    }
                    if let Some(object) = self.storage.get(project, &entry.key, None, SLOT_LIMIT)? {
                        size += object.body.len();
                        if size > 4 * 1024 * 1024 {
                            bail!("store slots are too large");
                        }
                        slots.push(json!({"name":entry.key.strip_prefix("slots/").unwrap().strip_suffix(".age").unwrap_or(entry.key.strip_prefix("slots/").unwrap()),
                                          "data":base64::engine::general_purpose::STANDARD.encode(object.body)}));
                    }
                }
                return Ok(Reply::json(200, json!({"slots":slots})));
            }
            ("GET", "challenge", true) => {
                return Ok(Reply::json(
                    200,
                    json!({"challenge":self.proof.challenge(&binding, &project.recipient, at)?}),
                ));
            }
            ("POST", "token", true) => {
                if request.body.len() > JSON_LIMIT {
                    return Ok(Reply::error(413, "token request is too large"));
                }
                let value: serde_json::Value = match serde_json::from_slice(&request.body) {
                    Ok(value) => value,
                    Err(_) => return Ok(Reply::error(400, "invalid token request")),
                };
                let proof = value
                    .get("challenge")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                return Ok(match self.proof.exchange(&binding, proof, at) {
                    Some((token, expires)) => {
                        Reply::json(200, json!({"token":token,"expires":expires}))
                    }
                    None => Reply::error(401, "invalid challenge proof"),
                });
            }
            ("GET", "objects", true) => {
                let prefix = match query.strip_prefix("prefix=").and_then(decode) {
                    Some(p) if !query.contains('&') && list_prefix(&p) => p,
                    None if query.is_empty() => String::new(),
                    _ => return Ok(Reply::error(400, "invalid object prefix")),
                };
                let role = if read
                    || ((login.is_some() || gc) && namespace == "root" && prefix == "keys/")
                {
                    Role::Reader
                } else if write
                    && (overlay_keys_prefix(&prefix) || (namespace == "root" && prefix == "keys/"))
                {
                    Role::Writer
                } else {
                    return Ok(Reply::error(
                        401,
                        "a read challenge token or scoped write token is required",
                    ));
                };
                let objects = self.storage.list(project, &prefix, role)?;
                if objects.len() > 10_000 {
                    bail!("too many listed objects");
                }
                let objects: Vec<_> = objects
                    .into_iter()
                    .filter(|e| e.key.starts_with(&prefix))
                    .filter(|e| super::epochs::split(&e.key).is_none_or(|(epoch, _)|
                        epoch_chain.as_ref().is_some_and(|chain| chain.reads(&self.proof, name, epoch, token, at))))
                    .map(|e| json!({"key":if namespace == "root" {e.key} else {format!("project/epochs/{namespace}/{}",e.key)},"modified":e.modified}))
                    .collect();
                return Ok(Reply::json(200, json!({"objects":objects})));
            }
            ("GET" | "PUT" | "DELETE", "objects", false) => {}
            _ => return Ok(Reply::error(405, "method or route is not supported")),
        }
        if !query.is_empty() {
            return Ok(Reply::error(400, "object requests have no query"));
        }
        let key = parts[4..].join("/");
        if !object_name(&key)
            && !(key == "store.json" && (request.method == "GET" || namespace != "root"))
        {
            return Ok(Reply::error(400, "invalid object name"));
        }
        if namespace != "root" && super::epochs::discovery(&key) {
            return Ok(Reply::error(400, "discovery belongs to the root namespace"));
        }
        if (namespace == "root" && key.starts_with("claims/"))
            || (namespace != "root" && key.starts_with("requests/claims/"))
        {
            return Ok(Reply::error(400, "claim namespace differs"));
        }
        match request.method.as_str() {
            "GET" => {
                if !read
                    && !(write && (overlay_key(&key) || super::epochs::discovery(&key)))
                    && !(gc && namespace == "root" && super::epochs::discovery(&key))
                    && !(login.is_some() && super::epochs::discovery(&key))
                {
                    return Ok(Reply::error(401, "a read challenge token is required"));
                }
                let range = request
                    .header("range")
                    .map(http::Range::parse)
                    .transpose()?;
                Ok(
                    match self.storage.get(project, &key, range, OBJECT_LIMIT)? {
                        Some(object) => {
                            let mut reply = Reply::bytes(
                                object.status,
                                object.body,
                                "application/octet-stream",
                            );
                            reply.headers.push(("Accept-Ranges".into(), "bytes".into()));
                            if let Some(range) = object.content_range {
                                reply.headers.push(("Content-Range".into(), range));
                            }
                            reply
                        }
                        None => Reply::error(404, "no such object"),
                    },
                )
            }
            "PUT" => {
                if matches!(key.as_str(), "store.json" | "public.json") {
                    if !write {
                        return Ok(Reply::error(403, "a write token is required"));
                    }
                    if request.body.len() > JSON_LIMIT {
                        return Ok(Reply::error(413, "metadata is too large"));
                    }
                    if key == "store.json" {
                        let descriptor =
                            serde_json::from_slice::<layr_store::format::Descriptor>(&request.body);
                        let anchor = config.projects.get(name).context("project disappeared")?;
                        let root_descriptor = self
                            .storage
                            .get(anchor, "store.json", None, JSON_LIMIT)?
                            .map(|o| {
                                serde_json::from_slice::<layr_store::format::Descriptor>(&o.body)
                            })
                            .transpose()?;
                        let published = epoch_chain
                            .as_ref()
                            .and_then(|chain| chain.project(&namespace));
                        let valid = descriptor.is_ok_and(|d| {
                            d.format == 2
                                && layr_store::format::is_uuid(&d.project)
                                && layr_store::keys::valid_recipient(&d.recipient)
                                && root_descriptor
                                    .as_ref()
                                    .is_none_or(|root| d.project == root.project)
                                && published
                                    .as_ref()
                                    .is_none_or(|scope| d.recipient == scope.recipient)
                        });
                        if !valid {
                            return Ok(Reply::error(400, "invalid store descriptor"));
                        }
                    } else {
                        let Some(descriptor) =
                            self.storage.get(project, "store.json", None, JSON_LIMIT)?
                        else {
                            return Ok(Reply::error(404, "store is not initialized"));
                        };
                        let descriptor: layr_store::format::Descriptor =
                            serde_json::from_slice(&descriptor.body)?;
                        if layr_store::public::Manifest::parse(&request.body, &descriptor).is_err()
                        {
                            return Ok(Reply::error(400, "invalid public manifest"));
                        }
                    }
                }
                if super::epochs::discovery(&key) {
                    let Some(descriptor) =
                        self.storage.get(project, "store.json", None, JSON_LIMIT)?
                    else {
                        return Ok(Reply::error(404, "store is not initialized"));
                    };
                    let descriptor: layr_store::format::Descriptor =
                        serde_json::from_slice(&descriptor.body)?;
                    if super::epochs::key_record(&key, &request.body, &descriptor.project).is_err()
                    {
                        return Ok(Reply::error(400, "invalid epoch discovery record"));
                    }
                }
                if let Some((claim, id)) = super::requests::path(&key) {
                    if namespace != "root" && super::epochs::discovery(&key) {
                        return Ok(Reply::error(400, "discovery belongs to the root namespace"));
                    }
                    if (namespace == "root" && key.starts_with("claims/"))
                        || (namespace != "root" && key.starts_with("requests/claims/"))
                    {
                        return Ok(Reply::error(400, "claim namespace differs"));
                    }
                    return self
                        .put_request(&request, project, &key, claim, id, &namespace, login, write);
                }
                if key.starts_with("slots/") {
                    if !read && !(namespace != "root" && write) {
                        return Ok(Reply::error(
                            403,
                            "slot writes require a read challenge token",
                        ));
                    }
                    if request.body.len() > SLOT_LIMIT {
                        return Ok(Reply::error(413, "slot is too large"));
                    }
                } else if !write {
                    return Ok(Reply::error(403, "a write token is required"));
                }
                if overlay_epoch(&key).is_some_and(|key| key.starts_with("slots/"))
                    && request.body.len() > SLOT_LIMIT
                {
                    return Ok(Reply::error(413, "slot is too large"));
                }
                if request.header("content-type") != Some("application/octet-stream") {
                    return Ok(Reply::error(415, "send application/octet-stream"));
                }
                if request.header("content-length").is_none() {
                    return Ok(Reply::error(411, "Content-Length is required"));
                }
                if key.strip_prefix("slots/").is_some_and(person_slot_part)
                    && (request.body.is_empty() || request.body.len() > 4096)
                {
                    return Ok(Reply::error(400, "a slot is 1 to 4096 bytes"));
                }
                Ok(if self.storage.put(project, &key, request.body)? {
                    empty(201)
                } else {
                    Reply::error(412, "object already exists")
                })
            }
            "DELETE" => {
                if !gc || !gc_name(&key) {
                    return Ok(Reply::error(403, "GC can delete only state data parts"));
                }
                self.storage.delete(project, &key)?;
                Ok(empty(204))
            }
            _ => unreachable!(),
        }
    }
}

fn person_slot_part(part: &str) -> bool {
    layr_store::person::slot_login(part).is_some()
}
fn bearer(request: &http::Request) -> &str {
    request
        .header("authorization")
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("")
}
fn other_site(
    request: &http::Request,
    require_json: bool,
    public_host: Option<&str>,
) -> Option<Reply> {
    const REFUSED: &str = "a page of another site cannot change this gateway";
    if request
        .header("sec-fetch-site")
        .is_some_and(|site| !matches!(site, "same-origin" | "none"))
    {
        return Some(Reply::error(403, REFUSED));
    }
    if request.header("origin").is_some_and(|origin| {
        !origin.split_once("://").is_some_and(|(_, rest)| {
            rest.eq_ignore_ascii_case(
                public_host.unwrap_or_else(|| request.header("host").unwrap_or("")),
            )
        })
    }) {
        return Some(Reply::error(403, REFUSED));
    }
    if require_json
        && !request
            .header("content-type")
            .and_then(|kind| kind.split(';').next())
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
    {
        return Some(Reply::error(
            415,
            "send JSON (content-type: application/json)",
        ));
    }
    None
}
fn read_json(request: &http::Request, limit: usize) -> Result<serde_json::Value> {
    if request.body.len() > limit {
        return super::person_support::fail(413, "the body is too large");
    }
    if request.body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&request.body).map_err(|_| {
        http::Error {
            status: 400,
            message: "the body is not JSON",
        }
        .into()
    })
}
impl Gateway {
    fn visible(&self, project: &Project, login: Option<&str>) -> Result<bool> {
        let chain = super::epochs::Chain::load(self, project)?;
        if chain.public(self).unwrap_or(false) {
            return Ok(true);
        }
        self.visible_in(
            &chain
                .project(chain.current())
                .expect("current epoch exists"),
            login,
            chain.current() != "root",
        )
    }
    fn visible_in(&self, project: &Project, login: Option<&str>, strict: bool) -> Result<bool> {
        let slots = self.storage.list(project, "slots/", Role::Reader)?;
        if slots.len() > 1024 {
            bail!("too many store slots");
        }
        let people: Vec<_> = slots
            .iter()
            .filter_map(|entry| layr_store::person::slot_login(entry.key.strip_prefix("slots/")?))
            .collect();
        Ok((!strict && people.is_empty()) || login.is_some_and(|login| people.contains(&login)))
    }
    fn api(
        &self,
        request: &http::Request,
        config: &super::config::Config,
        login: Option<&str>,
        parts: &[String],
    ) -> Result<Reply> {
        let kind = parts.first().map(String::as_str);
        if kind == Some("projects") && parts.len() == 1 && request.method == "GET" {
            let mut projects = Vec::new();
            for (name, project) in &config.projects {
                match self.visible(project, login) {
                    Ok(true) => projects.push(name),
                    Ok(false) => (),
                    Err(_) => eprintln!("layr-gateway: cannot check project visibility"),
                }
            }
            return Ok(Reply::json(200, json!({"projects":projects})));
        }
        if !matches!(kind, Some("person" | "people")) {
            return Ok(Reply::error(404, "no such route"));
        }
        let Some(person_config) = &config.people else {
            return Ok(Reply::error(404, "this gateway keeps no person keys"));
        };
        let Some(login) = login.filter(|login| layr_store::person::login_name(login)) else {
            return Ok(Reply::error(401, "sign in to the gateway"));
        };
        let people = super::people::People::new(
            self.storage.people(person_config)?,
            &person_config.prefix,
            self.person_secret.clone(),
            Arc::new(auth::now),
            self.person_writing.clone(),
        );
        if kind == Some("people") {
            if parts.len() != 3
                || parts[2] != "inbox"
                || request.method != "POST"
                || !layr_store::person::login_name(&parts[1])
            {
                return Ok(Reply::error(404, "no such route"));
            }
            let body = read_json(request, 16 * 1024)?;
            let id = people.drop_invite(&parts[1], login, body["data"].as_str().unwrap_or(""))?;
            return Ok(Reply::json(200, json!({"id":id})));
        }
        if parts.len() > 3 {
            return Ok(Reply::error(404, "no such route"));
        }
        let action = parts.get(1).map(String::as_str);
        let name = parts.get(2).map(String::as_str);
        if request.method == "GET" && action.is_none() && name.is_none() {
            return Ok(Reply::json(200, people.view(login)?));
        }
        if self.sessions.login(bearer(request), auth::now()).as_deref() != Some(login) {
            return Ok(Reply::error(401, "the token is missing, wrong or expired"));
        }
        let body = read_json(request, 68 * 1024)?;
        let data = body["data"].as_str().unwrap_or("");
        let value = match (request.method.as_str(), action, name) {
            ("PUT", Some("index"), None) => {
                people.put_index(login, data, body["previous"].as_str())?;
                json!({"saved":"index"})
            }
            ("PUT", Some("slots"), Some(name)) => {
                people.add_slot(login, name, data)?;
                json!({"added":name})
            }
            ("DELETE", Some("slots"), Some(name)) => {
                people.remove_slot(login, name)?;
                json!({"removed":name})
            }
            ("DELETE", Some("inbox"), Some(id)) => {
                people.remove_invite(login, id)?;
                json!({"removed":id})
            }
            _ => return Ok(Reply::error(404, "no such route")),
        };
        Ok(Reply::json(200, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);
    impl super::super::person_support::Bucket for Memory {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn list(&self, prefix: &str) -> Result<Vec<String>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect())
        }
        fn put_new(&self, key: &str, body: Vec<u8>) -> Result<bool> {
            let mut objects = self.0.lock().unwrap();
            if objects.contains_key(key) {
                return Ok(false);
            }
            objects.insert(key.into(), body);
            Ok(true)
        }
        fn put(&self, key: &str, body: Vec<u8>) -> Result<()> {
            self.0.lock().unwrap().insert(key.into(), body);
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    fn memory_key(project: &Project, key: &str) -> String {
        match project.prefix.split_once("/project/epochs/") {
            Some((_, epoch)) => format!("project/epochs/{}/{key}", epoch.trim_end_matches('/')),
            None => key.to_owned(),
        }
    }
    impl Storage for Memory {
        fn people(
            &self,
            _: &super::super::config::People,
        ) -> Result<Box<dyn super::super::person_support::Bucket>> {
            Ok(Box::new(self.clone()))
        }

        fn get(
            &self,
            project: &Project,
            key: &str,
            range: Option<http::Range>,
            _: usize,
        ) -> Result<Option<Object>> {
            let objects = self.0.lock().unwrap();
            let key = memory_key(project, key);
            let Some(body) = objects.get(&key) else {
                return Ok(None);
            };
            Ok(Some(match range {
                Some(range) => {
                    let (start, end) = range.resolve(body.len() as u64).ok_or(http::Error {
                        status: 416,
                        message: "outside object",
                    })?;
                    Object {
                        status: 206,
                        body: body[start as usize..=end as usize].to_vec(),
                        content_range: Some(format!("bytes {start}-{end}/{}", body.len())),
                    }
                }
                None => Object {
                    status: 200,
                    body: body.clone(),
                    content_range: None,
                },
            }))
        }
        fn list(&self, project: &Project, prefix: &str, _: Role) -> Result<Vec<Entry>> {
            let root = memory_key(project, "");
            let prefix = memory_key(project, prefix);
            Ok(self
                .0
                .lock()
                .unwrap()
                .keys()
                .filter(|s| s.starts_with(&prefix))
                .map(|s| Entry {
                    key: s.strip_prefix(&root).unwrap().to_string(),
                    modified: 0,
                })
                .collect())
        }
        fn put(&self, project: &Project, key: &str, body: Vec<u8>) -> Result<bool> {
            let key = memory_key(project, key);
            let mut objects = self.0.lock().unwrap();
            if objects.contains_key(&key) {
                return Ok(false);
            }
            objects.insert(key.into(), body);
            Ok(true)
        }
        fn delete(&self, project: &Project, key: &str) -> Result<()> {
            self.0.lock().unwrap().remove(&memory_key(project, key));
            Ok(())
        }
    }
    struct Fixture {
        gateway: Gateway,
        identity: age::x25519::Identity,
        config: super::super::config::Config,
        path: std::path::PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let identity = age::x25519::Identity::generate();
            let mut config = super::super::config::sample();
            config.projects.get_mut("example").unwrap().recipient =
                identity.to_public().to_string();
            let path = std::env::temp_dir().join(format!(
                "layr-gateway-routes-{:016x}.json",
                rand::random::<u64>()
            ));
            std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
            let gateway = Gateway {
                config: Arc::new(ConfigFile::new(path.clone())),
                proof: auth::Proof::new(),
                listen: config.listen,
                storage: Arc::new(Memory::default()),
                origin: None,
                person_secret: Arc::new(rand::random()),
                person_writing: Arc::new(Mutex::new(())),
                public_limit: super::super::public::Limiter::default(),
                sessions: super::super::session::Sessions::default(),
                bootstrap_limit: super::super::public::Limiter::default(),
            };
            Self {
                gateway,
                identity,
                config,
                path,
            }
        }
        fn request(
            &self,
            method: &str,
            path: &str,
            token: &str,
            body: &[u8],
            range: Option<&str>,
        ) -> Reply {
            let mut headers: BTreeMap<String, String> = [
                ("host".into(), "localhost".into()),
                ("content-length".into(), body.len().to_string()),
                (
                    "content-type".into(),
                    if path.contains("/objects/") {
                        "application/octet-stream".into()
                    } else {
                        "application/json".into()
                    },
                ),
            ]
            .into();
            if !token.is_empty() {
                headers.insert("authorization".into(), format!("Bearer {token}"));
            }
            if let Some(range) = range {
                headers.insert("range".into(), range.into());
            }
            self.gateway.handle(
                http::Request {
                    method: method.into(),
                    target: path.into(),
                    headers,
                    body: body.to_vec(),
                },
                None,
            )
        }
        fn token(&mut self, gc: bool) -> String {
            let token = auth::issue(gc, 60_000, auth::now()).unwrap();
            let project = self.config.projects.get_mut("example").unwrap();
            if gc {
                project.gc_token_hashes.push(auth::hash(&token));
            } else {
                project.write_token_hashes.push(auth::hash(&token));
            }
            std::fs::write(&self.path, serde_json::to_vec(&self.config).unwrap()).unwrap();
            token
        }
        fn read_token(&self) -> String {
            let reply = self.request("GET", "/v1/p/example/challenge", "", b"", None);
            assert_eq!(reply.status, 200);
            let value: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(value["challenge"].as_str().unwrap())
                .unwrap();
            let proof = String::from_utf8(age::decrypt(&self.identity, &bytes).unwrap()).unwrap();
            let body = json!({"challenge":proof}).to_string();
            let reply = self.request("POST", "/v1/p/example/token", "", body.as_bytes(), None);
            assert_eq!(reply.status, 200);
            serde_json::from_slice::<serde_json::Value>(&reply.body).unwrap()["token"]
                .as_str()
                .unwrap()
                .into()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    const OVERLAY: &str = "01a12290-0000-7000-8000-000000000001";
    const EPOCH: &str = "01a12290-0000-7000-8000-000000000002";

    #[test]
    fn release_file_discovery_needs_a_read_proof() {
        let mut f = Fixture::new();
        let read = f.read_token();
        let write = f.token(false);
        let path = "/v1/p/example/objects?prefix=files%2F";
        let reply = f.request("GET", path, &read, b"", None);
        assert_eq!(reply.status, 200);
        assert_eq!(value(reply), json!({"objects":[]}));
        assert_eq!(f.request("GET", path, &write, b"", None).status, 401);
    }

    #[test]
    fn epoch_grants_stage_then_discover_without_reading_new_ciphertext_and_proofs_are_scoped() {
        use layr_store::{keys, person};
        let mut f = Fixture::new();
        let expired = auth::issue(false, 1, 0).unwrap();
        f.config
            .projects
            .get_mut("example")
            .unwrap()
            .write_token_hashes
            .push(auth::hash(&expired));
        let other_grant = auth::issue(false, 60_000, auth::now()).unwrap();
        let mut other = f.config.projects["example"].clone();
        other.write_token_hashes = vec![auth::hash(&other_grant)];
        f.config.projects.insert("other".into(), other);
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        let project = f.config.projects["example"].clone();
        let write = f.token(false);
        let root_read = f.read_token();
        let gc = f.token(true);
        let other_gc = auth::issue(true, 60_000, auth::now()).unwrap();
        f.config.projects.get_mut("other").unwrap().gc_token_hashes = vec![auth::hash(&other_gc)];
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        let next = age::x25519::Identity::generate();
        let descriptor = |recipient: String| {
            serde_json::to_vec(&json!({"format":2,"project":OVERLAY,"recipient":recipient}))
                .unwrap()
        };
        f.gateway
            .storage
            .put(
                &project,
                "store.json",
                descriptor(project.recipient.clone()),
            )
            .unwrap();
        f.gateway
            .storage
            .put(
                &project,
                "slots/person-owner@example.com-0123456789abcdef.age",
                b"root person slot".to_vec(),
            )
            .unwrap();
        f.gateway
            .storage
            .put(&project, "records/root.age", b"old ciphertext".to_vec())
            .unwrap();
        let base = format!("/v1/p/example/objects/project/epochs/{EPOCH}");
        assert_eq!(
            f.request(
                "PUT",
                &format!("{base}/store.json"),
                &write,
                &descriptor(next.to_public().to_string()),
                None
            )
            .status,
            201
        );
        assert_eq!(
            f.request("GET", &format!("{base}/store.json"), &write, b"", None)
                .status,
            404
        );
        let own = format!("slots/machine-{OVERLAY}.age");
        assert_eq!(
            f.request(
                "PUT",
                &format!("{base}/{own}"),
                &write,
                b"sealed machine slot",
                None
            )
            .status,
            201
        );
        assert_eq!(
            f.request(
                "PUT",
                &format!("{base}/history/root.age"),
                &write,
                b"sealed history",
                None
            )
            .status,
            201
        );
        assert_eq!(
            f.request(
                "PUT",
                &format!("{base}/states/{OVERLAY}/01234567.tree.age"),
                &write,
                b"new ciphertext",
                None
            )
            .status,
            201
        );
        let identity = keys::generate_identity();
        let signing = person::make_signing_keys(&keys::recipient_of(&identity).unwrap()).unwrap();
        let signing =
            person::open_signing_key(&identity, &signing.sealed, &signing.public_key).unwrap();
        let key = layr_store::epochs::Key {
            project: OVERLAY.into(),
            parent: "root".into(),
            parent_recipient: project.recipient.clone(),
            epoch: EPOCH.into(),
            recipient: next.to_public().to_string(),
            mode: layr_store::epochs::Mode::Private,
            readers_hash: "0".repeat(64),
        };
        let record = json!({"id":OVERLAY,"project":OVERLAY,"op":"project.key","data":key});
        let body = serde_json::to_vec(&record).unwrap();
        let signed = format!(
            "{{\"r\":{},\"s\":\"{}\"}}",
            String::from_utf8(body.clone()).unwrap(),
            keys::sign_bytes(&body, &signing)
        );
        let discovery = format!("/v1/p/example/objects/keys/{EPOCH}-{OVERLAY}.json");
        assert_eq!(
            f.request("PUT", &discovery, &write, signed.as_bytes(), None)
                .status,
            201
        );
        assert_eq!(
            f.request("PUT", &discovery, &write, signed.as_bytes(), None)
                .status,
            412
        );
        for token in [
            "",
            "wrong",
            other_grant.as_str(),
            other_gc.as_str(),
            expired.as_str(),
        ] {
            for (method, path) in [
                ("GET", "/v1/p/example/store.json".into()),
                ("GET", format!("{base}/store.json")),
                ("GET", format!("/v1/p/example/challenge?epoch={EPOCH}")),
                ("POST", format!("/v1/p/example/token?epoch={EPOCH}")),
                ("GET", format!("/v1/p/example/project/epochs/{EPOCH}/{own}")),
                ("GET", "/v1/p/example/objects?prefix=keys%2F".into()),
                ("GET", discovery.clone()),
            ] {
                let hidden = f.request(method, &path, token, b"", None);
                let absent = f.request(method, "/v1/p/absent/store.json", token, b"", None);
                assert_eq!(
                    (hidden.status, hidden.headers, hidden.body),
                    (absent.status, absent.headers, absent.body),
                    "{path}"
                );
            }
        }
        assert_eq!(
            f.request("GET", &discovery, &write, b"", None).body,
            signed.as_bytes()
        );
        assert_eq!(
            f.request("GET", &discovery, &gc, b"", None).body,
            signed.as_bytes()
        );
        assert_eq!(
            f.request(
                "GET",
                "/v1/p/example/objects?prefix=keys%2F",
                &gc,
                b"",
                None
            )
            .status,
            200
        );
        for path in [
            format!("{base}/states/{OVERLAY}/01234567.tree.age"),
            format!("{base}/history/root.age"),
        ] {
            assert_eq!(
                f.request("GET", &path, &gc, b"", None).status,
                404,
                "GC must not read encrypted bytes: {path}"
            );
        }
        assert_eq!(
            f.request(
                "GET",
                "/v1/p/example/objects?prefix=keys%2F",
                &write,
                b"",
                None
            )
            .status,
            200
        );
        for token in ["", "wrong", root_read.as_str()] {
            let hidden = f.request(
                "GET",
                &format!("{base}/states/{OVERLAY}/01234567.tree.age"),
                token,
                b"",
                None,
            );
            let absent = f.request("GET", "/v1/p/absent/store.json", token, b"", None);
            assert_eq!(
                (hidden.status, hidden.headers, hidden.body),
                (absent.status, absent.headers, absent.body)
            );
        }
        assert_eq!(
            f.request(
                "GET",
                &format!("/v1/p/example/project/epochs/{EPOCH}/{own}"),
                &write,
                b"",
                None
            )
            .status,
            200
        );
        assert_eq!(
            f.request(
                "GET",
                &format!("/v1/p/example/slots?epoch={EPOCH}"),
                &write,
                b"",
                None
            )
            .status,
            404
        );
        assert_eq!(
            f.request("GET", &format!("{base}/store.json"), &write, b"", None)
                .status,
            200
        );
        let challenge = f.request(
            "GET",
            &format!("/v1/p/example/challenge?epoch={EPOCH}"),
            &write,
            b"",
            None,
        );
        assert_eq!(challenge.status, 200);
        let challenge: serde_json::Value = serde_json::from_slice(&challenge.body).unwrap();
        let sealed = base64::engine::general_purpose::STANDARD
            .decode(challenge["challenge"].as_str().unwrap())
            .unwrap();
        assert!(age::decrypt(&f.identity, &sealed).is_err());
        let clear = String::from_utf8(age::decrypt(&next, &sealed).unwrap()).unwrap();
        let body = json!({"challenge":clear}).to_string();
        assert_eq!(
            f.request("POST", "/v1/p/example/token", &write, body.as_bytes(), None)
                .status,
            401
        );
        let token = f.request(
            "POST",
            &format!("/v1/p/example/token?epoch={EPOCH}"),
            &write,
            body.as_bytes(),
            None,
        );
        assert_eq!(token.status, 200);
        let token: serde_json::Value = serde_json::from_slice(&token.body).unwrap();
        let read = token["token"].as_str().unwrap();
        assert_eq!(
            f.request("GET", &format!("{base}/history/root.age"), read, b"", None)
                .body,
            b"sealed history"
        );
        assert_eq!(
            f.request(
                "GET",
                "/v1/p/example/objects/records/root.age",
                read,
                b"",
                None
            )
            .body,
            b"old ciphertext"
        );
        let listed = f.request(
            "GET",
            &format!("/v1/p/example/objects?prefix=project%2Fepochs%2F{EPOCH}%2Fstates%2F"),
            read,
            b"",
            None,
        );
        assert_eq!(listed.status, 200);
        let listed: serde_json::Value = serde_json::from_slice(&listed.body).unwrap();
        assert_eq!(
            listed["objects"][0]["key"],
            format!("project/epochs/{EPOCH}/states/{OVERLAY}/01234567.tree.age")
        );
        let old_listing = f.request(
            "GET",
            "/v1/p/example/objects?prefix=",
            &root_read,
            b"",
            None,
        );
        assert_eq!(old_listing.status, 200);
        assert!(!String::from_utf8(old_listing.body)
            .unwrap()
            .contains("project/epochs/"));
        // Opening an existing epoch does not change its discovery key's earlier mode.
        let root_descriptor = layr_store::format::Descriptor {
            format: 2,
            project: OVERLAY.into(),
            recipient: project.recipient.clone(),
        };
        let root_manifest = layr_store::public::Manifest::new(
            &root_descriptor,
            keys::generate_identity(),
            OVERLAY.into(),
            &signing,
        )
        .unwrap();
        f.gateway
            .storage
            .put(
                &project,
                "public.json",
                serde_json::to_vec(&root_manifest).unwrap(),
            )
            .unwrap();
        assert_eq!(
            f.request("GET", "/p/example/store.json", "", b"", None)
                .status,
            404,
            "an old public opener cannot expose a private current epoch"
        );
        let mut scoped = project.clone();
        scoped.prefix = format!("{}/project/epochs/{EPOCH}", project.prefix);
        scoped.recipient = next.to_public().to_string();
        let current_descriptor = layr_store::format::Descriptor {
            format: 2,
            project: OVERLAY.into(),
            recipient: scoped.recipient.clone(),
        };
        let manifest = layr_store::public::Manifest::new(
            &current_descriptor,
            keys::generate_identity(),
            OVERLAY.into(),
            &signing,
        )
        .unwrap();
        f.gateway
            .storage
            .put(
                &scoped,
                "public.json",
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
        assert_eq!(
            f.request(
                "GET",
                &format!("/p/example/project/epochs/{EPOCH}/states/{OVERLAY}/01234567.tree.age"),
                "",
                b"",
                None
            )
            .status,
            200
        );
        let later = "01a12290-0000-7000-8000-000000000003";
        let later_recipient = keys::recipient_of(&keys::generate_identity()).unwrap();
        let next_base = format!("/v1/p/example/objects/project/epochs/{later}");
        assert_eq!(
            f.request(
                "PUT",
                &format!("{next_base}/store.json"),
                &write,
                &descriptor(later_recipient.clone()),
                None
            )
            .status,
            201
        );
        let later_key = layr_store::epochs::Key {
            project: OVERLAY.into(),
            parent: EPOCH.into(),
            parent_recipient: scoped.recipient.clone(),
            epoch: later.into(),
            recipient: later_recipient,
            mode: layr_store::epochs::Mode::Private,
            readers_hash: "0".repeat(64),
        };
        let record = serde_json::to_vec(
            &json!({"id":OVERLAY,"project":OVERLAY,"op":"project.key","data":later_key}),
        )
        .unwrap();
        let signed = format!(
            "{{\"r\":{},\"s\":\"{}\"}}",
            String::from_utf8(record.clone()).unwrap(),
            keys::sign_bytes(&record, &signing)
        );
        assert_eq!(
            f.request(
                "PUT",
                &format!("/v1/p/example/objects/keys/{later}-{OVERLAY}.json"),
                &write,
                signed.as_bytes(),
                None
            )
            .status,
            201
        );
        for path in [
            "/p/example/store.json".to_string(),
            format!("/p/example/project/epochs/{EPOCH}/store.json"),
            format!("/p/example/project/epochs/{later}/store.json"),
        ] {
            assert_eq!(f.request("GET", &path, "", b"", None).status, 404, "{path}");
        }
        let hidden = f.request(
            "GET",
            &format!("{next_base}/history/{EPOCH}.age"),
            read,
            b"",
            None,
        );
        let absent = f.request("GET", "/v1/p/absent/store.json", read, b"", None);
        assert_eq!(
            (hidden.status, hidden.headers, hidden.body),
            (absent.status, absent.headers, absent.body)
        );
    }

    #[test]
    fn signed_index_objects_require_writer_create_and_reader_proof_and_refuse_gc() {
        let mut f = Fixture::new();
        let write = f.token(false);
        let gc = f.token(true);
        let read = f.read_token();
        for name in [
            layr_store::index::heads_name(OVERLAY, 1),
            layr_store::index::paths_name(EPOCH),
            layr_store::index::graph_name(EPOCH),
        ] {
            let path = format!("/v1/p/example/objects/{name}");
            assert_eq!(
                f.request("PUT", &path, "", b"encrypted index", None).status,
                403
            );
            assert_eq!(
                f.request("PUT", &path, &write, b"encrypted index", None)
                    .status,
                201
            );
            assert_eq!(
                f.request("PUT", &path, &write, b"overwrite", None).status,
                412
            );
            assert_eq!(f.request("GET", &path, &write, b"", None).status, 401);
            assert_eq!(
                f.request("GET", &path, &read, b"", None).body,
                b"encrypted index"
            );
            assert_eq!(f.request("DELETE", &path, &gc, b"", None).status, 403);
            assert_eq!(
                f.request("GET", &format!("/p/example/{name}"), "", b"", None)
                    .status,
                404
            );
        }
        let list = f.request(
            "GET",
            "/v1/p/example/objects?prefix=index%2Fheads%2F",
            &read,
            b"",
            None,
        );
        assert_eq!(list.status, 200);
        let listed: serde_json::Value = serde_json::from_slice(&list.body).unwrap();
        assert_eq!(listed["objects"].as_array().unwrap().len(), 1);
        for name in [
            format!("index/heads/{OVERLAY}/00000000000000000000.age"),
            format!("index/heads/{OVERLAY}/1.age"),
            format!("index/graph/{EPOCH}.age/extra"),
            "index/paths/../key.age".into(),
            format!("index/paths/{EPOCH}.age\n"),
        ] {
            assert!(!object_name(&name), "{name}");
        }
    }

    #[test]
    fn public_reads_require_publication_and_exclude_private_routes_and_writes() {
        use layr_store::{format, keys, person, public::Manifest};
        let f = Fixture::new();
        let project = &f.config.projects["example"];
        let descriptor = format::Descriptor {
            format: 2,
            project: OVERLAY.into(),
            recipient: project.recipient.clone(),
        };
        f.gateway
            .storage
            .put(
                project,
                "store.json",
                serde_json::to_vec(&descriptor).unwrap(),
            )
            .unwrap();
        let record = format::segment_name(OVERLAY, 1, 5);
        let tree = format::tree_name(EPOCH, "0123abcd");
        let data = format::data_name(EPOCH, "0123abcd", 0);
        for key in [
            &record,
            &tree,
            &data,
            "slots/public.age",
            "records/private-name",
            "people/ann/person.json",
        ] {
            f.gateway
                .storage
                .put(project, key, b"encrypted".to_vec())
                .unwrap();
        }
        for path in [
            "/p/example/store.json",
            "/p/example/public.json",
            "/p/example/objects?prefix=records/",
            "/p/example/slots/public.age",
        ] {
            assert_eq!(f.request("GET", path, "", b"", None).status, 404, "{path}");
        }
        let identity = keys::generate_identity();
        let signing = person::make_signing_keys(&keys::recipient_of(&identity).unwrap()).unwrap();
        let key =
            person::open_signing_key(&identity, &signing.sealed, &signing.public_key).unwrap();
        let manifest =
            Manifest::new(&descriptor, keys::generate_identity(), EPOCH.into(), &key).unwrap();
        f.gateway
            .storage
            .put(
                project,
                "public.json",
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
        for name in [
            "store.json",
            "public.json",
            "slots/public.age",
            &record,
            &tree,
            &data,
        ] {
            let reply = f.request("GET", &format!("/p/example/{name}"), "", b"", None);
            assert_eq!(reply.status, 200, "{name}");
        }
        let reply = f.request(
            "GET",
            &format!("/p/example/{data}"),
            "",
            b"",
            Some("bytes=1-3"),
        );
        assert_eq!(reply.status, 206);
        assert_eq!(reply.body, b"ncr");
        for (prefix, expected) in [
            ("records%2F", vec![record.as_str()]),
            ("states/", vec![data.as_str(), tree.as_str()]),
            (
                &format!("states/{EPOCH}/"),
                vec![data.as_str(), tree.as_str()],
            ),
        ] {
            let reply = f.request(
                "GET",
                &format!("/p/example/objects?prefix={prefix}"),
                "",
                b"",
                None,
            );
            assert_eq!(reply.status, 200);
            let value: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
            let names: Vec<_> = value["objects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o["key"].as_str().unwrap())
                .collect();
            assert_eq!(names, expected);
            assert!(value["objects"]
                .as_array()
                .unwrap()
                .iter()
                .all(|o| o["modified"] == 0));
        }
        for path in [
            "overlays/id/keys/key.json",
            "people/ann/person.json",
            "slots/paper.age",
            "requests/id.json",
            "../store.json",
            "%2e%2e/store.json",
            "records%2f..%2fstore.json",
            "public.json%0a",
            "objects/records/x",
            "token",
            "challenge",
        ] {
            assert_eq!(
                f.request("GET", &format!("/p/example/{path}"), "", b"", None)
                    .status,
                404,
                "{path}"
            );
        }
        for prefix in [
            "",
            "people/",
            "overlays/",
            "slots/",
            "requests/",
            "records/../",
            "records/&prefix=people/",
        ] {
            assert_eq!(
                f.request(
                    "GET",
                    &format!("/p/example/objects?prefix={prefix}"),
                    "",
                    b"",
                    None
                )
                .status,
                400
            );
        }
        for method in ["PUT", "POST", "DELETE", "PATCH", "HEAD"] {
            for path in ["store.json", "token", "objects", &record] {
                assert_eq!(
                    f.request(method, &format!("/p/example/{path}"), "", b"", None)
                        .status,
                    405
                );
            }
        }
        assert_eq!(
            f.request(
                "PUT",
                &format!("/v1/p/example/objects/{record}"),
                "",
                b"new",
                None
            )
            .status,
            403
        );
        assert_eq!(
            f.request(
                "DELETE",
                &format!("/v1/p/example/objects/{data}"),
                "",
                b"",
                None
            )
            .status,
            403
        );
        assert_eq!(
            f.request("POST", "/v1/p/example/token", "", b"{}", None)
                .status,
            401
        );
        // A corrupted or descriptor-mismatched publication cannot open anonymous access.
        f.gateway.storage.delete(project, "public.json").unwrap();
        let mut invalid = serde_json::to_value(&manifest).unwrap();
        invalid["project"] = json!(EPOCH);
        f.gateway
            .storage
            .put(
                project,
                "public.json",
                serde_json::to_vec(&invalid).unwrap(),
            )
            .unwrap();
        assert_eq!(
            f.request("GET", "/p/example/store.json", "", b"", None)
                .status,
            404
        );
    }

    #[test]
    fn origin_guard_covers_routes_and_limits_each_authenticated_viewer() {
        let mut f = Fixture::new();
        let edge = "2001:db8::1".parse().unwrap();
        let request = |origin: Option<&str>, viewer: &str| {
            let mut headers = std::collections::BTreeMap::from([
                ("host".into(), "localhost".into()),
                ("cloudfront-viewer-address".into(), viewer.into()),
            ]);
            if let Some(secret) = origin {
                headers.insert(super::super::origin::HEADER.into(), secret.into());
            }
            http::Request {
                method: "GET".into(),
                target: "/api/person/unlock?login=unknown%40example.com".into(),
                headers,
                body: vec![],
            }
        };
        // Without edge mode, forwarded headers cannot replace the socket address.
        let forged = request(None, "198.51.100.9:1234");
        assert_eq!(
            f.gateway.client_address(&forged.headers, edge).ok(),
            Some(edge)
        );
        let secret = "ab".repeat(32);
        f.gateway.origin = Some(super::super::origin::Origin::new(secret.clone()).unwrap());
        for path in ["/", "/v1/person/unlock", "/p/example/store.json"] {
            let mut forged = request(None, "198.51.100.9:1234");
            forged.target = path.into();
            assert_eq!(f.gateway.handle_from(forged, None, edge).status, 403);
        }
        assert_eq!(
            f.gateway
                .handle_from(
                    request(Some(&"ac".repeat(32)), "198.51.100.9:1234"),
                    None,
                    edge
                )
                .status,
            403
        );
        // Malformed trusted addresses fail closed rather than sharing the edge IP.
        assert_eq!(
            f.gateway
                .handle_from(request(Some(&secret), "invalid"), None, edge)
                .status,
            400
        );
        for _ in 0..12 {
            assert_ne!(
                f.gateway
                    .handle_from(request(Some(&secret), "198.51.100.9:1234"), None, edge)
                    .status,
                429
            );
        }
        // Port changes do not reset a client's window; another viewer has its own.
        assert_eq!(
            f.gateway
                .handle_from(request(Some(&secret), "198.51.100.9:9876"), None, edge)
                .status,
            429
        );
        assert_ne!(
            f.gateway
                .handle_from(request(Some(&secret), "2001:db8::4:4321"), None, edge)
                .status,
            429
        );
    }

    #[test]
    fn authenticated_edge_browser_writes_use_only_configured_public_origin() {
        let mut f = Fixture::new();
        let secret = "ab".repeat(32);
        f.gateway.origin = Some(super::super::origin::Origin::new(secret.clone()).unwrap());
        f.config
            .extra
            .insert("publicHost".into(), json!("cloud.layr.ws"));
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        for (origin, expected) in [
            ("https://cloud.layr.ws", 404),
            ("https://attacker.example", 403),
            ("https://localhost", 403),
        ] {
            let request = http::Request {
                method: "POST".into(),
                target: "/api/session/challenge".into(),
                headers: [
                    ("host".into(), "localhost".into()),
                    ("origin".into(), origin.into()),
                    ("content-type".into(), "application/json".into()),
                    ("x-layr-origin".into(), secret.clone()),
                    (
                        "cloudfront-viewer-address".into(),
                        "198.51.100.4:1234".into(),
                    ),
                ]
                .into(),
                body: vec![],
            };
            assert_eq!(f.gateway.handle(request, None).status, expected);
        }
    }

    #[test]
    fn site_routes_preserve_api_host_and_path_boundaries() {
        let mut f = Fixture::new();
        let root = f.path.with_extension("site");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<title>Layr</title>").unwrap();
        f.config.extra.insert("siteDir".into(), json!(root));
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        assert_eq!(f.request("GET", "/", "", b"", None).status, 200);
        assert_eq!(
            f.request("GET", "/?source=fixture", "", b"", None).status,
            200
        );
        assert_eq!(
            f.request("GET", "/%2e%2e/etc/passwd", "", b"", None).status,
            400
        );
        assert_eq!(f.request("GET", "/api/missing", "", b"", None).status, 404);
        assert_eq!(f.request("GET", "/v1/missing", "", b"", None).status, 404);
        assert_eq!(f.request("GET", "/missing.js", "", b"", None).status, 404);
        assert_eq!(f.request("POST", "/", "", b"", None).status, 405);
        let request = http::Request {
            method: "GET".into(),
            target: "/".into(),
            headers: [("host".into(), "other.example".into())].into(),
            body: vec![],
        };
        assert_eq!(f.gateway.handle(request, None).status, 421);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unlock_decoy_matches_real_age_structure_and_survives_restart() {
        use base64::engine::general_purpose::STANDARD as B64;
        fn shape(data: &[u8]) -> (Vec<String>, usize) {
            assert!(age::Decryptor::new(data).is_ok());
            let footer = data.windows(5).position(|v| v == b"\n--- ").unwrap() + 1;
            let end = footer + data[footer..].iter().position(|b| *b == b'\n').unwrap() + 1;
            let lines = std::str::from_utf8(&data[..end])
                .unwrap()
                .lines()
                .map(|line| {
                    if line.starts_with("-> ") {
                        let words: Vec<_> = line.split(' ').collect();
                        let kind = if words[1] == "X25519" {
                            "X25519"
                        } else {
                            assert!(words[1].ends_with("-grease"));
                            "GREASE"
                        };
                        format!("stanza {kind}")
                    } else if line.starts_with("age-encryption.org/") {
                        line.into()
                    } else if line.starts_with("--- ") {
                        format!("MAC {}", line.len())
                    } else {
                        assert!(line.len() <= 64);
                        base64::engine::general_purpose::STANDARD_NO_PAD
                            .decode(line)
                            .unwrap();
                        String::new()
                    }
                })
                .filter(|line| !line.is_empty())
                .collect();
            (lines, data.len() - end)
        }
        let mut f = Fixture::new();
        f.config.people = Some(super::super::config::People {
            bucket: "artifacts".into(),
            region: "eu-north-1".into(),
            prefix: "people/".into(),
        });
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        let identity = layr_store::keys::identity_from_secret(&[1; 32]).unwrap();
        let recipient = layr_store::keys::recipient_of(&identity).unwrap();
        let signing = layr_store::person::make_signing_keys(&recipient).unwrap();
        assert_eq!(
            api(
                &f,
                Some("ann@example.com"),
                "PUT",
                "/api/person",
                "",
                json!({
                    "recipient":recipient,"paper":crate::decoy::PAPER.trim(),
                    "signing_key":signing.public_key,"signing":crate::decoy::SIGNING.trim()
                })
            )
            .status,
            200
        );
        let known = f.request(
            "GET",
            "/api/person/unlock?login=ann%40example.com",
            "",
            b"",
            None,
        );
        let path = "/api/person/unlock?login=missing%40example.com";
        let missing = f.request("GET", path, "", b"", None);
        assert_eq!((known.status, missing.status), (200, 200));
        assert_eq!(known.body.len(), missing.body.len());
        let decode = |reply: &Reply| -> serde_json::Value {
            let outer: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
            serde_json::from_slice(&B64.decode(outer["unlock"].as_str().unwrap()).unwrap()).unwrap()
        };
        let k = decode(&known);
        let m = decode(&missing);
        assert_eq!(
            k["slots"].as_array().unwrap().len(),
            m["slots"].as_array().unwrap().len()
        );
        for (real, decoy) in [
            (&k["slots"][0]["data"], &m["slots"][0]["data"]),
            (&k["signing"], &m["signing"]),
        ] {
            let real = B64.decode(real.as_str().unwrap()).unwrap();
            let decoy = B64.decode(decoy.as_str().unwrap()).unwrap();
            assert_eq!(shape(&real), shape(&decoy));
            assert_ne!(real, decoy);
        }
        assert_eq!(missing.body, f.request("GET", path, "", b"", None).body);
        let mut restarted = Fixture::new();
        restarted.config.people = f.config.people.clone();
        std::fs::write(
            &restarted.path,
            serde_json::to_vec(&restarted.config).unwrap(),
        )
        .unwrap();
        restarted.gateway.person_secret = f.gateway.person_secret.clone();
        assert_eq!(
            missing.body,
            restarted.request("GET", path, "", b"", None).body
        );
    }

    #[test]
    fn public_sign_in_registration_decoys_and_session_identity() {
        use base64::engine::general_purpose::STANDARD as B64;
        let mut f = Fixture::new();
        f.config.people = Some(super::super::config::People {
            bucket: "artifacts".into(),
            region: "eu-north-1".into(),
            prefix: "people/".into(),
        });
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        let login = "ann@example.com";
        let identity = layr_store::keys::generate_identity();
        let recipient = layr_store::keys::recipient_of(&identity).unwrap();
        let keys = layr_store::person::make_signing_keys(&recipient).unwrap();
        let signing =
            layr_store::person::open_signing_key(&identity, &keys.sealed, &keys.public_key)
                .unwrap();
        let body = serde_json::to_vec(&json!({"recipient":recipient,"paper":B64.encode(b"encrypted paper slot"),"signing_key":keys.public_key,"signing":B64.encode(&keys.sealed)})).unwrap();
        let path = "/api/person?login=ann%40example.com";
        assert_eq!(f.request("PUT", path, "", &body, None).status, 401);
        let bucket = f
            .gateway
            .storage
            .people(f.config.people.as_ref().unwrap())
            .unwrap();
        let invite =
            super::super::session::invite(bucket.as_ref(), "people/", login, auth::now()).unwrap();
        assert_eq!(
            f.request(
                "PUT",
                "/api/person?login=bob%40example.com",
                &invite,
                &body,
                None
            )
            .status,
            401
        );
        assert_eq!(f.request("PUT", path, &invite, &body, None).status, 200);
        assert_eq!(f.request("PUT", path, &invite, &body, None).status, 409);
        assert_eq!(
            f.request("GET", "/api/person", &invite, b"", None).status,
            401
        );
        let known = f.request(
            "GET",
            "/api/person/unlock?login=ann%40example.com",
            "",
            b"",
            None,
        );
        let missing = f.request(
            "GET",
            "/api/person/unlock?login=missing%40example.com",
            "",
            b"",
            None,
        );
        assert_eq!(known.status, 200);
        assert_eq!(missing.status, 200);
        assert_eq!(known.body.len(), missing.body.len());
        let k: serde_json::Value = serde_json::from_slice(&known.body).unwrap();
        let m: serde_json::Value = serde_json::from_slice(&missing.body).unwrap();
        assert_eq!(
            k.as_object().unwrap().keys().collect::<Vec<_>>(),
            m.as_object().unwrap().keys().collect::<Vec<_>>()
        );
        let unlock: serde_json::Value =
            serde_json::from_slice(&B64.decode(k["unlock"].as_str().unwrap()).unwrap()).unwrap();
        assert!(unlock.get("index").is_none() && unlock.get("inbox").is_none());
        assert_eq!(unlock["signing_key"], keys.public_key);
        assert_eq!(
            missing.body,
            f.request(
                "GET",
                "/api/person/unlock?login=missing%40example.com",
                "",
                b"",
                None
            )
            .body
        );
        let challenge = value(f.request(
            "GET",
            "/api/session/challenge?login=ann%40example.com",
            "",
            b"",
            None,
        ));
        let challenge = challenge["challenge"].as_str().unwrap();
        let exchange=serde_json::to_vec(&json!({"login":login,"challenge":challenge,"signature":layr_store::session::sign(login,challenge,&signing).unwrap()})).unwrap();
        let signed = value(f.request("POST", "/api/session", "", &exchange, None));
        let token = signed["token"].as_str().unwrap();
        assert_eq!(
            f.request("GET", "/api/person", token, b"", None).status,
            200
        );
        assert_eq!(
            f.request("POST", "/api/session", "", &exchange, None)
                .status,
            401
        );
        // A session identity comes from its token, never an injected network login.
        let wrong = api(
            &f,
            Some("bob@example.com"),
            "GET",
            "/api/person",
            token,
            json!({}),
        );
        assert_eq!(value(wrong)["login"], login);
        for _ in 0..12 {
            f.request(
                "GET",
                "/api/session/challenge?login=ann%40example.com",
                "",
                b"",
                None,
            );
        }
        assert_eq!(
            f.request(
                "GET",
                "/api/person/unlock?login=ann%40example.com",
                "",
                b"",
                None
            )
            .status,
            429
        );
    }

    #[test]
    fn person_requests_and_machine_claims_are_bound_and_create_only() {
        use base64::engine::general_purpose::STANDARD as B64;
        use layr_store::{
            keys,
            request::{self, Action, Claim, Request},
        };
        let mut f = Fixture::new();
        f.config.people = Some(super::super::config::People {
            bucket: "artifacts".into(),
            region: "eu-north-1".into(),
            prefix: "people/".into(),
        });
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        let project = f.config.projects["example"].clone();
        let project_id = "01a12290-0000-7000-8000-000000000002";
        f.gateway
            .storage
            .put(
                &project,
                "store.json",
                serde_json::to_vec(&json!({
            "format":2,"project":project_id,"recipient":project.recipient}))
                .unwrap(),
            )
            .unwrap();
        let login = "ann@example.com";
        let identity = keys::generate_identity();
        let recipient = keys::recipient_of(&identity).unwrap();
        let signing = layr_store::person::make_signing_keys(&recipient).unwrap();
        let key =
            layr_store::person::open_signing_key(&identity, &signing.sealed, &signing.public_key)
                .unwrap();
        let registration = json!({"recipient":recipient,"paper":B64.encode(b"test paper slot"),
            "signing_key":signing.public_key,"signing":B64.encode(&signing.sealed)});
        assert_eq!(
            api(&f, Some(login), "PUT", "/api/person", "", registration).status,
            200
        );
        let view = value(api(&f, Some(login), "GET", "/api/person", "", json!({})));
        assert_eq!(view["signing_key"], signing.public_key);
        assert_eq!(view["signing"], B64.encode(&signing.sealed));
        let write = f.token(false);
        let gc = f.token(true);
        let read = f.read_token();
        let id = "01a12290-0000-7000-8000-000000000001";
        let base = format!("/v1/p/example/objects/requests/{id}.json");
        let claim_path = format!("/v1/p/example/objects/requests/claims/{id}.json");
        let raw = |who: Option<&str>, method: &str, path: &str, token: &str, body: Vec<u8>| {
            let person_token = who
                .filter(|_| token.is_empty())
                .map(|login| f.gateway.sessions.test_token(login));
            let token = person_token.as_deref().unwrap_or(token);
            f.gateway.handle(
                http::Request {
                    method: method.into(),
                    target: path.into(),
                    headers: [
                        ("host".into(), "localhost".into()),
                        ("content-type".into(), "application/json".into()),
                        ("content-length".into(), body.len().to_string()),
                        ("authorization".into(), format!("Bearer {token}")),
                    ]
                    .into(),
                    body,
                },
                who,
            )
        };
        let request = Request::new(
            id.into(),
            project_id.into(),
            login.into(),
            auth::now() as i64,
            Action::Comment {
                change_id: project_id.into(),
                version: project_id.into(),
                path: None,
                line: None,
                text: "private review text is visible only to project readers".into(),
                reply_to: None,
            },
        )
        .unwrap();
        let plaintext = request.sign(&key).unwrap().to_bytes().unwrap();
        assert_eq!(
            raw(Some(login), "PUT", &base, "", plaintext.clone()).status,
            400
        );
        let body = request::seal(&plaintext, &project.recipient, &key).unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("private review text"));
        let envelope = request::Sealed::parse(&body).unwrap();
        assert_eq!(
            keys::decrypt_with(&f.identity, &B64.decode(&envelope.ct).unwrap()).unwrap(),
            plaintext
        );
        assert_eq!(raw(None, "PUT", &base, &write, body.clone()).status, 403);
        assert_eq!(
            raw(Some("bob@example.com"), "PUT", &base, &write, body.clone()).status,
            403
        );
        let mut altered: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let mut ciphertext = B64.decode(altered["ct"].as_str().unwrap()).unwrap();
        *ciphertext.last_mut().unwrap() ^= 1;
        altered["ct"] = json!(B64.encode(ciphertext));
        let altered = serde_json::to_vec(&altered).unwrap();
        assert_eq!(raw(Some(login), "PUT", &base, "", altered).status, 403);
        let mut wrong = request.clone();
        wrong.project = id.into();
        assert_eq!(
            raw(
                Some(login),
                "PUT",
                &base,
                "",
                request::seal(
                    &wrong.sign(&key).unwrap().to_bytes().unwrap(),
                    &project.recipient,
                    &key
                )
                .unwrap()
            )
            .status,
            403
        );
        let other_path = base.replace(id, project_id);
        assert_eq!(
            raw(Some(login), "PUT", &other_path, "", body.clone()).status,
            403
        );
        for (field, value, status) in [
            ("person", json!("bob@example.com"), 403),
            ("project", json!(id), 403),
            ("epoch", json!(id), 400),
            ("v", json!(1), 400),
            ("sig", json!("00".repeat(64)), 403),
            ("action", json!({"text":"plaintext"}), 400),
        ] {
            let mut changed: serde_json::Value = serde_json::from_slice(&body).unwrap();
            changed[field] = value;
            assert_eq!(
                raw(
                    Some(login),
                    "PUT",
                    &base,
                    "",
                    serde_json::to_vec(&changed).unwrap()
                )
                .status,
                status,
                "{field}"
            );
        }
        assert_eq!(
            raw(
                Some(login),
                "PUT",
                &base,
                "",
                vec![b'x'; request::MAX_SEALED_BYTES + 1]
            )
            .status,
            413
        );
        assert_eq!(raw(Some(login), "PUT", &base, "", body.clone()).status, 201);
        assert_eq!(raw(Some(login), "PUT", &base, "", body.clone()).status, 412);
        assert_eq!(raw(Some(login), "GET", &base, &write, vec![]).status, 401);
        assert_eq!(raw(Some(login), "GET", &base, &read, vec![]).body, body);
        assert_eq!(raw(Some(login), "DELETE", &base, &gc, vec![]).status, 403);
        let claim = Claim {
            format: 1,
            project: project_id.into(),
            request: id.into(),
            machine: project_id.into(),
        };
        let claim_body = claim.sign(&key).unwrap().to_bytes().unwrap();
        assert_eq!(
            raw(None, "PUT", &claim_path, &read, claim_body.clone()).status,
            403
        );
        assert_eq!(
            raw(None, "PUT", &claim_path, "", claim_body.clone()).status,
            403
        );
        let mut wrong = claim.clone();
        wrong.project = id.into();
        assert_eq!(
            raw(
                None,
                "PUT",
                &claim_path,
                &write,
                wrong.sign(&key).unwrap().to_bytes().unwrap()
            )
            .status,
            400
        );
        assert_eq!(
            raw(
                None,
                "PUT",
                &claim_path.replace(id, project_id),
                &write,
                claim_body.clone()
            )
            .status,
            400
        );
        // The gateway checks the writer bearer; encrypted canonical machine trust stays at the executor.
        assert_eq!(
            raw(None, "PUT", &claim_path, &write, claim_body.clone()).status,
            201
        );
        assert_eq!(
            raw(None, "PUT", &claim_path, &write, claim_body).status,
            412
        );
        assert_eq!(raw(None, "DELETE", &claim_path, &gc, vec![]).status, 403);
        for suffix in ["/extra", ".extra", "\n"] {
            assert_eq!(
                raw(
                    Some(login),
                    "PUT",
                    &format!("{base}{suffix}"),
                    &write,
                    body.clone()
                )
                .status,
                400
            );
        }
        // The same envelope is accepted only in its exact published epoch namespace.
        let next = keys::generate_identity();
        let next_recipient = keys::recipient_of(&next).unwrap();
        let mut scoped = project.clone();
        scoped.prefix = format!("{}/project/epochs/{EPOCH}", project.prefix);
        scoped.recipient = next_recipient.clone();
        f.gateway
            .storage
            .put(
                &scoped,
                "store.json",
                serde_json::to_vec(
                    &json!({"format":2,"project":project_id,"recipient":next_recipient}),
                )
                .unwrap(),
            )
            .unwrap();
        f.gateway
            .storage
            .put(
                &scoped,
                "slots/person-ann@example.com-0123456789abcdef.age",
                b"own sealed slot".to_vec(),
            )
            .unwrap();
        let epoch_key = layr_store::epochs::Key {
            project: project_id.into(),
            parent: "root".into(),
            parent_recipient: project.recipient.clone(),
            epoch: EPOCH.into(),
            recipient: next_recipient.clone(),
            mode: layr_store::epochs::Mode::Private,
            readers_hash: "0".repeat(64),
        };
        let record = json!({"id":id,"project":project_id,"op":"project.key","data":epoch_key});
        let record = serde_json::to_vec(&record).unwrap();
        let publication = format!(
            "{{\"r\":{},\"s\":\"{}\"}}",
            String::from_utf8(record.clone()).unwrap(),
            keys::sign_bytes(&record, &key)
        );
        f.gateway
            .storage
            .put(
                &project,
                &format!("keys/{EPOCH}-{id}.json"),
                publication.into_bytes(),
            )
            .unwrap();
        let epoch_path = format!("/v1/p/example/objects/project/epochs/{EPOCH}/requests/{id}.json");
        let epoch_body = request::seal_for_epoch(&plaintext, EPOCH, &next_recipient, &key).unwrap();
        assert_eq!(
            raw(Some(login), "PUT", &epoch_path, "", body.clone()).status,
            400
        );
        assert_eq!(
            raw(Some(login), "PUT", &base, "", epoch_body.clone()).status,
            400
        );
        assert_eq!(
            raw(Some(login), "PUT", &epoch_path, "", epoch_body.clone()).status,
            201
        );
        assert_eq!(
            raw(Some(login), "PUT", &epoch_path, "", epoch_body.clone()).status,
            412
        );
        let challenge = value(f.request(
            "GET",
            &format!("/v1/p/example/challenge?epoch={EPOCH}"),
            &write,
            b"",
            None,
        ));
        let clear = keys::decrypt(
            &next,
            &B64.decode(challenge["challenge"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        let proof = json!({"challenge":String::from_utf8(clear).unwrap()}).to_string();
        let epoch_read = value(f.request(
            "POST",
            &format!("/v1/p/example/token?epoch={EPOCH}"),
            &write,
            proof.as_bytes(),
            None,
        ));
        let epoch_read = epoch_read["token"].as_str().unwrap();
        assert_eq!(
            raw(None, "GET", &epoch_path, epoch_read, vec![]).body,
            epoch_body
        );
        assert_eq!(raw(None, "GET", &epoch_path, &read, vec![]).status, 404);
        let epoch_claim = format!("/v1/p/example/objects/project/epochs/{EPOCH}/claims/{id}.json");
        let claim_body = claim.sign(&key).unwrap().to_bytes().unwrap();
        assert_eq!(
            raw(None, "PUT", &epoch_claim, &write, claim_body.clone()).status,
            201
        );
        assert_eq!(
            raw(None, "PUT", &epoch_claim, &write, claim_body.clone()).status,
            412
        );
        assert_eq!(
            raw(None, "GET", &epoch_claim, epoch_read, vec![]).body,
            claim_body
        );
        assert_eq!(
            raw(
                None,
                "PUT",
                &format!("/v1/p/example/objects/project/epochs/{EPOCH}/requests/claims/{id}.json"),
                &write,
                claim_body
            )
            .status,
            400
        );
        assert_eq!(
            raw(
                None,
                "GET",
                &format!("/v1/p/example/objects/claims/{id}.json"),
                epoch_read,
                vec![]
            )
            .status,
            400
        );
    }

    #[test]
    fn hidden_manifest_requires_exact_opaque_path_and_bound_read_proof() {
        let mut f = Fixture::new();
        let write = f.token(false);
        let read = f.read_token();
        let gc = f.token(true);
        let key = format!("overlays/{OVERLAY}/keys/{EPOCH}-{OVERLAY}.age");
        let path = format!("/v1/p/example/objects/{key}");
        assert!(object_name(&key));
        assert!(
            !overlay_key(&key),
            "a hidden manifest is not a public key record"
        );
        assert_eq!(
            f.request("PUT", &path, &write, b"encrypted manifest", None)
                .status,
            201
        );
        for token in ["", &write, &gc] {
            assert_eq!(f.request("GET", &path, token, b"", None).status, 401);
        }
        let response = f.request("GET", &path, &read, b"", None);
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"encrypted manifest");
        let foreign = Fixture::new().read_token();
        assert_eq!(f.request("GET", &path, &foreign, b"", None).status, 401);
        assert_eq!(f.request("DELETE", &path, &gc, b"", None).status, 403);
        for invalid in [
            key.replace("/keys/", "/../keys/"),
            key.replace("/keys/", "/keyx/"),
            key.replace(".age", ".agx"),
            key.replace(".age", ".age/"),
            key.replace(".age", ".age\n"),
            key.replacen("01a12290", "g1a12290", 1),
        ] {
            assert!(!object_name(&invalid), "{invalid:?}");
            assert_eq!(
                f.request(
                    "GET",
                    &format!("/v1/p/example/objects/{invalid}"),
                    &read,
                    b"",
                    None
                )
                .status,
                400
            );
        }
    }

    #[test]
    fn overlay_write_grant_reads_only_public_keys_and_creates_immutable_objects() {
        let mut f = Fixture::new();
        let write = f.token(false);
        let gc = f.token(true);
        let read = f.read_token();
        let key = format!("overlays/{OVERLAY}/keys/{EPOCH}-{OVERLAY}.json");
        let epoch = format!("overlays/{OVERLAY}/epochs/{EPOCH}");
        for object in [
            key.clone(),
            format!("{epoch}/records/machine/1-1.age"),
            format!("{epoch}/states/state/12345678.tree.age"),
            format!("{epoch}/states/state/12345678.0000.data.age"),
            format!("{epoch}/slots/machine-{OVERLAY}.age"),
            format!("{epoch}/slots/person-ann%40example.com-0123456789abcdef.age"),
            format!("{epoch}/slots/paper.age"),
        ] {
            let path = format!("/v1/p/example/objects/{object}");
            for token in ["", &read, &gc] {
                assert_eq!(
                    f.request("PUT", &path, token, b"first", None).status,
                    403,
                    "{object}"
                );
            }
            assert_eq!(
                f.request("PUT", &path, &write, b"first", None).status,
                201,
                "{object}"
            );
            assert_eq!(f.request("PUT", &path, &write, b"second", None).status, 412);
            assert_eq!(f.request("GET", &path, &read, b"", None).body, b"first");
            assert_eq!(
                f.request("GET", &path, &write, b"", None).status,
                if object == key { 200 } else { 401 }
            );
            for token in ["", &gc] {
                assert_eq!(f.request("GET", &path, token, b"", None).status, 401);
            }
            assert_eq!(f.request("DELETE", &path, &gc, b"", None).status, 403);
        }
        let prefix = format!("overlays/{OVERLAY}/keys/");
        let path = format!("/v1/p/example/objects?prefix={prefix}");
        let listed = value(f.request("GET", &path, &write, b"", None));
        assert_eq!(listed["objects"].as_array().unwrap().len(), 1);
        assert_eq!(listed["objects"][0]["key"], key);
        for prefix in [
            "overlays/".into(),
            format!("overlays/{OVERLAY}/"),
            format!("overlays/{OVERLAY}/keys"),
            format!("{epoch}/"),
            format!("{epoch}/records/"),
            format!("{epoch}/states/"),
            format!("{epoch}/slots/"),
        ] {
            let path = format!("/v1/p/example/objects?prefix={prefix}");
            assert_eq!(
                f.request("GET", &path, &write, b"", None).status,
                401,
                "{prefix}"
            );
            assert_eq!(
                f.request("GET", &path, &read, b"", None).status,
                200,
                "{prefix}"
            );
        }
    }

    #[test]
    fn overlay_paths_require_canonical_ids_and_single_separators() {
        let mut f = Fixture::new();
        let write = f.token(false);
        for name in [
            "overlays/../keys/x.json".into(),
            "overlays/not-a-uuid/keys/x.json".into(),
            format!("overlays/{}/keys/x.json", OVERLAY.to_uppercase()),
            format!("overlays/{OVERLAY}//keys/x.json"),
            format!("overlays/{OVERLAY}/keys//x.json"),
            format!("overlays/{OVERLAY}/keys/nested/x.json"),
            format!("overlays/{OVERLAY}/keys/x.age"),
            format!("overlays/{OVERLAY}/epochs/../states/x/a.age"),
            format!("overlays/{OVERLAY}/epochs/not-a-uuid/states/x/a.age"),
            format!("overlays/{OVERLAY}/epochs/{EPOCH}//states/x/a.age"),
            format!("overlays/{OVERLAY}/epochs/{EPOCH}/other/x.age"),
            format!("overlays/{OVERLAY}/epochs/{EPOCH}/states/../a.age"),
            format!("overlays/{OVERLAY}/epochs/{EPOCH}/states/x/a.age/"),
            format!("overlays/{OVERLAY}/epochs/{EPOCH}/states/x%2fy/a.age"),
        ] {
            assert!(!object_name(&name), "{name}");
            let path = format!("/v1/p/example/objects/{name}");
            assert_eq!(
                f.request("PUT", &path, &write, b"x", None).status,
                400,
                "{name}"
            );
        }
        for prefix in [
            format!("overlays/{OVERLAY}/keys//"),
            format!("overlays/{OVERLAY}/epochs/{EPOCH}//states/"),
            format!("overlays/{OVERLAY}/epochs/%2e%2e/states/"),
            "overlays/not-a-uuid/keys/".into(),
            format!("overlays/{OVERLAY}/keys/%2f"),
        ] {
            assert_eq!(
                f.request(
                    "GET",
                    &format!("/v1/p/example/objects?prefix={prefix}"),
                    &write,
                    b"",
                    None
                )
                .status,
                400,
                "{prefix}"
            );
        }
    }

    #[test]
    fn grants_are_separate_and_put_is_immutable() {
        let mut f = Fixture::new();
        let write = f.token(false);
        let gc = f.token(true);
        let read = f.read_token();
        let path = "/v1/p/example/objects/records/machine/1-1.age";
        assert_eq!(f.request("PUT", path, &read, b"first", None).status, 403);
        assert_eq!(f.request("PUT", path, &gc, b"first", None).status, 403);
        assert_eq!(f.request("PUT", path, &write, b"first", None).status, 201);
        assert_eq!(f.request("PUT", path, &write, b"second", None).status, 412);
        assert_eq!(f.request("GET", path, &write, b"", None).status, 401);
        for prefix in ["records/", "records/machine/", "states/", "states/state/"] {
            assert_eq!(
                f.request(
                    "GET",
                    &format!("/v1/p/example/objects?prefix={prefix}"),
                    &write,
                    b"",
                    None
                )
                .status,
                401
            );
        }
        for prefix in ["", "slots/", "records", "states"] {
            assert_ne!(
                f.request(
                    "GET",
                    &format!("/v1/p/example/objects?prefix={prefix}"),
                    &write,
                    b"",
                    None
                )
                .status,
                200
            );
        }

        assert_eq!(f.request("GET", path, &read, b"", None).body, b"first");
        let slot = "/v1/p/example/objects/slots/machine-id.age";
        assert_eq!(f.request("PUT", slot, &write, b"slot", None).status, 403);
        assert_eq!(f.request("PUT", slot, &read, b"slot", None).status, 201);
        assert_eq!(
            f.request(
                "PUT",
                "/v1/p/example/objects/store.json",
                &write,
                b"{}",
                None
            )
            .status,
            400
        );
        assert_eq!(f.request("DELETE", path, &gc, b"", None).status, 403);
        let data = "/v1/p/example/objects/states/state/attempt.0001.data.age";
        assert_eq!(f.request("PUT", data, &write, b"data", None).status, 201);
        assert_eq!(f.request("DELETE", data, &write, b"", None).status, 403);
        assert_eq!(f.request("DELETE", data, &gc, b"", None).status, 204);
        assert_eq!(f.request("GET", data, &read, b"", None).status, 404);
    }
    #[test]
    fn encoded_paths_ranges_and_config_changes_do_not_bypass_grants() {
        let mut f = Fixture::new();
        let write = f.token(false);
        let read = f.read_token();
        for name in [
            "records/%2e%2e/secret",
            "records/a%2fb/secret",
            "slots/%5csecret",
            "slots/%00",
            "states//x",
            "states/%xx",
        ] {
            assert_eq!(
                f.request(
                    "PUT",
                    &format!("/v1/p/example/objects/{name}"),
                    &write,
                    b"x",
                    None
                )
                .status,
                400,
                "{name}"
            );
        }
        let path = "/v1/p/example/objects/states/state/attempt.0001.data.age";
        assert_eq!(f.request("PUT", path, &write, b"abcdef", None).status, 201);
        let reply = f.request("GET", path, &read, b"", Some("bytes=1-3"));
        assert_eq!(reply.status, 206);
        assert_eq!(reply.body, b"bcd");
        assert!(reply
            .headers
            .contains(&("Content-Range".into(), "bytes 1-3/6".into())));
        assert_eq!(
            f.request("GET", path, &read, b"", Some("bytes=9-")).status,
            416
        );
        assert_eq!(
            f.request("GET", path, &read, b"", Some("bytes=0-1,3-4"))
                .status,
            416
        );
        f.config
            .projects
            .get_mut("example")
            .unwrap()
            .write_token_hashes
            .clear();
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        assert_eq!(f.request("PUT", path, &write, b"new", None).status, 403);
        f.config.projects.get_mut("example").unwrap().recipient =
            age::x25519::Identity::generate().to_public().to_string();
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        assert_eq!(f.request("GET", path, &read, b"", None).status, 401);
    }
    fn api(
        f: &Fixture,
        login: Option<&str>,
        method: &str,
        path: &str,
        token: &str,
        body: serde_json::Value,
    ) -> Reply {
        let mut path = path.to_string();
        let mut token = token.to_string();
        if let Some(login) = login {
            if method == "PUT" && path == "/api/person" {
                let people_config = f.config.people.as_ref().unwrap();
                let bucket = f.gateway.storage.people(people_config).unwrap();
                token = super::super::session::invite(
                    bucket.as_ref(),
                    &people_config.prefix,
                    login,
                    auth::now(),
                )
                .unwrap();
                path.push_str(&format!(
                    "?login={}",
                    layr_store::aws::uri_encode(login, false)
                ));
            } else if token.is_empty() && (method == "GET" || path.starts_with("/api/people/")) {
                token = f.gateway.sessions.test_token(login);
            }
        }
        f.gateway.handle(
            http::Request {
                method: method.into(),
                target: path.clone(),
                body: if method == "GET" || method == "DELETE" {
                    Vec::new()
                } else {
                    serde_json::to_vec(&body).unwrap()
                },
                headers: [
                    ("host".into(), "localhost".into()),
                    (
                        "content-type".into(),
                        if path.contains("/objects/") {
                            "application/octet-stream".into()
                        } else {
                            "application/json".into()
                        },
                    ),
                    ("authorization".into(), format!("Bearer {token}")),
                ]
                .into(),
            },
            login,
        )
    }
    fn value(reply: Reply) -> serde_json::Value {
        assert_eq!(
            reply.status,
            200,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        serde_json::from_slice(&reply.body).unwrap()
    }
    #[test]
    fn person_api_registration_proofs_index_slots_and_invites_match_desk() {
        use base64::engine::general_purpose::STANDARD as B64;
        use layr_store::{keys, person};
        use sha2::{Digest, Sha256};
        let mut f = Fixture::new();
        f.config.people = Some(super::super::config::People {
            bucket: "artifacts".into(),
            region: "eu-central-1".into(),
            prefix: "people/".into(),
        });
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        let ann = Some("ann@example.com");
        let bob = Some("bob@example.com");
        let call =
            |who: Option<&str>, method: &str, path: &str, token: &str, body: serde_json::Value| {
                api(&f, who, method, path, token, body)
            };
        assert_eq!(call(None, "GET", "/api/person", "", json!({})).status, 401);
        assert_eq!(
            value(call(ann, "GET", "/api/person", "", json!({})))["recipient"],
            serde_json::Value::Null
        );
        let identity = keys::generate_identity();
        let recipient = keys::recipient_of(&identity).unwrap();
        let paper = keys::generate_identity();
        let paper_slot = keys::seal_slot(&identity, &keys::recipient_of(&paper).unwrap()).unwrap();
        let signing = person::make_signing_keys(&recipient).unwrap();
        let body = json!({"recipient":recipient,"paper":B64.encode(&paper_slot),"signing_key":signing.public_key,"signing":B64.encode(signing.sealed)});
        assert_eq!(
            value(call(ann, "PUT", "/api/person", "", body.clone()))["new"],
            true
        );
        assert_eq!(
            call(ann, "PUT", "/api/person", "", body.clone()).status,
            409
        );
        let mut retry = body.clone();
        retry["paper"] = json!(B64.encode(b"other slot"));
        assert_eq!(call(ann, "PUT", "/api/person", "", retry).status, 409);
        let record = value(call(ann, "GET", "/api/person", "", json!({})));
        assert_eq!(record["slots"][0]["data"], body["paper"]);
        let other = keys::recipient_of(&keys::generate_identity()).unwrap();
        assert_eq!(
            call(
                ann,
                "PUT",
                "/api/person",
                "",
                json!({"recipient":other,"paper":B64.encode(b"slot")})
            )
            .status,
            409
        );
        let token = f.gateway.sessions.test_token(ann.unwrap());
        let index = person::seal_index(&identity, &person::Index::default()).unwrap();
        let data = json!({"data":B64.encode(&index),"previous":null});
        assert_eq!(
            call(ann, "PUT", "/api/person/index", "", data.clone()).status,
            401
        );
        assert_eq!(
            call(bob, "PUT", "/api/person/index", "invalid", data.clone()).status,
            401
        );
        assert_eq!(
            call(ann, "PUT", "/api/person/index", &token, data.clone()).status,
            200
        );
        assert_eq!(
            call(ann, "PUT", "/api/person/index", &token, data.clone()).status,
            409
        );
        let mut data = data;
        data["previous"] = json!(hex::encode(Sha256::digest(&index)));
        assert_eq!(
            call(ann, "PUT", "/api/person/index", &token, data).status,
            200
        );
        assert_eq!(
            call(
                ann,
                "PUT",
                "/api/person/index",
                &token,
                json!({"data":B64.encode(vec![0;49153])})
            )
            .status,
            400
        );
        for slot in [
            "paper",
            "person-ann@example.com-0123456789abcdef",
            "passkey-../x",
            "passkey-",
        ] {
            assert_eq!(
                call(
                    ann,
                    "PUT",
                    &format!("/api/person/slots/{slot}"),
                    &token,
                    json!({"data":B64.encode(b"slot")})
                )
                .status,
                if slot.contains('/') { 404 } else { 400 }
            );
        }
        let path = "/api/person/slots/passkey-abc_123";
        assert_eq!(
            call(
                ann,
                "PUT",
                path,
                &token,
                json!({"data":B64.encode(b"slot")})
            )
            .status,
            200
        );
        assert_eq!(
            call(
                ann,
                "PUT",
                path,
                &token,
                json!({"data":B64.encode(b"slot")})
            )
            .status,
            409
        );
        assert_eq!(call(ann, "DELETE", path, &token, json!({})).status, 200);
        assert_eq!(
            call(
                bob,
                "POST",
                "/api/people/nobody@example.com/inbox",
                "",
                json!({"data":B64.encode(b"invite")})
            )
            .status,
            404
        );
        assert_eq!(
            call(
                None,
                "POST",
                "/api/people/ann@example.com/inbox",
                "",
                json!({"data":B64.encode(b"invite")})
            )
            .status,
            401
        );
        let invite = value(call(
            bob,
            "POST",
            "/api/people/ann%40example.com/inbox",
            "",
            json!({"data":B64.encode(b"invite")}),
        ));
        let id = invite["id"].as_str().unwrap();
        let record = value(call(ann, "GET", "/api/person", "", json!({})));
        assert_eq!(record["inbox"][0]["from"], "bob@example.com");
        assert_eq!(
            call(
                ann,
                "DELETE",
                &format!("/api/person/inbox/{id}"),
                &token,
                json!({})
            )
            .status,
            200
        );
        for _ in 0..64 {
            assert_eq!(
                call(
                    bob,
                    "POST",
                    "/api/people/ann@example.com/inbox",
                    "",
                    json!({"data":B64.encode(b"invite")})
                )
                .status,
                200
            );
        }
        assert_eq!(
            call(
                bob,
                "POST",
                "/api/people/ann@example.com/inbox",
                "",
                json!({"data":B64.encode(b"invite")})
            )
            .status,
            429
        );
        assert_eq!(
            call(
                bob,
                "POST",
                "/api/people/ann@example.com/inbox",
                "",
                json!({"data":B64.encode(vec![0;4097])})
            )
            .status,
            400
        );
    }
    #[test]
    fn machine_grants_bootstrap_only_their_private_project_without_listing() {
        let mut f = Fixture::new();
        let read = f.read_token();
        let write = f.token(false);
        let gc = f.token(true);
        let other = auth::issue(false, 60_000, auth::now()).unwrap();
        let other_gc = auth::issue(true, 60_000, auth::now()).unwrap();
        let expired = auth::issue(false, 1, auth::now() - 10_000).unwrap();
        f.config
            .projects
            .get_mut("example")
            .unwrap()
            .write_token_hashes
            .push(auth::hash(&expired));
        let mut q = f.config.projects["example"].clone();
        q.prefix = "other".into();
        q.write_token_hashes = vec![auth::hash(&other)];
        q.gc_token_hashes = vec![auth::hash(&other_gc)];
        f.config.projects.insert("other".into(), q);
        std::fs::write(&f.path, serde_json::to_vec(&f.config).unwrap()).unwrap();
        let machine = "machine-01a12290-0000-7000-8000-000000000001.age";
        for project in f.config.projects.values() {
            f.gateway
                .storage
                .put(
                    project,
                    "slots/person-ann@example.com-0123456789abcdef.age",
                    b"person-slot".to_vec(),
                )
                .unwrap();
            f.gateway
                .storage
                .put(project, "store.json", b"descriptor".to_vec())
                .unwrap();
            f.gateway
                .storage
                .put(project, "slots/paper.age", b"encrypted-paper-slot".to_vec())
                .unwrap();
            f.gateway
                .storage
                .put(
                    project,
                    &format!("slots/{machine}"),
                    b"encrypted-machine-slot".to_vec(),
                )
                .unwrap();
        }
        for token in [&write, &gc, &read] {
            assert_eq!(
                f.request("GET", "/v1/p/example/store.json", token, b"", None)
                    .status,
                200
            );
            assert_eq!(
                f.request(
                    "GET",
                    &format!("/v1/p/example/slots/{machine}"),
                    token,
                    b"",
                    None
                )
                .body,
                b"encrypted-machine-slot"
            );
            assert_eq!(
                f.request("GET", "/v1/p/example/challenge", token, b"", None)
                    .status,
                200
            );
        }
        let challenge = value(f.request("GET", "/v1/p/example/challenge", &gc, b"", None));
        let sealed = base64::engine::general_purpose::STANDARD
            .decode(challenge["challenge"].as_str().unwrap())
            .unwrap();
        let proof = String::from_utf8(age::decrypt(&f.identity, &sealed).unwrap()).unwrap();
        assert_eq!(
            f.request(
                "POST",
                "/v1/p/example/token",
                &gc,
                json!({"challenge":proof}).to_string().as_bytes(),
                None
            )
            .status,
            200
        );
        assert_eq!(
            f.request("GET", "/v1/p/example/slots", &write, b"", None)
                .status,
            404
        );
        assert_eq!(
            f.request("GET", "/v1/p/example/slots", &gc, b"", None)
                .status,
            404
        );
        let paper = "/v1/p/example/objects/slots/paper.age";
        let reply = f.request("GET", paper, &read, b"", None);
        assert_eq!(reply.status, 200);
        assert_eq!(reply.body, b"encrypted-paper-slot");
        for token in ["", "invalid", &write, &gc, &other, &other_gc, &expired] {
            let reply = f.request("GET", paper, token, b"", None);
            let absent = f.request(
                "GET",
                "/v1/p/nonexistent/objects/slots/paper.age",
                token,
                b"",
                None,
            );
            assert_eq!(reply.status, 404);
            assert_eq!(reply.body, absent.body);
            assert_eq!(reply.headers, absent.headers);
        }
        assert_eq!(
            f.request(
                "GET",
                "/v1/p/example/objects?prefix=records/",
                &write,
                b"",
                None
            )
            .status,
            404
        );
        assert_eq!(
            f.request(
                "GET",
                "/v1/p/example/slots/person-ann%40example.com-0123456789abcdef.age",
                &write,
                b"",
                None
            )
            .status,
            404
        );
        assert_eq!(
            value(f.request("GET", "/api/projects", &write, b"", None))["projects"],
            json!([])
        );
        let object = "/v1/p/example/objects/states/state/attempt.0000.data.age";
        assert_eq!(
            f.request("PUT", object, &write, b"ciphertext", None).status,
            201
        );
        assert_eq!(f.request("GET", object, &write, b"", None).status, 404);
        assert_eq!(f.request("GET", object, &gc, b"", None).status, 404);
        assert_eq!(f.request("DELETE", object, &write, b"", None).status, 404);
        assert_eq!(
            f.request(
                "GET",
                "/v1/p/example/objects?prefix=records/",
                &gc,
                b"",
                None
            )
            .status,
            404
        );
        assert_eq!(
            f.request("GET", object, &read, b"", None).body,
            b"ciphertext"
        );
        for token in [
            "",
            "invalid",
            other.as_str(),
            other_gc.as_str(),
            expired.as_str(),
        ] {
            for action in [
                "store.json",
                "challenge",
                "token",
                &format!("slots/{machine}"),
            ] {
                let method = if action == "token" { "POST" } else { "GET" };
                let hidden = f.request(
                    method,
                    &format!("/v1/p/example/{action}"),
                    token,
                    b"{}",
                    None,
                );
                let unknown = f.request(
                    method,
                    &format!("/v1/p/nonexistent/{action}"),
                    token,
                    b"{}",
                    None,
                );
                assert_eq!(hidden.status, 404);
                assert_eq!(hidden.body, unknown.body);
                assert_eq!(hidden.headers, unknown.headers);
            }
        }
    }

    #[test]
    fn hidden_and_unknown_store_routes_have_the_same_404() {
        let f = Fixture::new();
        let read = f.read_token();
        let write = auth::issue(false, 60_000, auth::now()).unwrap(); // Unrecognized grant remains hidden.
        let path = "/v1/p/example/objects/slots/person-ann%40example.com-0123456789abcdef.age";
        assert_eq!(
            f.request("PUT", path, &read, b"person-slot", None).status,
            201
        );
        for action in [
            "store.json",
            "challenge",
            "slots",
            "token",
            "objects",
            "objects/states/nope",
            "objects/overlays/01a12290-0000-7000-8000-000000000001/keys/x.json",
            "objects/overlays/01a12290-0000-7000-8000-000000000001/epochs/01a12290-0000-7000-8000-000000000002/slots/paper.age",
            "objects?prefix=overlays/01a12290-0000-7000-8000-000000000001/keys/",
            "unknown",
            "unknown/a/b",
            "slots?foo=x",
        ] {
            for method in ["GET", "PUT", "POST", "DELETE", "OPTIONS"] {
                for who in [None, Some("bob@example.com")] {
                    let hidden = api(
                        &f,
                        who,
                        method,
                        &format!("/v1/p/example/{action}"),
                        &write,
                        json!({}),
                    );
                    let unknown = api(
                        &f,
                        who,
                        method,
                        &format!("/v1/p/unknown/{action}"),
                        &write,
                        json!({}),
                    );
                    assert_eq!(hidden.status, 404, "{method} {action}");
                    assert_eq!(unknown.status, 404);
                    assert_eq!(hidden.body, unknown.body);
                }
            }
        }
        assert_eq!(
            value(api(
                &f,
                Some("ann@example.com"),
                "GET",
                "/api/projects",
                "",
                json!({})
            ))["projects"],
            json!(["example"])
        );
        assert_eq!(
            value(api(&f, None, "GET", "/api/projects", "", json!({})))["projects"],
            json!([])
        );
        let slots = value(api(
            &f,
            Some("ann@example.com"),
            "GET",
            "/v1/p/example/slots",
            "",
            json!({}),
        ));
        assert_eq!(slots["slots"].as_array().unwrap().len(), 1);
        assert_eq!(
            slots["slots"][0]["name"],
            "person-ann@example.com-0123456789abcdef"
        );
        assert_eq!(
            api(
                &f,
                None,
                "GET",
                "/v1/p/example/objects/states/nope",
                &read,
                json!({})
            )
            .status,
            404
        );
        assert_eq!(
            api(&f, None, "GET", "/v1/p/example/objects", &read, json!({})).status,
            200
        );
    }
    #[test]
    fn writes_from_other_pages_and_non_json_person_writes_are_refused() {
        let f = Fixture::new();
        for method in ["PUT", "POST", "DELETE"] {
            for path in [
                "/api/person",
                "/v1/p/example/objects/states/x",
                "/v1/p/example/objects/overlays/01a12290-0000-7000-8000-000000000001/keys/x.json",
                "/v1/p/example/objects/overlays/01a12290-0000-7000-8000-000000000001/epochs/01a12290-0000-7000-8000-000000000002/states/x/a.age",
                "/v1/p/example/token",
            ] {
                for (name, value) in [
                    ("origin", "https://other.example"),
                    ("origin", "null"),
                    ("sec-fetch-site", "cross-site"),
                    ("sec-fetch-site", "same-site"),
                ] {
                    let request = http::Request {
                        method: method.into(),
                        target: path.into(),
                        body: Vec::new(),
                        headers: [
                            ("host".into(), "localhost".into()),
                            (
                                "content-type".into(),
                                if path.contains("/objects/") {
                                    "application/octet-stream".into()
                                } else {
                                    "application/json".into()
                                },
                            ),
                            (name.into(), value.into()),
                        ]
                        .into(),
                    };
                    assert_eq!(
                        f.gateway.handle(request, Some("ann@example.com")).status,
                        403
                    );
                }
            }
        }
        for media in ["text/plain", "application/x-www-form-urlencoded", ""] {
            let request = http::Request {
                method: "PUT".into(),
                target: "/api/person".into(),
                body: Vec::new(),
                headers: [
                    ("host".into(), "localhost".into()),
                    ("content-type".into(), media.into()),
                ]
                .into(),
            };
            assert_eq!(
                f.gateway.handle(request, Some("ann@example.com")).status,
                415
            );
        }
    }
}
