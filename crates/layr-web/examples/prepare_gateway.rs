//! Make disposable person objects around a CLI-created test store.
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use layr_store::{format::Descriptor, keys, person};
use serde_json::json;
use std::{env, fs, path::PathBuf};
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let root = PathBuf::from(args.first().context("store folder")?);
    let paper = fs::read_to_string(args.get(1).context("store recovery key file")?)?;
    let output = PathBuf::from(args.get(2).context("output folder")?);
    fs::create_dir_all(&output)?;
    let descriptor: Descriptor = serde_json::from_slice(&fs::read(root.join("store.json"))?)?;
    let vault = keys::open_slot(
        &fs::read(root.join("slots/paper.age"))?,
        &keys::identity_from_paper(&paper)?,
    )?;
    anyhow::ensure!(
        keys::recipient_of(&vault)? == descriptor.recipient,
        "store recipient"
    );
    let identity = keys::generate_identity();
    let recovery = keys::generate_identity();
    let login = "owner@example.com";
    let recipient = keys::recipient_of(&identity)?;
    let slot = keys::seal_slot(&identity, &keys::recipient_of(&recovery)?)?;
    let index = person::Index {
        vaults: vec![person::Pin::of_store(&descriptor)],
    };
    let sealed_index = person::seal_index(&identity, &index)?;
    fs::write(
        root.join("slots")
            .join(person::slot_name(login, &recipient)?),
        keys::seal_slot(&vault, &recipient)?,
    )?;
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    let signing = person::SigningKeys {
        public_key: signing_key.verifying_key().to_bytes().iter().map(|byte| format!("{byte:02x}")).collect(),
        sealed: keys::encrypt(&recipient, &signing_key.to_bytes())?,
    };
    let person_root = output.join("people").join(login);
    fs::create_dir_all(person_root.join("slots"))?;
    fs::write(
        person_root.join("person.json"),
        serde_json::to_vec(&person::Record {
            format: 1,
            recipient: recipient.clone(),
            created: 0,
            signing_key: Some(signing.public_key.clone()),
        })?,
    )?;
    fs::write(person_root.join("slots/paper.age"), &slot)?;
    fs::write(person_root.join("signing.age"), &signing.sealed)?;
    fs::write(output.join("signing-public.txt"), &signing.public_key)?;
    fs::write(person_root.join("index.age"), &sealed_index)?;
    fs::write(
        output.join("recovery.txt"),
        keys::words_of_identity(&recovery)?,
    )?;
    fs::write(
        output.join("person-view.json"),
        serde_json::to_vec(
            &json!({"login":login,"recipient":recipient,"slots":[{"name":"paper","data":B64.encode(slot)}],"index":B64.encode(sealed_index),"inbox":[],"signing_key":signing.public_key,"signing":B64.encode(signing.sealed)}),
        )?,
    )?;
    fs::write(
        output.join("project.json"),
        serde_json::to_vec(
            &json!({"project":descriptor.project,"recipient":descriptor.recipient}),
        )?,
    )?;
    Ok(())
}
