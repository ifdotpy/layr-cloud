//! Person signing stays inside the worker's Session; only signed envelopes leave it.
use super::*;
use layr_store::request::{Action, Request};
use ed25519_dalek::SigningKey;

#[wasm_bindgen]
impl Session {
    pub fn open_signing_key(&mut self, sealed: &[u8], public: &str) -> std::result::Result<(), JsValue> {
        self.open_signing(sealed, public).map_err(js)
    }
    pub fn sign_request(&self, name: &str, id: &str, action: &str, now: f64) -> std::result::Result<String, JsValue> {
        self.signed_request(name, id, action, now as i64).map_err(js)
    }
}
impl Session {
    fn open_signing(&mut self, sealed: &[u8], public: &str) -> Result<()> {
        self.signing = None;
        let bytes = Zeroizing::new(keys::decrypt(self.person.as_ref().context("locked")?, sealed)?);
        let seed = Zeroizing::new(<[u8; 32]>::try_from(bytes.as_slice()).context("invalid person signing identity")?);
        let key = SigningKey::from_bytes(&seed);
        if key.verifying_key() != person::verifying_key(public)? {
            bail!("person signing identity differs from its public key");
        }
        self.signing = Some(key);
        Ok(())
    }
    pub(super) fn action_access(&self, name: &str) -> Result<Value> {
        let project = self.project(name)?;
        let public = project.config.get("person.keys").and_then(|keys| keys.get(&self.login)).and_then(Value::as_str);
        let ready = match (&self.signing, public) {
            (Some(key), Some(public)) => person::verifying_key(public).is_ok_and(|public| public == key.verifying_key()),
            _ => false,
        };
        Ok(json!({"available":ready,"reason":if ready {None} else if self.signing.is_none() {Some("Your person signing key is unavailable. Use layr person keys to set it up.")} else {Some("Your signing key is not pinned in this project.")}}))
    }
    fn signed_request(&self, name: &str, id: &str, action: &str, now: i64) -> Result<String> {
        if self.action_access(name)?["available"] != true { bail!("person signing key is unavailable or not pinned"); }
        let action: Action = serde_json::from_str(action)?;
        let request = Request::new(id.into(), self.project(name)?.id.clone(), self.login.clone(), now, action)?;
        let signed = request.sign(self.signing.as_ref().context("signing key is unavailable")?)?;
        let bytes = signed.to_bytes()?;
        if bytes.len() > layr_store::request::MAX_BYTES { bail!("request exceeds 96 KiB"); }
        Ok(String::from_utf8(bytes)?)
    }
    pub(super) fn request_status(&self, name: &str, id: &str) -> Result<Value> {
        if !layr_store::request::request_id(id) { bail!("invalid request id"); }
        let project = self.project(name)?;
        let records = project.op_log(None, usize::MAX);
        let outcomes = records.iter().filter(|record| record.data["request_id"] == id).collect::<Vec<_>>();
        if outcomes.len() > 1 { bail!("conflicting request outcomes"); }
        Ok(match outcomes.first() {
            Some(record) => json!({"id":id,"status":if record.op == "request.refused" {"refused"} else {"applied"},"record":record.id,"reason":record.data.get("reason")}),
            None => json!({"id":id,"status":"pending"}),
        })
    }
}
