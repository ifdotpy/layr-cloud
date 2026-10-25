//! Stable public unlock decoys. HMAC fills an age packet's public framing and opaque bytes.
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

// Public real-age ciphertext fixtures are used only to compare packet structure in tests.
#[cfg(test)]
pub(super) const PAPER: &str = include_str!("fixtures/decoy-paper.b64");
#[cfg(test)]
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

// age-core 0.11.0 format::grease_the_joint: all ranges here are half-open.
// Rejection sampling gives the same uniform choices from a deterministic HMAC stream.
fn uniform(secret: &[u8; 32], login: &str, field: &str, bound: u16) -> usize {
    let cutoff = 256 - 256 % bound;
    for counter in 0.. {
        let value = u16::from(bytes(secret, login, &format!("{field}/{counter}"), 1)[0]);
        if value < cutoff {
            return usize::from(value % bound);
        }
    }
    unreachable!()
}

fn printable(secret: &[u8; 32], login: &str, field: &str) -> String {
    let length = 1 + uniform(secret, login, &format!("{field}/length"), 8);
    (0..length)
        .map(|i| char::from(33 + uniform(secret, login, &format!("{field}/char/{i}"), 94) as u8))
        .collect()
}

fn wrapped(out: &mut String, value: &[u8]) {
    let encoded = STANDARD_NO_PAD.encode(value);
    let mut tail = encoded.as_str();
    while tail.len() >= 64 {
        out.push_str(&tail[..64]);
        out.push('\n');
        tail = &tail[64..];
    }
    // The final body line is always short, including an empty line after a full line.
    out.push_str(tail);
    out.push('\n');
}

fn packet(secret: &[u8; 32], login: &str, field: &str, plaintext_length: usize) -> String {
    let mut ephemeral = bytes(secret, login, &format!("{field}/ephemeral"), 32);
    ephemeral[31] &= 0x7f;
    let mut out = format!(
        "age-encryption.org/v1\n-> X25519 {}\n",
        STANDARD_NO_PAD.encode(ephemeral)
    );
    wrapped(
        &mut out,
        &bytes(secret, login, &format!("{field}/wrapped-key"), 32),
    );
    // age 0.11.5 HeaderV1::new always appends one GREASE stanza for X25519.
    out.push_str(&format!(
        "-> {}-grease",
        printable(secret, login, &format!("{field}/grease/tag"))
    ));
    for i in 0..uniform(secret, login, &format!("{field}/grease/arg-count"), 5) {
        out.push(' ');
        out.push_str(&printable(
            secret,
            login,
            &format!("{field}/grease/arg/{i}"),
        ));
    }
    out.push('\n');
    let length = uniform(secret, login, &format!("{field}/grease/body-length"), 100);
    wrapped(
        &mut out,
        &bytes(secret, login, &format!("{field}/grease/body"), length),
    );
    out.push_str(&format!(
        "--- {}\n",
        STANDARD_NO_PAD.encode(bytes(secret, login, &format!("{field}/mac"), 32))
    ));
    let mut out = out.into_bytes();
    // One stream chunk: 16-byte nonce, plaintext, and 16-byte authentication tag.
    out.extend_from_slice(&bytes(
        secret,
        login,
        &format!("{field}/payload"),
        16 + plaintext_length + 16,
    ));
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
        "slots":[{"name":"paper","data":packet(secret,login,"paper",75)}],
        "signing_key":signing,"signing":packet(secret,login,"signing",32)})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousand_decoy_lengths_follow_real_age_grease_and_survive_restart() {
        let secret = [42; 32];
        let identity = layr_store::keys::identity_from_secret(&[1; 32]).unwrap();
        let recipient = layr_store::keys::recipient_of(&identity).unwrap();
        let mut real = [[0usize; 64]; 2];
        let mut decoys = [[0usize; 64]; 2];
        let mut lengths = std::collections::BTreeSet::new();
        for i in 0..1000 {
            let login = format!("person-{i}@example.com");
            let before = unlock(&secret, &login);
            // A reconstructed process has the persisted key, with no per-login cache.
            let restarted_secret: [u8; 32] = secret.to_vec().try_into().unwrap();
            assert_eq!(before, unlock(&restarted_secret, &login));
            for (field, plain, value) in [
                (0, 75, &before["slots"][0]["data"]),
                (1, 32, &before["signing"]),
            ] {
                let known = layr_store::keys::encrypt(&recipient, &vec![0; plain]).unwrap();
                let decoy = STANDARD.decode(value.as_str().unwrap()).unwrap();
                assert!(age::Decryptor::new(decoy.as_slice()).is_ok());
                real[field][known.len() / 16] += 1;
                decoys[field][decoy.len() / 16] += 1;
                if field == 0 {
                    lengths.insert(decoy.len());
                }
            }
        }
        assert!(
            lengths.len() > 80,
            "GREASE lengths must vary between logins"
        );
        for field in 0..2 {
            let l1: usize = real[field]
                .iter()
                .zip(&decoys[field])
                .map(|(a, b)| a.abs_diff(*b))
                .sum();
            let (mut r, mut d) = (0usize, 0usize);
            for (a, b) in real[field].iter().zip(&decoys[field]) {
                r += a;
                d += b;
                assert!(
                    r.abs_diff(d) < 150,
                    "age/decoy length CDF differs by 15% or more"
                );
            }
            assert!(
                l1 < 350,
                "age/decoy 16-byte histogram differs by 35% or more"
            );
        }
    }
}
