//! Disposable native caller for the same Session signing API used by the browser.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use layr_web::Session;
use serde_json::{json, Value};
use std::{env, fs};
fn main() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "passkey-fixture") {
        let home = std::path::Path::new(args.get(1).context("person folder")?);
        let words = fs::read_to_string(home.join("recovery.txt"))?;
        let slot = home.join("people/owner@example.com/slots/paper.age");
        let identity = layr_store::keys::open_slot(&fs::read(slot)?,&layr_store::keys::identity_from_paper(words.trim())?)?;
        let secret = (0..32).collect::<Vec<u8>>();
        let opener = layr_store::keys::identity_from_secret(&secret)?;
        fs::write(home.join("people/owner@example.com/slots/passkey-AQIDBA.age"),layr_store::keys::seal_slot(&identity,&layr_store::keys::recipient_of(&opener)?)?)?;
        return Ok(());
    }
    let base = args.first().context("gateway origin")?;
    let login = args.get(1).context("login")?;
    let words = fs::read_to_string(args.get(2).context("recovery file")?)?;
    let http = reqwest::blocking::Client::builder().no_proxy().build()?;
    ensure!(http.get(format!("{base}/api/person")).send()?.status() == 401, "person route needs a session");
    let bootstrap: Value = http.get(format!("{base}/api/person/unlock")).query(&[("login",login)]).send()?.error_for_status()?.json()?;
    let bytes = B64.decode(bootstrap["unlock"].as_str().context("padded unlock")?)?;
    ensure!(bytes.len() == 65536, "unlock size");
    let view: Value = serde_json::from_slice(&bytes)?;
    ensure!(view.get("index").is_none() && view.get("inbox").is_none(),"private person data before proof");
    let mut session = Session::new();
    session.unlock_recovery(words.trim(), &view.to_string()).map_err(|_| anyhow::anyhow!("native recovery unlock failed"))?;
    session.open_signing_key(&B64.decode(view["signing"].as_str().context("sealed signing")?)?,view["signing_key"].as_str().context("public signing")?).map_err(|_| anyhow::anyhow!("native signing key failed"))?;
    let challenge: Value = http.get(format!("{base}/api/session/challenge")).query(&[("login",login)]).send()?.error_for_status()?.json()?;
    let signature = session.sign_session(challenge["challenge"].as_str().context("challenge")?).map_err(|_|anyhow::anyhow!("native session signature failed"))?;
    let exchange = json!({"login":login,"challenge":challenge["challenge"],"signature":signature});
    let token: Value = http.post(format!("{base}/api/session")).json(&exchange).send()?.error_for_status()?.json()?;
    let person: Value = http.get(format!("{base}/api/person")).bearer_auth(token["token"].as_str().context("session")?).send()?.error_for_status()?.json()?;
    ensure!(person["login"] == *login && person["index"].is_string(),"authenticated person index");
    ensure!(http.post(format!("{base}/api/session")).json(&exchange).send()?.status()==401,"replayed challenge was accepted");
    let challenge: Value = http.get(format!("{base}/api/session/challenge")).query(&[("login",login)]).send()?.error_for_status()?.json()?;
    ensure!(http.post(format!("{base}/api/session")).json(&json!({"login":login,"challenge":challenge["challenge"],"signature":"00".repeat(64)})).send()?.status()==401,"wrong proof was accepted");
    session.lock();
    println!("PASS native browser Session: padded bootstrap, shared signature, authenticated index, replay and wrong proof refused, lock");
    Ok(())
}
