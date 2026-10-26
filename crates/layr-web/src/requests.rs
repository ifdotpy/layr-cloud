//! Person signing stays inside the worker's Session; only encrypted requests leave it.
use super::*;
use ed25519_dalek::SigningKey;
use layr_store::request::{Action, Request};

#[wasm_bindgen]
impl Session {
    pub fn sign_session(&self, challenge: &str) -> std::result::Result<String, JsValue> {
        let key = self
            .signing
            .as_ref()
            .context("person signing key is unavailable")
            .map_err(js)?;
        layr_store::session::sign(&self.login, challenge, key).map_err(js)
    }
    pub fn refresh_person(&mut self, view: &str) -> std::result::Result<(), JsValue> {
        self.refresh_person_view(&serde_json::from_str(view).map_err(|e| js(e.into()))?)
            .map_err(js)
    }
    /// New person material is produced and retained in the worker; only recovery words and sealed fields leave this method.
    pub fn prepare_person(
        &mut self,
        login: &str,
        entropy: &mut [u8],
    ) -> std::result::Result<String, JsValue> {
        let result = self.new_person(login, entropy);
        entropy.fill(0);
        result.map_err(js)
    }
    pub fn open_signing_key(
        &mut self,
        sealed: &[u8],
        public: &str,
    ) -> std::result::Result<(), JsValue> {
        self.open_signing(sealed, public).map_err(js)
    }
    pub fn sign_request(
        &self,
        name: &str,
        id: &str,
        action: &str,
        now: f64,
    ) -> std::result::Result<String, JsValue> {
        self.signed_request(name, id, action, now as i64)
            .map_err(js)
    }
}
impl Session {
    fn new_person(&mut self, login: &str, entropy: &[u8]) -> Result<String> {
        if !person::login_name(login) || entropy.len() != 96 {
            bail!("invalid person login or entropy");
        }
        self.lock();
        let identity = Zeroizing::new(keys::identity_from_secret(&entropy[..32])?);
        let paper = Zeroizing::new(keys::identity_from_secret(&entropy[32..64])?);
        let seed = Zeroizing::new(<[u8; 32]>::try_from(&entropy[64..])?);
        let signing = SigningKey::from_bytes(&seed);
        let recipient = keys::recipient_of(&identity)?;
        let public = signing
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let result = encoded(
            json!({"recipient":recipient,"paper":B64.encode(keys::seal_slot(&identity,&keys::recipient_of(&paper)?)?),"signing_key":public,"signing":B64.encode(keys::encrypt(&recipient,seed.as_slice())?),"words":keys::words_of_identity(&paper)?}),
        )?;
        self.person = Some(identity);
        self.login = login.into();
        self.signing = Some(signing);
        Ok(result)
    }
    fn open_signing(&mut self, sealed: &[u8], public: &str) -> Result<()> {
        self.signing = None;
        let bytes = Zeroizing::new(keys::decrypt(
            self.person.as_ref().context("locked")?,
            sealed,
        )?);
        let seed = Zeroizing::new(
            <[u8; 32]>::try_from(bytes.as_slice()).context("invalid person signing identity")?,
        );
        let key = SigningKey::from_bytes(&seed);
        if key.verifying_key() != person::verifying_key(public)? {
            bail!("person signing identity differs from its public key");
        }
        self.signing = Some(key);
        Ok(())
    }
    pub(super) fn action_access(&self, name: &str) -> Result<Value> {
        if let Some(reason) = &self
            .stores
            .get(name)
            .context("store is locked")?
            .cache
            .policy_note
        {
            return Ok(json!({"available":false,"reason":reason}));
        }
        let project = self.project(name)?;
        let public = project
            .config
            .get("person.keys")
            .and_then(|keys| keys.get(&self.login))
            .and_then(Value::as_str);
        let ready = match (&self.signing, public) {
            (Some(key), Some(public)) => {
                person::verifying_key(public).is_ok_and(|public| public == key.verifying_key())
            }
            _ => false,
        };
        Ok(
            json!({"available":ready,"reason":if ready {None} else if self.signing.is_none() {Some("Your person signing key is unavailable. Use layr person keys to set it up.")} else {Some("Your signing key is not pinned in this project.")}}),
        )
    }
    fn signed_request(&self, name: &str, id: &str, action: &str, now: i64) -> Result<String> {
        if self.action_access(name)?["available"] != true {
            bail!("person signing key is unavailable or not pinned");
        }
        let action: Action = serde_json::from_str(action)?;
        let request = Request::new(
            id.into(),
            self.project(name)?.id.clone(),
            self.login.clone(),
            now,
            action,
        )?;
        let key = self
            .signing
            .as_ref()
            .context("signing key is unavailable")?;
        let signed = request.sign(key)?;
        let inner = Zeroizing::new(signed.to_bytes()?);
        let recipient = &self
            .stores
            .get(name)
            .context("store is locked")?
            .descriptor
            .recipient;
        let store = self.stores.get(name).context("store is locked")?;
        let bytes = match &store.epochs {
            Some(epochs) => layr_store::request::seal_for_epoch(
                &inner,
                epochs.epoch(),
                &epochs.current.recipient,
                key,
            )?,
            None => layr_store::request::seal(&inner, recipient, key)?,
        };
        if bytes.len() > layr_store::request::MAX_SEALED_BYTES {
            bail!("encrypted request exceeds 144 KiB");
        }
        Ok(String::from_utf8(bytes)?)
    }
    pub(super) fn request_status(&self, name: &str, id: &str) -> Result<Value> {
        if !layr_store::request::request_id(id) {
            bail!("invalid request id");
        }
        let project = self.project(name)?;
        let records = project.op_log(None, usize::MAX);
        let outcomes = records
            .iter()
            .filter(|record| record.data["request_id"] == id)
            .collect::<Vec<_>>();
        if outcomes.len() > 1 {
            bail!("conflicting request outcomes");
        }
        Ok(match outcomes.first() {
            Some(record) => {
                json!({"id":id,"status":if record.op == "request.refused" {"refused"} else {"applied"},"record":record.id,"reason":record.data.get("reason")})
            }
            None => json!({"id":id,"status":"pending"}),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_person_recovers_and_signs_the_shared_session_proof() {
        let mut session = Session::new();
        let mut entropy = (0..96).collect::<Vec<u8>>();
        let prepared: Value = serde_json::from_str(
            &session
                .prepare_person("ann@example.com", &mut entropy)
                .unwrap(),
        )
        .unwrap();
        assert!(entropy.iter().all(|byte| *byte == 0));
        let view = json!({"login":"ann@example.com","recipient":prepared["recipient"],"signing_key":prepared["signing_key"],"signing":prepared["signing"],"slots":[{"name":"paper","data":prepared["paper"]}],"index":null,"inbox":[]});
        let key = session.signing.as_ref().unwrap().verifying_key();
        session.lock();
        let paper = keys::identity_from_paper(prepared["words"].as_str().unwrap()).unwrap();
        session.unlock(&paper, "paper", &view.to_string()).unwrap();
        session
            .open_signing(
                &unb64(prepared["signing"].as_str().unwrap()).unwrap(),
                prepared["signing_key"].as_str().unwrap(),
            )
            .unwrap();
        let challenge = B64.encode([17; 32]);
        let signature = session.sign_session(&challenge).unwrap();
        let bytes = (0..signature.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&signature[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let signature = ed25519_dalek::Signature::from_slice(&bytes).unwrap();
        key.verify_strict(
            &layr_store::session::message("ann@example.com", &challenge).unwrap(),
            &signature,
        )
        .unwrap();
        assert!(key
            .verify_strict(
                &layr_store::session::message("bob@example.com", &challenge).unwrap(),
                &signature
            )
            .is_err());
        session.refresh_person_view(&view).unwrap();
        let mut changed = view.clone();
        changed["login"] = json!("bob@example.com");
        assert!(session.refresh_person_view(&changed).is_err());
        changed = view.clone();
        changed["signing_key"] = json!("00".repeat(32));
        assert!(session.refresh_person_view(&changed).is_err());
        session.lock();
        assert!(session.signing.is_none());
    }
    #[test]
    fn signing_requires_matching_person_and_project_pin_and_lock_discards_it() {
        let (mut session, _, _, _) = crate::tests::fixture();
        let identity = session.person.as_ref().unwrap().to_string();
        let key = SigningKey::from_bytes(&[47; 32]);
        let public = key
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let sealed =
            keys::encrypt(&keys::recipient_of(&identity).unwrap(), &key.to_bytes()).unwrap();
        session.open_signing(&sealed, &public).unwrap();
        let name = session.stores.keys().next().unwrap().clone();
        let action =
            json!({"kind":"approve","state":"01a12290-0000-7000-8000-000000000003","into":"main"})
                .to_string();
        let id = "01a12290-0000-7000-8000-000000000004";
        // The fixture has no records yet. A pin and an opened signing key are both required.
        let project = session
            .stores
            .get(&name)
            .unwrap()
            .descriptor
            .project
            .clone();
        let model = layr_model::Project::from_records(&project, [], "").unwrap();
        session
            .stores
            .get_mut(&name)
            .unwrap()
            .model
            .projects
            .insert(project.clone(), model);
        assert_eq!(session.action_access(&name).unwrap()["available"], false);
        assert!(session.signed_request(&name, id, &action, 1).is_err());
        session
            .stores
            .get_mut(&name)
            .unwrap()
            .model
            .projects
            .get_mut(&project)
            .unwrap()
            .config
            .insert("person.keys".into(), json!({session.login.clone(): public}));
        let signed = session.signed_request(&name, id, &action, 1).unwrap();
        layr_store::request::Sealed::parse(signed.as_bytes())
            .unwrap()
            .verify(&key.verifying_key())
            .unwrap();
        let inner =
            layr_store::request::open(signed.as_bytes(), &session.stores[&name].identity).unwrap();
        assert!(!signed.contains("\"kind\""));
        let request = layr_store::request::Signed::parse(&inner)
            .unwrap()
            .verify(&key.verifying_key())
            .unwrap();
        assert_eq!(request.person, session.login);
        assert_eq!(request.project, project);
        assert!(session.signed_request(&name, "bad", &action, 1).is_err());
        assert!(session.open_signing(&sealed, &"00".repeat(32)).is_err());
        assert!(session.signing.is_none());
        session.open_signing(&sealed, &public).unwrap();
        session.lock();
        assert!(session.signing.is_none());
        assert!(session.person.is_none());
    }
}

#[cfg(test)]
mod executor_test {
    use super::*;
    use std::{
        env, fs,
        path::Path,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    fn load(root: &Path, person: &Path) -> Session {
        let view: Value =
            serde_json::from_slice(&fs::read(person.join("person-view.json")).unwrap()).unwrap();
        let words = fs::read_to_string(person.join("recovery.txt")).unwrap();
        let mut session = Session::new();
        session
            .unlock(
                &keys::identity_from_paper(words.trim()).unwrap(),
                "paper",
                &view.to_string(),
            )
            .unwrap();
        session
            .open_signing(
                &unb64(view["signing"].as_str().unwrap()).unwrap(),
                view["signing_key"].as_str().unwrap(),
            )
            .unwrap();
        let slot = session.slot_name().unwrap();
        session
            .add_store(
                "test",
                &fs::read_to_string(root.join("store.json")).unwrap(),
                &fs::read(root.join("slots").join(slot)).unwrap(),
            )
            .unwrap();
        fn records(session: &mut Session, root: &Path, at: &Path) {
            for entry in fs::read_dir(at).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    records(session, root, &path);
                } else {
                    session
                        .segment(
                            "test",
                            path.strip_prefix(root).unwrap().to_str().unwrap(),
                            &fs::read(&path).unwrap(),
                        )
                        .unwrap();
                }
            }
        }
        records(&mut session, root, &root.join("records"));
        session.finish("test").unwrap();
        session
    }
    #[test]
    #[ignore = "requires a disposable real CLI executor; see README"]
    fn real_executor_applies_browser_signatures_and_records_refusals() {
        let home = env::var("LAYR_WEB_ACTION_FIXTURE").expect("LAYR_WEB_ACTION_FIXTURE");
        let home = Path::new(&home);
        let root = home.join("store");
        let person = home.join("person");
        let mut session = load(&root, &person);
        let project = session.project("test").unwrap();
        let feature = project
            .line_list()
            .into_iter()
            .find(|line| line.name == "feature")
            .unwrap()
            .head;
        let change = project.states[&feature].state.change_id.clone().unwrap();
        let mut count = 0;
        let mut apply = |action: Value, status: &str| -> String {
            count += 1;
            let id = format!("01a1734c-0000-7000-8000-{count:012}");
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let envelope = session
                .signed_request("test", &id, &action.to_string(), now)
                .unwrap();
            let outer: Value = serde_json::from_str(&envelope).unwrap();
            assert_eq!(outer["v"], 2);
            assert!(outer.get("r").is_none() && outer.get("action").is_none());
            assert!(!envelope.contains("\"kind\""));
            fs::create_dir_all(root.join("requests")).unwrap();
            fs::write(root.join("requests").join(format!("{id}.json")), &envelope).unwrap();
            assert_eq!(
                session.request_status("test", &id).unwrap()["status"],
                "pending"
            );
            let result = Command::new(env::var("LAYR_WEB_ACTION_EXECUTOR").unwrap())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "executor: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            session = load(&root, &person);
            let outcome = session.request_status("test", &id).unwrap();
            assert_eq!(outcome["status"], status, "{outcome}");
            assert!(outcome["record"].is_string());
            if status == "refused" {
                assert!(outcome["reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty()));
            }
            id
        };
        apply(
            json!({"kind":"approve","state":feature,"into":"main"}),
            "applied",
        );
        apply(
            json!({"kind":"withdraw","state":feature,"into":"main"}),
            "applied",
        );
        let comment = apply(
            json!({"kind":"comment","change_id":change,"version":feature,"text":"Ready for review."}),
            "applied",
        );
        apply(
            json!({"kind":"comment","change_id":change,"version":feature,"text":"Reviewed this version.","reply_to":comment}),
            "applied",
        );
        apply(
            json!({"kind":"comment","change_id":change,"version":feature,"path":"test.txt","line":2,"text":"Keep this second line."}),
            "applied",
        );
        apply(
            json!({"kind":"ref_move","name":"env/native-test","target":feature,"expected":null}),
            "applied",
        );
        apply(
            json!({"kind":"ref_move","name":"env/native-test","target":feature,"expected":null}),
            "refused",
        );
        let entry = apply(
            json!({"kind":"queue_add","state":feature,"into":"main"}),
            "applied",
        );
        apply(json!({"kind":"queue_remove","entry":entry}), "applied");
        drop(apply);
        let comments = session.project("test").unwrap().comments(&change);
        assert_eq!(comments.len(), 3);
        assert!(comments
            .iter()
            .all(|comment| comment["actor"]["name"] == "person:owner@example.com"));
        assert_eq!(comments[2]["line"], 2);
        println!("real executor: {count} signed browser requests, 8 applied, 1 refused, 3 folded comments");
    }
}
