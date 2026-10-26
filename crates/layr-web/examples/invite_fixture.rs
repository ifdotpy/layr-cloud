//! Disposable encrypted browser-test fixture; all keys are generated for this run.
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use layr_store::{format, keys, person};
use serde_json::json;
fn main() -> Result<()> {
    let identity = keys::generate_identity();
    let recovery = keys::generate_identity();
    let vault = keys::generate_identity();
    let recipient = keys::recipient_of(&identity)?;
    let project = "01a12290-0000-7000-8000-000000000001";
    let machine = "01a12290-0000-7000-8000-000000000002";
    let login = "accept@example.test";
    let descriptor = format::Descriptor {
        format: 2,
        project: project.into(),
        recipient: keys::recipient_of(&vault)?,
    };
    let signing = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
    let public = hex::encode(signing.verifying_key().to_bytes());
    let valid = person::seal_invite(&recipient, &person::Pin::of_store(&descriptor))?;
    let foreign = person::seal_invite(
        &keys::recipient_of(&keys::generate_identity())?,
        &person::Pin::of_store(&descriptor),
    )?;
    let mut records = String::new();
    for (seq, op, data) in [
        (1, "machine", json!({"public_key":public})),
        (2, "project", json!({"name":"Accepted project"})),
    ] {
        let r = json!({"id":format!("01a12290-0000-7000-8000-{seq:012}"),"machine":machine,"project":project,"seq":seq,"time":seq,"op":op,"actor":{"uid":0,"name":"test"},"states":[],"lines":{},"data":data}).to_string();
        records.push_str(&format!(
            "{{\"r\":{r},\"s\":\"{}\"}}\n",
            keys::sign_bytes(r.as_bytes(), &signing)
        ));
    }
    let view = json!({"login":login,"recipient":recipient,"slots":[{"name":"paper","data":B64.encode(keys::seal_slot(&identity,&keys::recipient_of(&recovery)?)?)}],"index":null,"inbox":[{"id":"1793010000000-00000001","data":B64.encode(valid)}],"signing_key":public,"signing":B64.encode(keys::encrypt(&recipient,&signing.to_bytes())?)});
    println!(
        "{}",
        json!({"words":keys::words_of_identity(&recovery)?,"view":view,"descriptor":descriptor,"slot":B64.encode(keys::seal_slot(&vault,&recipient)?),"foreign":B64.encode(foreign),"segment":format::segment_name(machine,1,2),"records":B64.encode(keys::encrypt(&descriptor.recipient,records.as_bytes())?)})
    );
    Ok(())
}
