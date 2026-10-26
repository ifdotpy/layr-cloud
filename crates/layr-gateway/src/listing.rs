//! Bounded object listing pages. Continuations bind the project, prefix and scanned count.
use crate::{config::Project, server::{Entry, Gateway, Reply, Role}};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const OBJECTS: usize = 100_000;
pub const PAGE: usize = 1000;
pub const BYTES: usize = 8 << 20;

pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 1024 && !key.contains(['\\', '\0'])
        && key.split('/').all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

#[derive(Default)]
pub struct Options {
    pub prefix: String,
    pub paged: bool,
    pub continuation: Option<String>,
}
impl Options {
    pub fn parse(query: &str) -> Result<Self> {
        if !query.is_empty() && query.split('&').any(|pair| !pair.contains('=')) { bail!("invalid listing option"); }
        let url = reqwest::Url::parse(&format!("https://listing.invalid/?{query}"))?;
        let mut result = Self::default();
        let mut names = BTreeSet::new();
        for (name, value) in url.query_pairs() {
            if !names.insert(name.to_string()) { bail!("duplicate listing option"); }
            match name.as_ref() {
                "prefix" => result.prefix = value.into_owned(),
                "limit" if value == "1000" => result.paged = true,
                "continuation" if !value.is_empty() && value.len() <= 8192 => result.continuation = Some(value.into_owned()),
                _ => bail!("invalid listing option"),
            }
        }
        if result.continuation.is_some() && !result.paged { bail!("continuation requires limit=1000"); }
        Ok(result)
    }
    pub fn with_prefix(&self, prefix: &str) -> String {
        let mut url = reqwest::Url::parse("https://listing.invalid/").unwrap();
        url.query_pairs_mut().append_pair("prefix", prefix);
        if self.paged { url.query_pairs_mut().append_pair("limit", "1000"); }
        if let Some(value) = &self.continuation { url.query_pairs_mut().append_pair("continuation", value); }
        url.query().unwrap().into()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor { position: String, seen: usize }

pub fn fetch(gateway: &Gateway, project: &Project, alias: &str, role: Role, options: &Options, public: bool) -> Result<(Vec<Entry>, Option<String>)> {
    let binding = json!(["listing", public, project.binding(alias), options.prefix]).to_string();
    let previous = options.continuation.as_ref().map(|value| -> Result<Cursor> {
        let raw = gateway.proof.open_listing(&binding, value, crate::auth::now()).ok_or(layr_store::http::Error { status: 400, message: "invalid listing continuation" })?;
        let cursor: Cursor = serde_json::from_str(&raw)?;
        if cursor.seen > OBJECTS || cursor.position.is_empty() { bail!("invalid listing continuation"); }
        Ok(cursor)
    }).transpose()?;
    let (entries, next) = if options.paged {
        let page = gateway.storage.list_page(project, &options.prefix, role, previous.as_ref().map(|c| c.position.as_str()), PAGE)?;
        if page.entries.len() > PAGE { bail!("storage exceeded listing page size"); }
        (page.entries, page.continuation)
    } else {
        (gateway.storage.list(project, &options.prefix, role)?, None)
    };
    let seen = previous.as_ref().map_or(0, |c| c.seen).checked_add(entries.len()).context("listing count overflow")?;
    if seen > OBJECTS || (seen == OBJECTS && next.is_some()) { bail!("too many listed objects"); }
    let mut keys = BTreeSet::new();
    for entry in &entries {
        if !valid_key(&entry.key) || !entry.key.starts_with(&options.prefix) || !keys.insert(&entry.key) { bail!("storage returned an invalid listing page"); }
    }
    let continuation = next.map(|position| -> Result<String> {
        if previous.as_ref().is_some_and(|c| c.position == position) { bail!("storage repeated a listing continuation"); }
        let raw = serde_json::to_string(&Cursor { position, seen })?;
        let token = gateway.proof.seal_listing(&binding, &raw, crate::auth::now());
        if token.len() > 8192 { bail!("listing continuation is too large"); }
        Ok(token)
    }).transpose()?;
    Ok((entries, continuation))
}

pub fn reply(objects: Vec<Value>, continuation: Option<String>, paged: bool) -> Result<Reply> {
    let value = if paged { json!({"objects":objects,"continuation":continuation}) } else { json!({"objects":objects}) };
    struct Bounded(Vec<u8>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) > BYTES { return Err(std::io::Error::other("listing response exceeds 8 MiB")); }
            self.0.extend_from_slice(bytes); Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut output = Bounded(Vec::new());
    serde_json::to_writer(&mut output, &value)?;
    Ok(Reply::bytes(200, output.0, "application/json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn options_are_explicit_and_epoch_rewrite_preserves_the_cursor() {
        let options = Options::parse("prefix=records%2F&limit=1000&continuation=opaque-token").unwrap();
        let rewritten = Options::parse(&options.with_prefix("states/")).unwrap();
        assert_eq!(rewritten.prefix, "states/");
        assert!(rewritten.paged);
        assert_eq!(rewritten.continuation.as_deref(), Some("opaque-token"));
        assert!(!Options::parse("prefix=records%2F").unwrap().paged);
        for query in ["limit=0", "limit=1001", "limit=1000&limit=1000", "continuation=x", "prefix=x&prefix=y", "limit=1000&continuation=", "prefix=x&unknown=1"] {
            assert!(Options::parse(query).is_err(), "{query}");
        }
    }
    #[test]
    fn serialized_listing_never_exceeds_eight_mib() {
        let small = reply(vec![json!({"key":"records/test.age","modified":0})], None, true).unwrap();
        assert_eq!(small.status, 200);
        assert_eq!(serde_json::from_slice::<Value>(&small.body).unwrap()["continuation"], Value::Null);
        assert!(reply(vec![json!({"key":"x".repeat(BYTES),"modified":0})], None, false).is_err());
    }
    #[test]
    fn continuation_mac_is_scoped_expires_and_cannot_be_edited() {
        let proof = crate::auth::Proof::new();
        let token = proof.seal_listing("project/prefix", "{\"seen\":1000}", 1000);
        assert!(proof.open_listing("project/prefix", &token, 1001).is_some());
        assert!(proof.open_listing("other/prefix", &token, 1001).is_none());
        assert!(proof.open_listing("project/other", &token, 1001).is_none());
        assert!(proof.open_listing("project/prefix", &token, 1000 + crate::auth::READ_LIFE_MS).is_none());
        assert!(crate::auth::Proof::new().open_listing("project/prefix", &token, 1001).is_none());
        let mut changed = token.into_bytes();
        let last = changed.len()-1; changed[last] = if changed[last] == b'a' { b'b' } else { b'a' };
        assert!(proof.open_listing("project/prefix", std::str::from_utf8(&changed).unwrap(), 1001).is_none());
    }
}
