//! Person records at the gateway (docs/design.md, "Person key"): below `people/<login>/` in the
//! gateway's own bucket, `person.json` (the recipient), the slots of the person key, the index
//! and the inbox. The gateway reads none of them: they are ciphertext and public keys. A caller
//! writes them only as the login that tailscaled names, and, but for the first registration and
//! an invite, only with a token that proves the person key.

use crate::vault::{
    challenge, fail, request_bytes, slot_bytes, slot_name, token_valid, Bucket, Clock,
};
use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// The most invites that one inbox holds.
pub const INBOX_LIMIT: usize = 64;
/// The largest index, in bytes.
pub const INDEX_LIMIT: usize = 48 * 1024;

/// `person.json`: the public part of a person key.
#[derive(Deserialize, Serialize)]
pub struct Record {
    pub format: u32,
    pub recipient: String,
    pub created: u64,
}

/// One invite in an inbox, as the gateway keeps it.
#[derive(Deserialize, Serialize)]
struct Invite {
    from: String,
    time: u64,
    data: String,
}

pub struct People<B> {
    bucket: B,
    /// `people/`, or another prefix in the bucket.
    prefix: String,
    /// Signs tokens; the scope `person:<login>` binds them to one person.
    secret: Arc<[u8; 32]>,
    now: Clock,
    /// One index write at a time, so that `previous` is checked against the object it replaces.
    writing: tokio::sync::Mutex<()>,
}

/// `<ms>-<8 hex>`: the name of an invite in an inbox.
fn invite_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 22
        && b[..13].iter().all(u8::is_ascii_digit)
        && b[13] == b'-'
        && b[14..]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}

impl<B: Bucket> People<B> {
    pub fn new(bucket: B, prefix: &str, secret: Arc<[u8; 32]>, now: Clock) -> Self {
        People {
            bucket,
            prefix: prefix.to_string(),
            secret,
            now,
            writing: tokio::sync::Mutex::new(()),
        }
    }

    fn key(&self, login: &str, rest: &str) -> String {
        format!("{}{login}/{rest}", self.prefix)
    }

    async fn read(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self.bucket.get(key).await? {
            Some(object) => Ok(Some(object.body.collect().await?.to_bytes().to_vec())),
            None => Ok(None),
        }
    }

    /// `person.json` of a login, if the person has registered.
    pub async fn record(&self, login: &str) -> Result<Option<Record>> {
        match self.read(&self.key(login, "person.json")).await? {
            Some(data) => match serde_json::from_slice(&data) {
                Ok(record) => Ok(Some(record)),
                Err(_) => fail(502, "the person record is not valid"),
            },
            None => Ok(None),
        }
    }

    async fn recipient(&self, login: &str) -> Result<String> {
        match self.record(login).await? {
            Some(record) => Ok(record.recipient),
            None => fail(404, "you have no person key on this gateway yet"),
        }
    }

    /// Everything that the site needs to unlock: the recipient, the slots, the index and the
    /// inbox. All of it is ciphertext but the recipient.
    pub async fn view(&self, login: &str) -> Result<Value> {
        let Some(record) = self.record(login).await? else {
            return Ok(
                json!({"login": login, "recipient": null, "slots": [], "index": null,
                             "inbox": []}),
            );
        };
        let under = self.key(login, "slots/");
        let mut slots = Vec::new();
        for key in self.bucket.list(&under).await? {
            if let Some(data) = self.read(&key).await? {
                let name = &key[under.len()..];
                let name = name.strip_suffix(".age").unwrap_or(name);
                slots.push(json!({"name": name, "data": STANDARD.encode(data)}));
            }
        }
        let index = self
            .read(&self.key(login, "index.age"))
            .await?
            .map(|data| STANDARD.encode(data));
        let under = self.key(login, "inbox/");
        let mut inbox = Vec::new();
        for key in self.bucket.list(&under).await? {
            let id = key[under.len()..].trim_end_matches(".json");
            let Some(data) = self.read(&key).await? else {
                continue;
            };
            if let Ok(invite) = serde_json::from_slice::<Invite>(&data) {
                inbox.push(json!({"id": id, "from": invite.from, "time": invite.time,
                                  "data": invite.data}));
            }
        }
        Ok(
            json!({"login": login, "recipient": record.recipient, "slots": slots,
                  "index": index, "inbox": inbox}),
        )
    }

