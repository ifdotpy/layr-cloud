//! Anonymous reads of deliberately published project objects.
use crate::{
    config::Config,
    server::{Gateway, Reply, Role, JSON_LIMIT},
};
use anyhow::{bail, Result};
use layr_store::{format, http};
use serde_json::json;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);
const REQUESTS: u32 = 600;
const AUTHENTICATED_REQUESTS: u32 = 3000;
const CLIENTS: usize = 4096;

#[derive(Eq, PartialEq, Hash)]
enum Client {
    Address(IpAddr),
    Credential(String),
}

#[derive(Default)]
pub struct Limiter(Mutex<HashMap<Client, (Instant, u32)>>);

impl Limiter {
    pub fn allow(&self, peer: IpAddr, now: Instant) -> bool {
        self.allow_count(peer, now, REQUESTS)
    }
    pub fn allow_count(&self, peer: IpAddr, now: Instant, requests: u32) -> bool {
        self.allow_client(Client::Address(peer), now, requests)
    }
    pub fn allow_credential(&self, token: &str, now: Instant) -> bool {
        self.allow_client(Client::Credential(crate::auth::hash(token)), now, AUTHENTICATED_REQUESTS)
    }
    fn allow_client(&self, peer: Client, now: Instant, requests: u32) -> bool {
        let Ok(mut clients) = self.0.lock() else {
            return false;
        };
        if !clients.contains_key(&peer) && clients.len() >= CLIENTS {
            clients.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
            if clients.len() >= CLIENTS {
                return false;
            }
        }
        let (start, count) = clients.entry(peer).or_insert((now, 0));
        if now.duration_since(*start) >= WINDOW {
            *start = now;
            *count = 0;
        }
        if *count >= requests {
            return false;
        }
        *count += 1;
        true
    }
}

pub fn object(key: &str) -> bool {
    if let Some((_, inner)) = crate::epochs::split(key) {
        return !inner.starts_with("project/") && object(inner);
    }
    crate::epochs::discovery(key)
        || key
            .strip_prefix("history/")
            .and_then(|s| s.strip_suffix(".age"))
            .is_some_and(|epoch| epoch == "root" || format::is_uuid(epoch))
        || matches!(key, "store.json" | "public.json" | "slots/public.age")
        || format::parse_segment_name(key).is_some()
        || format::parse_tree_name(key).is_some()
        || format::parse_data_name(key).is_some()
        || format::release_file(key)
}

fn prefix(value: &str) -> bool {
    if let Some((_, inner)) = crate::epochs::split(value) {
        return !inner.starts_with("project/") && prefix(inner) && inner != "keys/";
    }
    if value == "keys/" {
        return true;
    }
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        let id = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
        regex::Regex::new(&format!(
            r"\A(?:records|states)/(?:{id}(?:/[0-9a-f.-]*)?)?\z"
        ))
        .unwrap()
    });
    matches!(value, "files" | "files/")
        || format::release_file(value)
        || (value.len() <= 1024 && !value.contains("..") && PATTERN.is_match(value))
}

