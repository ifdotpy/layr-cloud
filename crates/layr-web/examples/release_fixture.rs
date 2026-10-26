//! Mutate only disposable release fixtures, using the existing vault and S3 implementations.
use anyhow::{Context, Result};
use layr_store::{format::Descriptor, keys, public::Manifest};
use std::{env, fs, path::PathBuf};
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let home = PathBuf::from(args.first().context("fixture folder")?);
    let store = home.join("store");
    // The fixture records its resolved epoch prefix; root exports keep it empty.
    let fixture: serde_json::Value = serde_json::from_slice(&fs::read(home.join("fixture.json"))?)?;
    let prefix = fixture["prefix"].as_str().unwrap_or("");
    let selected = store.join(prefix);
    let descriptor: Descriptor = serde_json::from_slice(&fs::read(selected.join("store.json"))?)?;
    let manifest = Manifest::parse(&fs::read(selected.join("public.json"))?, &descriptor)?;
    let identity = manifest.open(&fs::read(selected.join("slots/public.age"))?)?;
    let file = fixture["files"]
        .as_array()
        .context("files")?
        .iter()
        .find(|f| f["name"] == "sha-mismatch")
        .context("mismatch file")?;
    let object = file["object"].as_str().context("object")?;
    anyhow::ensure!(
        layr_store::format::release_file(object),
        "invalid fixture key"
    );
    let plain = vec![0x42; file["size"].as_u64().context("size")? as usize];
    let mut ciphertext = keys::encrypt(&keys::recipient_of(&identity)?, &plain)?;
    if args.get(1).map(String::as_str) == Some("truncate") {
        ciphertext.truncate(ciphertext.len() / 2);
        let env = env::vars().collect();
        let s3 = layr_store::s3::S3::with_env(args.get(2).context("test bucket")?, "", &env)?;
        let response = s3.request(
            reqwest::Method::PUT,
            &format!("fixture/{prefix}{object}"),
            &[],
            &[],
            ciphertext,
            std::time::Duration::from_secs(30),
        )?;
        anyhow::ensure!(
            response.status().is_success(),
            "fixture replacement: HTTP {}",
            response.status()
        );
    } else {
        fs::write(selected.join(object), ciphertext)?;
    }
    Ok(())
}
