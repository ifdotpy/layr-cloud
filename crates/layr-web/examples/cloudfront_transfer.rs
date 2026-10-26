//! A real authenticated 30 MiB object transfer through the deployed front.
//! All private input paths must be on the caller's RAM disk/tmpfs. The write grant
//! comes from LAYR_TEST_WRITE_TOKEN; neither that grant nor a session is printed.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use layr_store::{format::Descriptor, keys};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Read, path::Path, time::Duration};

fn main() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    ensure!(
        args.len() == 2,
        "usage: cloudfront_transfer <https-origin> <fixture-folder>"
    );
    let base = &args[0];
    let root = Path::new(&args[1]);
    let fixture: Value = serde_json::from_slice(&fs::read(root.join("browser.json"))?)?;
    let alias = fixture["alias"].as_str().context("alias")?;
    let descriptor: Descriptor =
        serde_json::from_slice(&fs::read(root.join("archive/store.json"))?)?;
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(150))
        .build()?;
    let grant = env::var("LAYR_TEST_WRITE_TOKEN").context("SSM write grant in environment")?;
    let identity = keys::open_slot(
        &fs::read(root.join("archive/slots/paper.age"))?,
        &keys::identity_from_paper(fs::read_to_string(root.join("paper"))?.trim())?,
    )?;
    let challenge: Value = http
        .get(format!("{base}/v1/p/{alias}/challenge"))
        .bearer_auth(&grant)
        .send()?
        .error_for_status()?
        .json()?;
    let clear = String::from_utf8(keys::decrypt(
        &identity,
        &B64.decode(
            challenge["challenge"]
                .as_str()
                .context("project challenge")?,
        )?,
    )?)?;
    let proof: Value = http
        .post(format!("{base}/v1/p/{alias}/token"))
        .bearer_auth(&grant)
        .json(&json!({"challenge":clear}))
        .send()?
        .error_for_status()?
        .json()?;
    let token = proof["token"].as_str().context("project read token")?;
    let path = root.join("cloudfront-30m.age");
    if !path.exists() {
        let mut plaintext = vec![0; 30 * 1024 * 1024];
        rand::rngs::OsRng.fill_bytes(&mut plaintext);
        fs::write(&path, keys::encrypt(&descriptor.recipient, &plaintext)?)?;
    }
    let ciphertext = fs::read(path)?;
    ensure!(
        ciphertext.len() > 30 * 1024 * 1024 && ciphertext.len() < 32 * 1024 * 1024,
        "test object size"
    );
    let expected = format!("{:x}", Sha256::digest(&ciphertext));
    let object = format!("states/{}/cf30beef.0000.data.age", descriptor.project);
    let url = format!("{base}/v1/p/{alias}/objects/{object}");
    let previous = http.get(&url).bearer_auth(token).send()?;
    if previous.status().as_u16() == 404 {
        let response = http
            .put(&url)
            .bearer_auth(grant)
            .header("Content-Type", "application/octet-stream")
            .body(ciphertext.clone())
            .send()?;
        ensure!(
            response.status().as_u16() == 201,
            "upload status {}",
            response.status()
        );
    } else {
        ensure!(
            previous.status().is_success(),
            "existing object status {}",
            previous.status()
        );
        ensure!(
            format!("{:x}", Sha256::digest(previous.bytes()?)) == expected,
            "existing object differs; do not repeat upload"
        );
    }
    let range = http
        .get(&url)
        .bearer_auth(token)
        .header("Range", "bytes=0-1023")
        .send()?;
    ensure!(range.status().as_u16() == 206, "Range was not forwarded");
    ensure!(
        range.bytes()?.as_ref() == &ciphertext[..1024],
        "range bytes differ"
    );
    let mut download = http
        .get(&url)
        .bearer_auth(token)
        .send()?
        .error_for_status()?;
    ensure!(
        download
            .headers()
            .get("via")
            .is_some_and(|v| v.to_str().unwrap_or("").contains("cloudfront")),
        "response did not traverse CloudFront"
    );
    let mut digest = Sha256::new();
    let mut count = 0;
    let mut chunk = [0; 64 * 1024];
    loop {
        let n = download.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        digest.update(&chunk[..n]);
        count += n;
    }
    ensure!(
        count == ciphertext.len() && format!("{:x}", digest.finalize()) == expected,
        "download bytes differ"
    );
    println!(
        "{}",
        json!({"status":"PASS","object":object,"bytes":count,"sha256":expected,"upload":201,"download":200,"range":206})
    );
    Ok(())
}
