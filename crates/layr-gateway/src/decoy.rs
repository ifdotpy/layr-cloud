//! Stable public unlock decoys. HMAC fills an age packet's public framing and opaque bytes.
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

// Public ciphertext, generated with age 0.11.5 from disposable, nonsecret inputs.
// Each is the median length of 1001 real packets: paper 372 bytes, signing 326 bytes.
pub(super) const PAPER: &str = include_str!("fixtures/decoy-paper.b64");
pub(super) const SIGNING: &str = include_str!("fixtures/decoy-signing.b64");

fn bytes(secret: &[u8; 32], login: &str, field: &str, length: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(length + 32);
    for i in 0..length.div_ceil(32) {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(b"layr/unlock-decoy/v2\0");
        mac.update(&(login.len() as u64).to_be_bytes());
        mac.update(login.as_bytes());
        mac.update(&(field.len() as u64).to_be_bytes());
        mac.update(field.as_bytes());
        mac.update(&(i as u64).to_be_bytes());
        out.extend_from_slice(&mac.finalize().into_bytes());
    }
    out.truncate(length);
    out
}

fn packet(secret: &[u8; 32], login: &str, field: &str, template: &str) -> String {
    let source = STANDARD
        .decode(template.trim())
        .expect("public age template");
    let footer = source.windows(5).position(|v| v == b"\n--- ").unwrap() + 1;
    let end = footer + source[footer..].iter().position(|b| *b == b'\n').unwrap() + 1;
    let header = std::str::from_utf8(&source[..end]).unwrap();
    let mut out = Vec::with_capacity(source.len());
    for (i, line) in header.lines().enumerate() {
        let label = format!("{field}/header/{i}");
        let printable = |label: &str, length| -> String {
            bytes(secret, login, label, length)
                .into_iter()
                .map(|b| char::from(33 + b % 94))
                .collect()
        };
        let replacement = if i == 0 {
            line.to_string()
        } else if let Some(stanza) = line.strip_prefix("-> ") {
            let mut words = stanza.split(' ');
            let tag = words.next().unwrap();
            let mut result = if tag == "X25519" {
                "-> X25519".into()
            } else {
                assert!(tag.ends_with("-grease"));
                format!(
                    "-> {}-grease",
                    printable(&format!("{label}/tag"), tag.len() - 7)
                )
            };
            for (j, word) in words.enumerate() {
                result.push(' ');
                let label = format!("{label}/arg/{j}");
                if tag == "X25519" {
                    let mut value = bytes(secret, login, &label, 32);
                    // X25519 public encodings have their high bit clear.
                    value[31] &= 0x7f;
                    result.push_str(&STANDARD_NO_PAD.encode(value));
                } else {
                    result.push_str(&printable(&label, word.len()));
                }
            }
            result
        } else if line.starts_with("--- ") {
            format!(
                "--- {}",
                STANDARD_NO_PAD.encode(bytes(secret, login, &label, 32))
            )
        } else {
            let length = STANDARD_NO_PAD.decode(line).unwrap().len();
            STANDARD_NO_PAD.encode(bytes(secret, login, &label, length))
        };
        out.extend_from_slice(replacement.as_bytes());
        out.push(b'\n');
    }
    out.extend_from_slice(&bytes(
        secret,
        login,
        &format!("{field}/payload"),
        source.len() - end,
    ));
    assert_eq!(out.len(), source.len());
    STANDARD.encode(out)
}

pub fn unlock(secret: &[u8; 32], login: &str) -> Value {
    let identity =
        layr_store::keys::identity_from_secret(&bytes(secret, login, "recipient", 32)).unwrap();
    let recipient = layr_store::keys::recipient_of(&identity).unwrap();
    // Rejection sampling keeps the public key well formed without making a signing key.
    let signing = (0..)
        .find_map(|i| {
            let candidate = hex::encode(bytes(secret, login, &format!("signing-key/{i}"), 32));
            layr_store::person::verifying_key(&candidate)
                .is_ok()
                .then_some(candidate)
        })
        .unwrap();
    json!({"login":login,"recipient":recipient,
        "slots":[{"name":"paper","data":packet(secret,login,"paper",PAPER)}],
        "signing_key":signing,"signing":packet(secret,login,"signing",SIGNING)})
}
