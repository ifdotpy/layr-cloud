//! Person records at the gateway (docs/design.md, "Person key"): below `people/<login>/` in the
//! gateway's own bucket, `person.json` (the recipient), the slots of the person key, the index
//! and the inbox. The gateway reads none of them: they are ciphertext and public keys. A caller
//! writes them only as the login that tailscaled names, and, but for the first registration and
//! an invite, only with a token that proves the person key.

use crate::person_support::{
    challenge, fail, request_bytes, slot_bytes, slot_name, token_valid, Bucket, Clock,
};
use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
pub use layr_store::person::Record;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// The most invites that one inbox holds.
pub const INBOX_LIMIT: usize = 64;
/// The largest index, in bytes.
pub const INDEX_LIMIT: usize = 48 * 1024;

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
    writing: Arc<std::sync::Mutex<()>>,
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
    pub fn new(
        bucket: B,
        prefix: &str,
        secret: Arc<[u8; 32]>,
        now: Clock,
        writing: Arc<std::sync::Mutex<()>>,
    ) -> Self {
        People {
            bucket,
            prefix: prefix.to_string(),
            secret,
            now,
            writing,
        }
    }

    fn key(&self, login: &str, rest: &str) -> String {
        format!("{}{login}/{rest}", self.prefix)
    }

    fn read(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self.bucket.get(key)? {
            Some(object) => Ok(Some(object)),
            None => Ok(None),
        }
    }

    /// `person.json` of a login, if the person has registered.
    pub fn record(&self, login: &str) -> Result<Option<Record>> {
        match self.read(&self.key(login, "person.json"))? {
            Some(data) => match serde_json::from_slice(&data) {
                Ok(record) => Ok(Some(record)),
                Err(_) => fail(502, "the person record is not valid"),
            },
            None => Ok(None),
        }
    }

    fn recipient(&self, login: &str) -> Result<String> {
        match self.record(login)? {
            Some(record) => Ok(record.recipient),
            None => fail(404, "you have no person key on this gateway yet"),
        }
    }

    /// Everything that the site needs to unlock: the recipient, the slots, the index and the
    /// inbox. All of it is ciphertext but the recipient.
    pub fn view(&self, login: &str) -> Result<Value> {
        let Some(record) = self.record(login)? else {
            return Ok(
                json!({"login": login, "recipient": null, "slots": [], "index": null,
                             "inbox": []}),
            );
        };
        let under = self.key(login, "slots/");
        let mut slots = Vec::new();
        for key in self.bucket.list(&under)? {
            if let Some(data) = self.read(&key)? {
                let name = &key[under.len()..];
                let name = name.strip_suffix(".age").unwrap_or(name);
                slots.push(json!({"name": name, "data": STANDARD.encode(data)}));
            }
        }
        let index = self
            .read(&self.key(login, "index.age"))?
            .map(|data| STANDARD.encode(data));
        let under = self.key(login, "inbox/");
        let mut inbox = Vec::new();
        for key in self.bucket.list(&under)? {
            let id = key[under.len()..].trim_end_matches(".json");
            let Some(data) = self.read(&key)? else {
                continue;
            };
            if let Ok(invite) = serde_json::from_slice::<Invite>(&data) {
                inbox.push(json!({"id": id, "from": invite.from, "time": invite.time,
                                  "data": invite.data}));
            }
        }
        Ok(
            json!({"login": login, "recipient": record.recipient, "slots": slots,
                  "index": index, "inbox": inbox, "signing_key":record.signing_key,
                  "signing":self.read(&self.key(login, "signing.age"))?.map(|data| STANDARD.encode(data))}),
        )
    }

    /// Unlock bootstrap excludes the index and inbox and bounds the public response.
    pub fn unlock(&self, login: &str) -> Result<Option<Value>> {
        let Some(record) = self.record(login)? else {
            return Ok(None);
        };
        let under = self.key(login, "slots/");
        let names = self.bucket.list(&under)?;
        if names.len() > crate::session::SLOT_COUNT {
            return fail(502, "too many person slots");
        }
        let mut slots = Vec::new();
        for name in names {
            let Some(data) = self.read(&name)? else {
                continue;
            };
            if data.len() > 4096 {
                return fail(502, "person slot exceeds limit");
            }
            slots.push(json!({"name":name[under.len()..].trim_end_matches(".age"),"data":STANDARD.encode(data)}));
        }
        Ok(Some(
            json!({"login":login,"recipient":record.recipient,"slots":slots,
            "signing_key":record.signing_key,"signing":self.read(&self.key(login,"signing.age"))?.map(|v|STANDARD.encode(v))}),
        ))
    }

    /// Register a person key: its recipient and its paper slot. Once only; the same recipient
    /// again is fine (true when it is new).
    pub fn register(
        &self,
        login: &str,
        recipient: &str,
        paper: &str,
        signing_key: Option<&str>,
        signing: Option<&str>,
        token: &str,
    ) -> Result<bool> {
        let recipient = recipient.trim();
        if !layr_store::keys::valid_recipient(recipient) {
            return fail(400, "the recipient is not an age recipient");
        }
        let slot = slot_bytes(paper)?;
        let signing = match (signing_key, signing) {
            (None, None) => None,
            (Some(public), Some(sealed)) => {
                if layr_store::person::verifying_key(public).is_err() {
                    return fail(400, "invalid person signing key");
                }
                Some(slot_bytes(sealed)?)
            }
            _ => return fail(400, "send both signing_key and signing"),
        };
        let record = Record {
            format: 1,
            recipient: recipient.to_string(),
            created: (self.now)() / 1000,
            signing_key: signing_key.map(str::to_string),
        };
        let paper_key = self.key(login, "slots/paper.age");
        let _writing = self
            .writing
            .lock()
            .map_err(|_| anyhow::anyhow!("person write lock failed"))?;
        // person.json first, and only if there is none: the registration that makes it is the
        // only one that writes the paper slot, so another registration of the same login (on
        // another gateway process) cannot leave a paper slot of another key.
        if self.bucket.put_new(
            &self.key(login, "person.json"),
            serde_json::to_vec(&record)?,
        )? {
            self.bucket.put(&paper_key, slot)?;
            if let Some(sealed) = signing {
                self.bucket
                    .put_new(&self.key(login, "signing.age"), sealed)?;
            }
            return Ok(true);
        }
        match self.record(login)? {
            Some(mut found) if found.recipient == recipient => {
                if let Some(public) = signing_key {
                    match found.signing_key.as_deref() {
                        Some(old) if old != public => {
                            return fail(409, "you have another person signing key")
                        }
                        None => {
                            if !self.valid(login, token) {
                                return fail(
                                    403,
                                    "upgrading a person key requires its challenge proof",
                                );
                            }
                            found.signing_key = Some(public.to_string());
                            self.bucket.put(
                                &self.key(login, "person.json"),
                                serde_json::to_vec(&found)?,
                            )?;
                        }
                        _ => (),
                    }
                    if let Some(sealed) = signing {
                        self.bucket
                            .put_new(&self.key(login, "signing.age"), sealed)?;
                    }
                }
                // The same key again: a registration that stopped before its paper slot ends now.
                self.bucket.put_new(&paper_key, slot)?;
                Ok(false)
            }
            _ => fail(409, "you have another person key on this gateway"),
        }
    }

    fn scope(login: &str) -> String {
        format!("person:{login}")
    }

    /// A token for the person, sealed to the person recipient.
    pub fn challenge(&self, login: &str) -> Result<String> {
        let recipient = self.recipient(login)?;
        challenge(&self.secret, &Self::scope(login), &recipient, (self.now)())
    }

    pub fn valid(&self, login: &str, token: &str) -> bool {
        token_valid(&self.secret, &Self::scope(login), token, (self.now)())
    }

    /// Replace the index. `previous` is the SHA-256 (hex) of the index that the caller read, or
    /// none when it read none; another one means that the index changed in between.
    pub fn put_index(&self, login: &str, data: &str, previous: Option<&str>) -> Result<()> {
        let body = request_bytes(data);
        if body.is_empty() || body.len() > INDEX_LIMIT {
            return fail(400, "an index is 1 to 49152 bytes");
        }
        let key = self.key(login, "index.age");
        let _writing = self
            .writing
            .lock()
            .map_err(|_| anyhow::anyhow!("person write lock failed"))?;
        let current = self
            .read(&key)?
            .map(|data| hex::encode(Sha256::digest(data)));
        if current.as_deref() != previous {
            return fail(409, "the index changed; read it again");
        }
        self.bucket.put(&key, body)
    }

    pub fn add_slot(&self, login: &str, name: &str, data: &str) -> Result<()> {
        if !slot_name(name) {
            return fail(400, "only passkey slots change here");
        }
        let body = slot_bytes(data)?;
        if self.bucket.list(&self.key(login, "slots/"))?.len() >= crate::session::SLOT_COUNT {
            return fail(429, "person has the maximum number of slots");
        }
        if !self
            .bucket
            .put_new(&self.key(login, &format!("slots/{name}.age")), body)?
        {
            return fail(409, "this passkey already opens the person key");
        }
        Ok(())
    }

    pub fn remove_slot(&self, login: &str, name: &str) -> Result<()> {
        if !slot_name(name) {
            return fail(400, "only passkey slots change here");
        }
        self.bucket
            .delete(&self.key(login, &format!("slots/{name}.age")))
    }

    /// Drop an invite from `from` into the inbox of `to`: its id.
    pub fn drop_invite(&self, to: &str, from: &str, data: &str) -> Result<String> {
        let sealed = request_bytes(data);
        if sealed.is_empty() || sealed.len() > 4096 {
            return fail(400, "an invite is 1 to 4096 bytes");
        }
        if self.record(to)?.is_none() {
            return fail(404, "this person has no person key on this gateway");
        }
        let under = self.key(to, "inbox/");
        if self.bucket.list(&under)?.len() >= INBOX_LIMIT {
            return fail(429, "the inbox of this person is full");
        }
        let mut random = [0u8; 4];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut random);
        let time = (self.now)();
        let id = format!("{time:013}-{}", hex::encode(random));
        let invite = Invite {
            from: from.to_string(),
            time,
            data: STANDARD.encode(sealed),
        };
        if !self
            .bucket
            .put_new(&format!("{under}{id}.json"), serde_json::to_vec(&invite)?)?
        {
            return fail(409, "try again");
        }
        Ok(id)
    }

    pub fn remove_invite(&self, login: &str, id: &str) -> Result<()> {
        if !invite_id(id) {
            return fail(400, "not an invite id");
        }
        self.bucket
            .delete(&self.key(login, &format!("inbox/{id}.json")))
    }
}