impl Gateway {
    pub fn public_read(
        &self,
        request: &http::Request,
        config: &Config,
        parts: &[String],
        query: &str,
        _peer: IpAddr,
    ) -> Result<Reply> {
        if request.method != "GET" {
            return Ok(Reply::error(405, "public routes are read only"));
        }
        if !request.body.is_empty() {
            return Ok(Reply::error(400, "GET has no body"));
        }
        if parts.len() < 3 || parts.iter().any(|p| !super::server::safe_part(p)) {
            return Ok(Reply::error(404, "no such public object"));
        }
        let Some(project) = config.projects.get(&parts[1]) else {
            return Ok(Reply::error(404, "no such public project"));
        };
        let key = parts[2..].join("/");
        let listing = key == "objects";
        let selected = if listing {
            match query
                .strip_prefix("prefix=")
                .and_then(super::server::decode)
            {
                Some(value) if !query.contains('&') && prefix(&value) => value,
                _ => return Ok(Reply::error(400, "invalid public object prefix")),
            }
        } else {
            if !query.is_empty() || !object(&key) {
                return Ok(Reply::error(404, "no such public object"));
            }
            key.clone()
        };
        let chain = match crate::epochs::Chain::load(self, project) {
            Ok(chain) => chain,
            Err(_) => return Ok(Reply::error(404, "no such public project")),
        };
        if !chain.public(self)? {
            return Ok(Reply::error(404, "no such public project"));
        }
        // The latest published epoch controls openness. An old public opener cannot
        // authorize a later private epoch, including metadata and staged namespaces.
        if crate::epochs::split(&selected).is_some_and(|(epoch, _)| chain.project(epoch).is_none())
        {
            return Ok(Reply::error(404, "no such public object"));
        }
        // The native/browser reader additionally verifies the signer against authenticated
        // project history. The gateway does not keep a parallel public machine trust registry.
        if listing {
            let entries = self.storage.list(project, &selected, Role::Reader)?;
            if entries.len() > 10_000 {
                bail!("too many public objects")
            }
            let objects: Vec<_> = entries
                .into_iter()
                .filter(|e| e.key.starts_with(&selected) && object(&e.key))
                .filter(|e| {
                    crate::epochs::split(&e.key)
                        .is_none_or(|(epoch, _)| chain.project(epoch).is_some())
                })
                .map(|e| json!({"key":e.key,"modified":e.modified}))
                .collect();
            return Ok(Reply::json(200, json!({"objects":objects})));
        }
        let range = request
            .header("range")
            .map(http::Range::parse)
            .transpose()?;
        let logical = crate::epochs::split(&key).map(|(_, inner)| inner).unwrap_or(&key);
        if format::release_file(logical) {
            return Ok(match self.storage.get_stream(project, &selected, range)? {
                Some(object) => Reply::stream(object),
                None => Reply::error(404, "no such public object"),
            });
        }
        let limit = if matches!(
            key.as_str(),
            "store.json" | "public.json" | "slots/public.age"
        ) {
            JSON_LIMIT
        } else {
            super::server::object_limit(&key)
        };
        let found = self.storage.get(project, &selected, range, limit)?;
        Ok(match found {
            None => Reply::error(404, "no such public object"),
            Some(object) => {
                let media = if key.ends_with(".json") {
                    "application/json"
                } else {
                    "application/octet-stream"
                };
                let mut reply = Reply::bytes(object.status, object.body, media);
                reply.headers.push(("Accept-Ranges".into(), "bytes".into()));
                if let Some(range) = object.content_range {
                    reply.headers.push(("Content-Range".into(), range));
                }
                reply
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rate_limit_is_bounded_per_socket_address_and_expires() {
        let limit = Limiter::default();
        let now = Instant::now();
        let peer = "100.64.0.1".parse().unwrap();
        for _ in 0..REQUESTS {
            assert!(limit.allow(peer, now));
        }
        assert!(!limit.allow(peer, now));
        assert!(limit.allow("100.64.0.2".parse().unwrap(), now));
        assert!(limit.allow(peer, now + WINDOW));
        for n in 3..CLIENTS as u32 + 2 {
            let address = std::net::Ipv4Addr::from(0x64000000 + n).into();
            assert!(limit.allow(address, now + WINDOW));
        }
        assert!(!limit.allow("100.80.0.1".parse().unwrap(), now + WINDOW));
        assert!(limit.allow("100.80.0.1".parse().unwrap(), now + WINDOW * 2));
    }
    #[test]
    fn public_names_are_anchored_and_never_include_private_namespaces() {
        for name in ["store.json", "public.json", "slots/public.age", "records/01a12290-0000-7000-8000-000000000001/00000000000000000001-00000000000000000002.age", "states/01a12290-0000-7000-8000-000000000001/0123abcd.0000.data.age"] { assert!(object(name), "{name}"); }
        for name in [
            "overlays/id/keys/key.json",
            "people/person.json",
            "slots/paper.age",
            "records/../key.age",
            "records/x/1-2.age",
            "public.json/",
            "public.json\n",
            "states/id/a.tree.age",
        ] {
            assert!(!object(name), "{name}");
        }
        for name in [
            "records/",
            "states/",
            "records/01a12290-0000-7000-8000-000000000001/0000",
        ] {
            assert!(prefix(name));
        }
        for name in [
            "",
            "overlays/",
            "people/",
            "records/../",
            "records//",
            "records/\nstates/",
            "states/01a12290-0000-7000-8000-000000000001/../../",
        ] {
            assert!(!prefix(name));
        }
    }
}
