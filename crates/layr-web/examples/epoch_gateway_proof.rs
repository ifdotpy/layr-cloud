//! VM-only live smoke proof. Private inputs and the temporary read token stay in /run.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use layr_store::keys;
use serde_json::{json, Value};
use std::{env, fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

fn main() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let phase = args.first().context("capture or deny")?;
    let home = PathBuf::from(args.get(1).context("VM fixture")?);
    ensure!(
        home.starts_with("/run") && home.is_dir(),
        "fixture must be in VM tmpfs"
    );
    let fixture: Value = serde_json::from_slice(&fs::read(home.join("browser.json"))?)?;
    let alias = fixture["alias"].as_str().context("alias")?;
    let base = format!("https://cloud.layr.ws/v1/p/{alias}");
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()?;
    let token_path = home.join("removed-root-proof");
    let identity = keys::open_slot(
        &fs::read(home.join("archive/slots/paper.age"))?,
        &keys::identity_from_paper(fs::read_to_string(home.join("paper"))?.trim())?,
    )?;
    match phase.as_str() {
        "capture" => {
            ensure!(!token_path.exists(), "proof already captured");
            let grant = fs::read_to_string(home.join("write-token"))?;
            let challenge: Value = http
                .get(format!("{base}/challenge"))
                .bearer_auth(grant.trim())
                .send()?
                .error_for_status()?
                .json()?;
            let clear = String::from_utf8(keys::decrypt(
                &identity,
                &B64.decode(challenge["challenge"].as_str().context("challenge")?)?,
            )?)?;
            let proof: Value = http
                .post(format!("{base}/token"))
                .bearer_auth(grant.trim())
                .json(&json!({"challenge":clear}))
                .send()?
                .error_for_status()?
                .json()?;
            let token = proof["token"].as_str().context("read token")?;
            ensure!(
                http.get(format!("{base}/objects?prefix=records%2F"))
                    .bearer_auth(token)
                    .send()?
                    .status()
                    == 200,
                "root proof must read the original epoch"
            );
            fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&token_path)?
                .write_all(token.as_bytes())?;
            println!("PASS removed reader root proof reads original epoch before rotation");
        }
        "deny" => {
            let epoch = fixture["epoch"].as_str().context("current epoch")?;
            let object = args.get(2).context("post-rotation encrypted object")?;
            ensure!(
                object.starts_with(&format!("project/epochs/{epoch}/")),
                "object must belong to current epoch"
            );
            let token = fs::read_to_string(&token_path)?;
            for target in [
                format!("{base}/objects/{object}"),
                format!("{base}/objects?prefix=project%2Fepochs%2F{epoch}%2Frecords%2F"),
            ] {
                ensure!(
                    http.get(target).bearer_auth(token.trim()).send()?.status() == 404,
                    "old proof must not read or list current ciphertext"
                );
            }
            let grant = fs::read_to_string(home.join("write-token"))?;
            let challenge: Value = http
                .get(format!("{base}/challenge?epoch={epoch}"))
                .bearer_auth(grant.trim())
                .send()?
                .error_for_status()?
                .json()?;
            ensure!(
                keys::decrypt(
                    &identity,
                    &B64.decode(challenge["challenge"].as_str().context("epoch challenge")?)?
                )
                .is_err(),
                "removed identity must not open current proof challenge"
            );
            fs::remove_file(token_path)?;
            println!("PASS removed reader old proof cannot fetch/list post-rotation ciphertext or open the new challenge");
        }
        _ => anyhow::bail!("expected capture or deny"),
    }
    Ok(())
}
