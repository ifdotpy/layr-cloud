//! Person signature sessions. Plain tokens never enter persistent storage.
use crate::{auth, person_support::Bucket, server::Reply};
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Mutex};

const CHALLENGE_LIFE: u64 = 60_000;
const SESSION_LIFE: u64 = 12 * 60 * 60 * 1000;
const CAPACITY: usize = 4096;
pub const INVITE_LIFE: u64 = 7 * 24 * 60 * 60 * 1000;
pub const SLOT_COUNT: usize = 8;
// Eight 4096-byte slots, a 4096-byte sealed signing key and JSON names fit in 64 KiB.
pub const UNLOCK_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct Sessions {
    challenges: Mutex<HashMap<String, (String, u64)>>,
    tokens: Mutex<HashMap<String, (String, u64)>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    pub login: String,
    pub challenge: String,
    pub signature: String,
}

#[derive(Deserialize, Serialize)]
pub struct Invite {
    login: String,
    expires: u64,
}

fn random() -> String {
    STANDARD.encode(rand::random::<[u8; 32]>())
}

impl Sessions {
    pub fn challenge(&self, login: &str, now: u64) -> Option<Value> {
        let mut values = self.challenges.lock().ok()?;
        values.retain(|_, (_, expires)| *expires > now);
        if values.len() >= CAPACITY {
            return None;
        }
        let challenge = random();
        let expires = now + CHALLENGE_LIFE;
        values.insert(auth::hash(&challenge), (login.into(), expires));
        Some(json!({"challenge":challenge,"expires":expires}))
    }

    pub fn exchange(&self, value: &Exchange, public: Option<&str>, now: u64) -> Option<Value> {
        // Consume even an invalid attempt. Failed signatures never leave a reusable challenge.
        let (login, expires) = self
            .challenges
            .lock()
            .ok()?
            .remove(&auth::hash(&value.challenge))?;
        if login != value.login || expires <= now {
            return None;
        }
        let message = layr_store::session::message(&value.login, &value.challenge).ok()?;
        let key = layr_store::person::verifying_key(public?).ok()?;
        let sig = hex::decode(&value.signature).ok()?;
        if sig.len() != 64 || hex::encode(&sig) != value.signature {
            return None;
        }
        layr_store::keys::verify_bytes(&message, &STANDARD.encode(sig), &key).ok()?;
        let mut tokens = self.tokens.lock().ok()?;
        tokens.retain(|_, (_, expires)| *expires > now);
        if tokens.len() >= CAPACITY {
            return None;
        }
        let token = hex::encode(rand::random::<[u8; 32]>());
        let expires = now + SESSION_LIFE;
        tokens.insert(auth::hash(&token), (login.clone(), expires));
        Some(json!({"login":login,"token":token,"expires":expires}))
    }

    #[cfg(test)]
    pub fn test_token(&self, login: &str) -> String {
        let token = random();
        self.tokens.lock().unwrap().insert(
            auth::hash(&token),
            (login.into(), auth::now() + SESSION_LIFE),
        );
        token
    }

    pub fn login(&self, token: &str, now: u64) -> Option<String> {
        if token.len() > 128 {
            return None;
        }
        let mut values = self.tokens.lock().ok()?;
        values.retain(|_, (_, expires)| *expires > now);
        values
            .get(&auth::hash(token))
            .map(|(login, _)| login.clone())
    }
}

pub fn invite(bucket: &dyn Bucket, prefix: &str, login: &str, now: u64) -> Result<String> {
    anyhow::ensure!(
        layr_store::person::login_name(login),
        "invalid person login"
    );
    let token = random();
    let key = format!("{prefix}_registration/{}", auth::hash(&token));
    anyhow::ensure!(
        bucket.put_new(
            &key,
            serde_json::to_vec(&Invite {
                login: login.into(),
                expires: now + INVITE_LIFE
            })?
        )?,
        "invite collision"
    );
    Ok(token)
}

