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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signing_requires_matching_person_and_project_pin_and_lock_discards_it() {
        let (mut session, _, _, _) = crate::tests::fixture();
        let identity = session.person.as_ref().unwrap().to_string();
        let key = SigningKey::from_bytes(&[47;32]);
        let public = key.verifying_key().to_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let sealed = keys::encrypt(&keys::recipient_of(&identity).unwrap(), &key.to_bytes()).unwrap();
        session.open_signing(&sealed,&public).unwrap();
        let name = session.stores.keys().next().unwrap().clone();
        let action = json!({"kind":"approve","state":"01a12290-0000-7000-8000-000000000003","into":"main"}).to_string();
        let id = "01a12290-0000-7000-8000-000000000004";
        // The fixture has no records yet. A pin and an opened signing key are both required.
        let project = session.stores.get(&name).unwrap().descriptor.project.clone();
        let model = layr_model::Project::from_records(&project, [], "").unwrap();
        session.stores.get_mut(&name).unwrap().model.projects.insert(project.clone(), model);
        assert_eq!(session.action_access(&name).unwrap()["available"],false);
        assert!(session.signed_request(&name,id,&action,1).is_err());
        session.stores.get_mut(&name).unwrap().model.projects.get_mut(&project).unwrap().config.insert("person.keys".into(),json!({session.login.clone(): public}));
        let signed = session.signed_request(&name,id,&action,1).unwrap();
        let request = layr_store::request::Signed::parse(signed.as_bytes()).unwrap().verify(&key.verifying_key()).unwrap();
        assert_eq!(request.person,session.login);
        assert_eq!(request.project,project);
        assert!(session.signed_request(&name,"bad",&action,1).is_err());
        assert!(session.open_signing(&sealed,&"00".repeat(32)).is_err());
        assert!(session.signing.is_none());
        session.open_signing(&sealed,&public).unwrap();
        session.lock();
        assert!(session.signing.is_none());
        assert!(session.person.is_none());
    }
}

#[cfg(test)]
mod executor_test {
    use super::*;
    use std::{env,fs,path::Path,process::Command,time::{SystemTime,UNIX_EPOCH}};
    fn load(root: &Path, person: &Path) -> Session {
        let view: Value = serde_json::from_slice(&fs::read(person.join("person-view.json")).unwrap()).unwrap();
        let words = fs::read_to_string(person.join("recovery.txt")).unwrap();
        let mut session = Session::new();
        session.unlock(&keys::identity_from_paper(words.trim()).unwrap(),"paper",&view.to_string()).unwrap();
        session.open_signing(&unb64(view["signing"].as_str().unwrap()).unwrap(),view["signing_key"].as_str().unwrap()).unwrap();
        let slot = session.slot_name().unwrap();
        session.add_store("test",&fs::read_to_string(root.join("store.json")).unwrap(),&fs::read(root.join("slots").join(slot)).unwrap()).unwrap();
        fn records(session: &mut Session, root: &Path, at: &Path) {
            for entry in fs::read_dir(at).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() { records(session,root,&path); }
                else { session.segment("test",path.strip_prefix(root).unwrap().to_str().unwrap(),&fs::read(&path).unwrap()).unwrap(); }
            }
        }
        records(&mut session,root,&root.join("records"));
        session.finish("test").unwrap();
        session
    }
    #[test]
    #[ignore = "requires a disposable real CLI executor; see README"]
    fn real_executor_applies_browser_signatures_and_records_refusals() {
        let home = env::var("LAYR_WEB_ACTION_FIXTURE").expect("LAYR_WEB_ACTION_FIXTURE");
        let home = Path::new(&home);
        let root = home.join("store"); let person = home.join("person");
        let mut session = load(&root,&person);
        let project = session.project("test").unwrap();
        let feature = project.line_list().into_iter().find(|line|line.name=="feature").unwrap().head;
        let change = project.states[&feature].state.change_id.clone().unwrap();
        let mut count = 0;
        let mut apply = |action: Value, status: &str| -> String {
            count += 1;
            let id = format!("01a1734c-0000-7000-8000-{count:012}");
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
            let envelope = session.signed_request("test",&id,&action.to_string(),now).unwrap();
            fs::create_dir_all(root.join("requests")).unwrap();
            fs::write(root.join("requests").join(format!("{id}.json")),&envelope).unwrap();
            assert_eq!(session.request_status("test",&id).unwrap()["status"],"pending");
            let result = Command::new(env::var("LAYR_WEB_ACTION_EXECUTOR").unwrap()).output().unwrap();
            assert!(result.status.success(),"executor: {}",String::from_utf8_lossy(&result.stderr));
            session = load(&root,&person);
            let outcome = session.request_status("test",&id).unwrap();
            assert_eq!(outcome["status"],status,"{outcome}");
            assert!(outcome["record"].is_string());
            if status == "refused" { assert!(outcome["reason"].as_str().is_some_and(|reason|!reason.is_empty())); }
            id
        };
        apply(json!({"kind":"approve","state":feature,"into":"main"}),"applied");
        apply(json!({"kind":"withdraw","state":feature,"into":"main"}),"applied");
        let comment = apply(json!({"kind":"comment","change_id":change,"version":feature,"text":"Ready for review."}),"applied");
        apply(json!({"kind":"comment","change_id":change,"version":feature,"text":"Reviewed this version.","reply_to":comment}),"applied");
        apply(json!({"kind":"comment","change_id":change,"version":feature,"path":"test.txt","line":2,"text":"Keep this second line."}),"applied");
        apply(json!({"kind":"ref_move","name":"env/native-test","target":feature,"expected":null}),"applied");
        apply(json!({"kind":"ref_move","name":"env/native-test","target":feature,"expected":null}),"refused");
        let entry = apply(json!({"kind":"queue_add","state":feature,"into":"main"}),"applied");
        apply(json!({"kind":"queue_remove","entry":entry}),"applied");
        drop(apply);
        let comments = session.project("test").unwrap().comments(&change);
        assert_eq!(comments.len(),3);
        assert!(comments.iter().all(|comment|comment["actor"]["name"]=="person:owner@example.com"));
        assert_eq!(comments[2]["line"],2);
        println!("real executor: {count} signed browser requests, 8 applied, 1 refused, 3 folded comments");
    }
}
