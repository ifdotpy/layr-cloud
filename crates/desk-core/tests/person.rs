//! The person index and invites against vectors (person-vectors.json): an index that the
//! JavaScript implementation sealed opens here with its MAC, and the index and invite that this
//! crate sealed open in JavaScript (js/vault/test/person.test.mjs).

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use desk_core::person::{self, Index, Pin};
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("person-vectors.json")).unwrap()
}

fn pins(v: &Value) -> Index {
    Index {
        vaults: serde_json::from_value::<Vec<Pin>>(v["pins"].clone()).unwrap(),
    }
}

#[test]
#[ignore = "writes person-vectors.json; run once, then run the JavaScript side"]
fn write_the_rust_vectors() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/person-vectors.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let identity = v["identity"].as_str().unwrap().to_string();
    let index = pins(&v);
    v["rustIndex"] = STANDARD
        .encode(person::seal_index(&identity, &index).unwrap())
        .into();
    let recipient = desk_core::keys::recipient_of(&identity).unwrap();
    v["rustInvite"] = STANDARD
        .encode(person::seal_invite(&recipient, &index.vaults[0]).unwrap())
        .into();
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
}

#[test]
fn an_index_sealed_in_javascript_opens_with_its_mac() {
    let v = vectors();
    let identity = v["identity"].as_str().unwrap();
    let data = STANDARD.decode(v["jsIndex"].as_str().unwrap()).unwrap();
    assert_eq!(person::open_index(identity, &data).unwrap(), pins(&v));
    let rust = STANDARD.decode(v["rustIndex"].as_str().unwrap()).unwrap();
    assert_eq!(person::open_index(identity, &rust).unwrap(), pins(&v));
    let other = desk_core::keys::generate_identity();
    assert!(person::open_index(&other, &data).is_err());
}

#[test]
fn an_invite_sealed_here_opens_here() {
    let v = vectors();
    let data = STANDARD.decode(v["rustInvite"].as_str().unwrap()).unwrap();
    let pin = person::open_invite(v["identity"].as_str().unwrap(), &data).unwrap();
    assert_eq!(pin, pins(&v).vaults[0]);
}