pub fn consume_invite(
    bucket: &dyn Bucket,
    prefix: &str,
    login: &str,
    token: &str,
    now: u64,
) -> Result<bool> {
    if !(40..=128).contains(&token.len()) {
        return Ok(false);
    }
    let key = format!("{prefix}_registration/{}", auth::hash(token));
    let Some(body) = bucket.get(&key)? else {
        return Ok(false);
    };
    let value: Invite = serde_json::from_slice(&body)?;
    if value.login != login || value.expires <= now {
        return Ok(false);
    }
    // Conditional creation makes consumption single-use across processes and restarts.
    bucket.put_new(&format!("{key}.used"), b"{}".to_vec())
}

pub fn padded(value: &Value) -> Result<Reply> {
    let mut data = serde_json::to_vec(value)?;
    anyhow::ensure!(
        data.len() <= UNLOCK_BYTES,
        "person unlock record exceeds limit"
    );
    data.resize(UNLOCK_BYTES, b' ');
    Ok(Reply::json(200, json!({"unlock":STANDARD.encode(data)})))
}

impl crate::server::Gateway {
    pub fn session_api(
        &self,
        request: &layr_store::http::Request,
        config: &crate::config::Config,
        parts: &[String],
        query: &str,
        peer: std::net::IpAddr,
    ) -> Result<Option<Reply>> {
        let route = parts.iter().map(String::as_str).collect::<Vec<_>>();
        let challenge = route.as_slice() == ["session", "challenge"];
        let unlock = route.as_slice() == ["person", "unlock"];
        let exchange = route.as_slice() == ["session"];
        let register = route.as_slice() == ["person"] && request.method == "PUT";
        if !(challenge || unlock || exchange || register) {
            return Ok(None);
        }
        if (challenge || unlock || exchange)
            && !self
                .bootstrap_limit
                .allow_count(peer, std::time::Instant::now(), 12)
        {
            let mut reply = Reply::error(429, "sign-in rate exceeded");
            reply.headers.push(("Retry-After".into(), "60".into()));
            return Ok(Some(reply));
        }
        let Some(people_config) = &config.people else {
            return Ok(Some(Reply::error(404, "this gateway keeps no person keys")));
        };
        let people = crate::people::People::new(
            self.storage.people(people_config)?,
            &people_config.prefix,
            self.person_secret.clone(),
            std::sync::Arc::new(auth::now),
            self.person_writing.clone(),
        );
        if exchange && request.method == "POST" {
            if !query.is_empty() || request.body.len() > 4096 {
                return Ok(Some(Reply::error(400, "invalid session request")));
            }
            let Ok(value) = serde_json::from_slice::<Exchange>(&request.body) else {
                return Ok(Some(Reply::error(400, "invalid session request")));
            };
            if !layr_store::person::login_name(&value.login) {
                return Ok(Some(Reply::error(400, "invalid login")));
            }
            let public = people.record(&value.login)?.and_then(|p| p.signing_key);
            return Ok(Some(
                match self
                    .sessions
                    .exchange(&value, public.as_deref(), auth::now())
                {
                    Some(value) => Reply::json(200, value),
                    None => Reply::error(401, "sign-in proof is wrong, expired or already used"),
                },
            ));
        }
        if exchange || ((challenge || unlock) && request.method != "GET") {
            return Ok(Some(Reply::error(405, "method is not allowed")));
        }
        let login = query
            .strip_prefix("login=")
            .filter(|q| !q.contains('&'))
            .and_then(crate::server::decode);
        let Some(login) = login.filter(|v| layr_store::person::login_name(v)) else {
            return Ok(Some(Reply::error(400, "give a valid login")));
        };
        if challenge {
            if !request.body.is_empty() {
                return Ok(Some(Reply::error(400, "GET has no body")));
            }
            return Ok(Some(match self.sessions.challenge(&login, auth::now()) {
                Some(value) => Reply::json(200, value),
                None => Reply::error(429, "too many pending challenges"),
            }));
        }
        if unlock {
            if !request.body.is_empty() {
                return Ok(Some(Reply::error(400, "GET has no body")));
            }
            let value = people.unlock(&login)?.unwrap_or_else(|| self.decoy(&login));
            return Ok(Some(padded(&value)?));
        }
        if people.record(&login)?.is_some() {
            return Ok(Some(Reply::error(
                409,
                "this person is already registered; sign in instead",
            )));
        }
        if request.body.len() > 16 * 1024 {
            return Ok(Some(Reply::error(413, "registration is too large")));
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Registration {
            recipient: String,
            paper: String,
            signing_key: String,
            signing: String,
        }
        let Ok(body) = serde_json::from_slice::<Registration>(&request.body) else {
            return Ok(Some(Reply::error(
                400,
                "send recipient, paper, signing_key and signing",
            )));
        };
        if !layr_store::keys::valid_recipient(&body.recipient)
            || layr_store::person::verifying_key(&body.signing_key).is_err()
            || crate::person_support::slot_bytes(&body.paper).is_err()
            || crate::person_support::slot_bytes(&body.signing).is_err()
        {
            return Ok(Some(Reply::error(400, "invalid person registration")));
        }
        let token = request
            .header("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        let bucket = self.storage.people(people_config)?;
        if !consume_invite(
            bucket.as_ref(),
            &people_config.prefix,
            &login,
            token,
            auth::now(),
        )? {
            return Ok(Some(Reply::error(
                401,
                "registration needs an unused invite for this login",
            )));
        }
        let new = people.register(
            &login,
            &body.recipient,
            &body.paper,
            Some(&body.signing_key),
            Some(&body.signing),
            "",
        )?;
        Ok(Some(Reply::json(200, json!({"login":login,"new":new}))))
    }

    fn decoy(&self, login: &str) -> Value {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let bytes = |label: &str, length: usize| {
            let mut out = Vec::new();
            for i in 0..length.div_ceil(32) {
                let mut mac = Hmac::<Sha256>::new_from_slice(self.person_secret.as_ref()).unwrap();
                mac.update(b"layr/unlock-decoy/v1\0");
                mac.update(login.as_bytes());
                mac.update(label.as_bytes());
                mac.update(&(i as u64).to_be_bytes());
                out.extend_from_slice(&mac.finalize().into_bytes());
            }
            out.truncate(length);
            out
        };
        let recipient = bech32::encode::<bech32::Bech32>(
            bech32::Hrp::parse("age").unwrap(),
            &bytes("recipient", 32),
        )
        .unwrap();
        json!({"login":login,"recipient":recipient,
            "slots":[{"name":"paper","data":STANDARD.encode(bytes("paper",384))}],
            "signing_key":hex::encode(bytes("signing-key",32)),"signing":STANDARD.encode(bytes("signing",384))})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signature_challenges_are_single_use_bound_and_expire_with_sessions() {
        let identity = layr_store::keys::generate_identity();
        let recipient = layr_store::keys::recipient_of(&identity).unwrap();
        let keys = layr_store::person::make_signing_keys(&recipient).unwrap();
        let signing =
            layr_store::person::open_signing_key(&identity, &keys.sealed, &keys.public_key)
                .unwrap();
        let sessions = Sessions::default();
        let login = "ann@example.com";
        let attempt = |now| {
            let challenge = sessions.challenge(login, now).unwrap()["challenge"]
                .as_str()
                .unwrap()
                .to_string();
            Exchange {
                signature: layr_store::session::sign(login, &challenge, &signing).unwrap(),
                login: login.into(),
                challenge,
            }
        };
        let wrong = attempt(100);
        assert!(sessions
            .exchange(&wrong, Some(&"00".repeat(32)), 101)
            .is_none());
        assert!(sessions
            .exchange(&wrong, Some(&keys.public_key), 101)
            .is_none());
        let mut other = attempt(100);
        other.login = "bob@example.com".into();
        assert!(sessions
            .exchange(&other, Some(&keys.public_key), 101)
            .is_none());
        let expired = attempt(100);
        assert!(sessions
            .exchange(&expired, Some(&keys.public_key), 100 + CHALLENGE_LIFE)
            .is_none());
        let valid = attempt(100);
        let reply = sessions
            .exchange(&valid, Some(&keys.public_key), 101)
            .unwrap();
        let token = reply["token"].as_str().unwrap();
        assert_eq!(sessions.login(token, 102).as_deref(), Some(login));
        assert!(sessions
            .exchange(&valid, Some(&keys.public_key), 102)
            .is_none());
        assert!(sessions.login(token, 101 + SESSION_LIFE).is_none());
        assert!(!sessions.tokens.lock().unwrap().contains_key(token));
    }
}
