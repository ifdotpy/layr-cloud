//! Desk person record validation and proof format.
use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
pub const TOKEN_LIFE_MS: u64 = 10 * 60 * 1000;
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;
pub fn fail<T>(status: u16, message: &'static str) -> Result<T> {
    Err(layr_store::http::Error { status, message }.into())
}
pub trait Bucket: Send + Sync {
    fn list(&self, prefix: &str) -> Result<Vec<String>>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn put_new(&self, key: &str, body: Vec<u8>) -> Result<bool>;
    fn put(&self, key: &str, body: Vec<u8>) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}
/// `passkey-<id>`: the slots that the gateway may add and remove.
pub fn slot_name(name: &str) -> bool {
    name.strip_prefix("passkey-").is_some_and(|id| {
        (1..=256).contains(&id.len())
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    })
}

fn token_mac(secret: &[u8; 32], scope: &str, expiry: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_slice()).expect("any key");
    mac.update(format!("{scope}:{expiry}").as_bytes());
    mac
}

/// A token for `scope` (a vault prefix, or `person:<login>`), encrypted to `recipient`: only the
/// holder of its identity can read it. `<expiry>.<HMAC of scope:expiry>`; a prefix has no `:`
/// and a login has an `@`, so a token of one scope is never valid for another.
pub fn challenge(secret: &[u8; 32], scope: &str, recipient: &str, now: u64) -> Result<String> {
    let expiry = (now + TOKEN_LIFE_MS).to_string();
    let mac = hex::encode(token_mac(secret, scope, &expiry).finalize().into_bytes());
    let sealed = layr_store::keys::encrypt(recipient, format!("{expiry}.{mac}").as_bytes())?;
    Ok(STANDARD.encode(sealed))
}

/// Whether `token` is a token of `scope` that has not expired.
pub fn token_valid(secret: &[u8; 32], scope: &str, token: &str, now: u64) -> bool {
    let Some((expiry, given)) = token.split_once('.') else {
        return false;
    };
    if !expiry.bytes().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Ok(at) = expiry.parse::<u64>() else {
        return false;
    };
    let left = i128::from(at) - i128::from(now);
    if !(left > 0 && left <= i128::from(TOKEN_LIFE_MS)) {
        return false;
    }
    match hex::decode(given) {
        Ok(given) => token_mac(secret, scope, expiry)
            .verify_slice(&given)
            .is_ok(),
        Err(_) => false,
    }
}

/// Base64 in a request, as browsers and Node write it: either alphabet, padding optional, white
/// space ignored. Nothing for text that is not base64.
pub fn request_bytes(data: &str) -> Vec<u8> {
    let text: String = data
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    LENIENT.decode(text).unwrap_or_default()
}

/// The bytes of a slot in a request: 1 to 4096 bytes.
pub fn slot_bytes(data: &str) -> Result<Vec<u8>> {
    let body = request_bytes(data);
    if body.is_empty() || body.len() > 4096 {
        return fail(400, "a slot is 1 to 4096 bytes");
    }
    Ok(body)
}

/// Base64 as browsers and Node write it: either alphabet, padding optional.
const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

impl Bucket for Box<dyn Bucket> {
    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        (**self).list(prefix)
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        (**self).get(key)
    }
    fn put_new(&self, key: &str, body: Vec<u8>) -> Result<bool> {
        (**self).put_new(key, body)
    }
    fn put(&self, key: &str, body: Vec<u8>) -> Result<()> {
        (**self).put(key, body)
    }
    fn delete(&self, key: &str) -> Result<()> {
        (**self).delete(key)
    }
}