    /// Register a person key: its recipient and its paper slot. Once only; the same recipient
    /// again is fine (true when it is new).
    pub async fn register(&self, login: &str, recipient: &str, paper: &str) -> Result<bool> {
        let recipient = recipient.trim();
        if !desk_core::keys::valid_recipient(recipient) {
            return fail(400, "the recipient is not an age recipient");
        }
        let slot = slot_bytes(paper)?;
        if let Some(record) = self.record(login).await? {
            if record.recipient == recipient {
                return Ok(false);
            }
            return fail(409, "you have another person key on this gateway");
        }
        // person.json comes last: it marks a complete registration. A paper slot of a run that
        // stopped before it is replaced.
        self.bucket
            .put(&self.key(login, "slots/paper.age"), slot)
            .await?;
        let created = (self.now)() / 1000;
        let record = Record {
            format: 1,
            recipient: recipient.to_string(),
            created,
        };
        if !self
            .bucket
            .put_new(
                &self.key(login, "person.json"),
                serde_json::to_vec(&record)?,
            )
            .await?
        {
            return fail(409, "you have another person key on this gateway");
        }
        Ok(true)
    }

    fn scope(login: &str) -> String {
        format!("person:{login}")
    }

    /// A token for the person, sealed to the person recipient.
    pub async fn challenge(&self, login: &str) -> Result<String> {
        let recipient = self.recipient(login).await?;
        challenge(&self.secret, &Self::scope(login), &recipient, (self.now)())
    }

    pub fn valid(&self, login: &str, token: &str) -> bool {
        token_valid(&self.secret, &Self::scope(login), token, (self.now)())
    }

    /// Replace the index. `previous` is the SHA-256 (hex) of the index that the caller read, or
    /// none when it read none; another one means that the index changed in between.
    pub async fn put_index(&self, login: &str, data: &str, previous: Option<&str>) -> Result<()> {
        let body = request_bytes(data);
        if body.is_empty() || body.len() > INDEX_LIMIT {
            return fail(400, "an index is 1 to 49152 bytes");
        }
        let key = self.key(login, "index.age");
        let _writing = self.writing.lock().await;
        let current = self
            .read(&key)
            .await?
            .map(|data| hex::encode(Sha256::digest(data)));
        if current.as_deref() != previous {
            return fail(409, "the index changed; read it again");
        }
        self.bucket.put(&key, body).await
    }

    pub async fn add_slot(&self, login: &str, name: &str, data: &str) -> Result<()> {
        if !slot_name(name) {
            return fail(400, "only passkey slots change here");
        }
        let body = slot_bytes(data)?;
        if !self
            .bucket
            .put_new(&self.key(login, &format!("slots/{name}.age")), body)
            .await?
        {
            return fail(409, "this passkey already opens the person key");
        }
        Ok(())
    }

    pub async fn remove_slot(&self, login: &str, name: &str) -> Result<()> {
        if !slot_name(name) {
            return fail(400, "only passkey slots change here");
        }
        self.bucket
            .delete(&self.key(login, &format!("slots/{name}.age")))
            .await
    }

    /// Drop an invite from `from` into the inbox of `to`: its id.
    pub async fn drop_invite(&self, to: &str, from: &str, data: &str) -> Result<String> {
        let sealed = request_bytes(data);
        if sealed.is_empty() || sealed.len() > 4096 {
            return fail(400, "an invite is 1 to 4096 bytes");
        }
        if self.record(to).await?.is_none() {
            return fail(404, "this person has no person key on this gateway");
        }
        let under = self.key(to, "inbox/");
        if self.bucket.list(&under).await?.len() >= INBOX_LIMIT {
            return fail(429, "the inbox of this person is full");
        }
        let mut random = [0u8; 4];
        getrandom::getrandom(&mut random).map_err(|e| anyhow::anyhow!("no random id: {e}"))?;
        let time = (self.now)();
        let id = format!("{time:013}-{}", hex::encode(random));
        let invite = Invite {
            from: from.to_string(),
            time,
            data: STANDARD.encode(sealed),
        };
        if !self
            .bucket
            .put_new(&format!("{under}{id}.json"), serde_json::to_vec(&invite)?)
            .await?
        {
            return fail(409, "try again");
        }
        Ok(id)
    }

    pub async fn remove_invite(&self, login: &str, id: &str) -> Result<()> {
        if !invite_id(id) {
            return fail(400, "not an invite id");
        }
        self.bucket
            .delete(&self.key(login, &format!("inbox/{id}.json")))
            .await
    }
}
