//! Indexes are authenticated caches; all history is reconciled against signed sources.
use super::*;
use layr_store::{index as store_index, public};

pub(super) const DISCARDED: &str = "Index discarded. Showing verified records.";
#[derive(Default)]
pub(super) struct Cache {
    pub heads: Option<store_index::Index>,
    candidates: Vec<store_index::Index>,
    pub paths: BTreeMap<String, store_index::Paths>,
    pub graphs: BTreeMap<String, store_index::Graph>,
    pub trusted: BTreeMap<String, VerifyingKey>,
    pub note: Option<String>,
    pub policy_note: Option<String>,
    pub public: Option<public::Manifest>,
}
#[wasm_bindgen]
impl Session {
    /// A public manifest is not a trust root. finish_records verifies its writer.
    pub fn open_public_store(
        &mut self,
        name: &str,
        descriptor: &str,
        manifest: &[u8],
        slot: &[u8],
    ) -> std::result::Result<String, JsValue> {
        self.add_public_store(name, descriptor, manifest, slot)
            .map_err(js)
    }
    pub fn mark_public(&mut self, name: &str, bytes: &[u8]) -> std::result::Result<(), JsValue> {
        let store = self
            .stores
            .get_mut(name)
            .context("store is locked")
            .map_err(js)?;
        let manifest = public::Manifest::parse(bytes, &store.descriptor).map_err(js)?;
        manifest.verify_signer(&store.cache.trusted).map_err(js)?;
        store.cache.public = Some(manifest);
        Ok(())
    }
    pub fn add_heads(
        &mut self,
        name: &str,
        object: &str,
        bytes: &[u8],
    ) -> std::result::Result<(), JsValue> {
        self.cache_heads(name, object, bytes).map_err(js)
    }
    pub fn use_heads(&mut self, name: &str) -> std::result::Result<bool, JsValue> {
        self.select_heads(name).map_err(js)
    }
    pub fn discard_index(&mut self, name: &str) {
        if let Some(store) = self.stores.get_mut(name) {
            store.cache.heads = None;
            store.cache.candidates.clear();
            store.cache.paths.clear();
            store.cache.graphs.clear();
            store.cache.note = Some(DISCARDED.into());
        }
    }
    pub fn add_history_index(
        &mut self,
        name: &str,
        object: &str,
        bytes: &[u8],
    ) -> std::result::Result<(), JsValue> {
        self.cache_history(name, object, bytes).map_err(js)
    }
    pub fn blame_inputs(
        &self,
        name: &str,
        state: &str,
        path: &str,
    ) -> std::result::Result<String, JsValue> {
        self.blame_objects(name, state, path)
            .and_then(encoded)
            .map_err(js)
    }
    /// Called after loading history trees, before selecting file versions to fetch.
    pub fn history_versions(
        &mut self,
        name: &str,
        state: &str,
        path: &str,
    ) -> std::result::Result<String, JsValue> {
        self.prepare_history(name, state)
            .and_then(|_| self.file_history_view(name, state, path, false))
            .and_then(encoded)
            .map_err(js)
    }
}
impl Session {
    fn add_public_store(
        &mut self,
        name: &str,
        descriptor: &str,
        bytes: &[u8],
        slot: &[u8],
    ) -> Result<String> {
        let descriptor: format::Descriptor = serde_json::from_str(descriptor)?;
        let manifest = public::Manifest::parse(bytes, &descriptor)?;
        let identity = Zeroizing::new(manifest.open(slot)?);
        let project = descriptor.project.clone();
        self.stores.insert(
            name.into(),
            Store {
                cache: Cache {
                    public: Some(manifest),
                    ..Default::default()
                },
                authorities: BTreeMap::new(),
                published: BTreeMap::new(),
                parent: None,
                identity,
                descriptor,
                segments: BTreeMap::new(),
                keys: BTreeMap::new(),
                model: Model::default(),
                trees: BTreeMap::new(),
                parts: BTreeMap::new(),
            },
        );
        Ok(project)
    }
    fn cache_heads(&mut self, name: &str, object: &str, bytes: &[u8]) -> Result<()> {
        let store = self.stores.get_mut(name).context("store is locked")?;
        let index = store_index::open(
            &store.identity,
            bytes,
            object,
            &store.descriptor.project,
            &store.cache.trusted,
        )?;
        if !matches!(index.data, store_index::Data::Heads(_)) {
            bail!("expected heads snapshot");
        }
        store.cache.candidates.push(index);
        Ok(())
    }
    fn select_heads(&mut self, name: &str) -> Result<bool> {
        let store = self.stores.get_mut(name).context("store is locked")?;
        store.cache.heads = store_index::newest(&store.cache.candidates)?.cloned();
        store.cache.candidates.clear();
        Ok(store.cache.heads.is_some())
    }
    pub(super) fn index_answer(
        &self,
        name: &str,
        method: &str,
        _args: &Value,
    ) -> Result<Option<Value>> {
        let Some(store) = self.stores.get(name) else {
            return Ok(None);
        };
        let Some(snapshot) = &store.cache.heads else {
            return Ok(None);
        };
        let store_index::Data::Heads(heads) = &snapshot.data else {
            bail!("expected heads snapshot")
        };
        Ok(Some(match method {
            "lines" => json!(heads.lines),
            "refs" => json!(heads.refs),
            "openChanges" => json!(heads.changes),
            "copies" => json!([]),
            // A first page never invents missing state metadata from the index summary.
            "log" | "queue" | "opLog" => json!([]),
            "layers" => json!([]),
            _ => return Ok(None),
        }))
    }
    fn cache_history(&mut self, name: &str, object: &str, bytes: &[u8]) -> Result<()> {
        let store = self.stores.get_mut(name).context("store is locked")?;
        let index = store_index::open(
            &store.identity,
            bytes,
            object,
            &store.descriptor.project,
            &store.cache.trusted,
        )?;
        match index.data {
            store_index::Data::Paths(p) => {
                store.cache.paths.insert(p.state.clone(), p);
            }
            store_index::Data::Graph(g) => {
                store.cache.graphs.insert(g.state.clone(), g);
            }
            _ => bail!("expected file history index"),
        }
        Ok(())
    }
    pub(super) fn history_states(&self, name: &str, start: &str) -> Result<Vec<String>> {
        let p = self.project(name)?;
        let mut at = start;
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        loop {
            if !seen.insert(at.to_string()) {
                bail!("state history has a loop");
            }
            let state = p.state(at).context("state is absent")?;
            out.push(at.to_string());
            let Some(parent) = state.parents.first() else {
                break;
            };
            at = parent;
        }
        Ok(out)
    }
    fn state_tree<'a>(&'a self, name: &str, state: &str) -> Result<&'a format::Tree> {
        self.stores
            .get(name)
            .context("store is locked")?
            .trees
            .iter()
            .find(|(object, _)| format::parse_tree_name(object).is_some_and(|v| v.0 == state))
            .map(|(_, (_, tree))| tree)
            .context("history tree is absent")
    }
    fn prepare_history(&mut self, name: &str, start: &str) -> Result<()> {
        let states = self.history_states(name, start)?;
        let p = self.project(name)?;
        let mut paths = BTreeMap::new();
        let mut graphs = BTreeMap::new();
        let empty = format::Tree {
            files: BTreeMap::new(),
        };
        for id in states.iter().rev() {
            let state = p.state(id).context("state is absent")?;
            let tree = self.state_tree(name, id)?;
            let parent = state
                .parents
                .first()
                .map(|id| self.state_tree(name, id))
                .transpose()?
                .unwrap_or(&empty);
            paths.insert(id.clone(), layr_model::index::paths(id, parent, tree));
            let graph = layr_model::index::graph(state, |id| {
                graphs.get(id).cloned().context("ancestor graph is absent")
            })?;
            graphs.insert(id.clone(), graph);
        }
        let store = self.stores.get_mut(name).context("store is locked")?;
        let mismatch = states.iter().any(|id| {
            store
                .cache
                .paths
                .get(id)
                .is_some_and(|cached| Some(cached) != paths.get(id))
                || store
                    .cache
                    .graphs
                    .get(id)
                    .is_some_and(|cached| Some(cached) != graphs.get(id))
        });
        if mismatch {
            store.cache.note = Some(DISCARDED.into());
        }
        // Use the derived values even when a cache object is missing or wrong.
        store.cache.paths = paths;
        store.cache.graphs = graphs;
        Ok(())
    }
    fn blame_objects(&self, name: &str, start: &str, path: &str) -> Result<Vec<String>> {
        let store = self.stores.get(name).context("store is locked")?;
        let history = layr_model::index::History {
            project: self.project(name)?,
            paths: &store.cache.paths,
            graphs: &store.cache.graphs,
        };
        let mut out = Vec::new();
        let mut held = 0u64;
        let mut previous = None;
        for state in history.file_history(start, path)? {
            let tree = self.state_tree(name, &state.id)?;
            let Some(entry) = tree.files.get(path) else {
                break;
            };
            if entry.0 == "120000" {
                break;
            }
            if entry.1 > 64 << 20 {
                bail!("file is larger than 64 MiB");
            }
            out.push(state.id);
            if previous.as_ref() != Some(&entry.2) {
                held += entry.1;
                previous = Some(entry.2.clone());
            }
            if held > 256 << 20 && out.len() > 1 {
                break;
            }
        }
        Ok(out)
    }
    pub(super) fn file_history_view(
        &self,
        name: &str,
        start: &str,
        path: &str,
        blame: bool,
    ) -> Result<Value> {
        if !format::safe_path(path) {
            bail!("invalid file path");
        }
        let store = self.stores.get(name).context("store is locked")?;
        let history = layr_model::index::History {
            project: self.project(name)?,
            paths: &store.cache.paths,
            graphs: &store.cache.graphs,
        };
        if !blame {
            return Ok(json!(history.file_history(start, path)?));
        }
        let lines = history.blame(start, path, |state, path| {
            let tree = self.state_tree(name, state)?;
            tree.files
                .get(path)
                .map(|entry| reader::file(&store.identity, entry, &store.parts))
                .transpose()
        })?;
        Ok(json!({"lines":lines,"states":history.file_history(start,path)?}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    const PROJECT: &str = "01a12290-0000-7000-8000-000000000001";
    const MACHINE: &str = "01a12290-0000-7000-8000-000000000002";
    fn records(key: &SigningKey) -> Vec<String> {
        [("machine",json!({"public_key":key.verifying_key().to_bytes().iter().map(|b|format!("{b:02x}")).collect::<String>()})),("project",json!({"id":PROJECT,"name":"Indexed"}))].into_iter().enumerate().map(|(i,(op,data))| {
            let body=json!({"id":format!("01a12290-0000-7000-8001-{:012}",i+1),"project":PROJECT,"machine":MACHINE,"seq":i+1,"time":i as i64,"op":op,"actor":{"uid":0,"name":"Reader"},"states":[],"lines":{},"data":data}).to_string();
            format!("{{\"r\":{body},\"s\":\"{}\"}}",keys::sign_bytes(body.as_bytes(),key))
        }).collect()
    }
    fn install(s: &mut Session, vault: &str, lines: &[String]) {
        let object = format::segment_name(MACHINE, 1, lines.len() as u64);
        let cipher = keys::encrypt(
            &keys::recipient_of(vault).unwrap(),
            lines.join("\n").as_bytes(),
        )
        .unwrap();
        s.segment("test", &object, &cipher).unwrap();
        s.finish("test").unwrap();
    }
    #[test]
    #[ignore = "requires LAYR_WEB_INDEX_FIXTURE encrypted store and LAYR_WEB_INDEX_PERSON in VM tmpfs"]
    fn actual_two_writer_indexes_reconcile() {
        use std::{env,fs,path::PathBuf};
        let root=PathBuf::from(env::var("LAYR_WEB_INDEX_FIXTURE").unwrap());
        let person=PathBuf::from(env::var("LAYR_WEB_INDEX_PERSON").unwrap());
        let mut session=Session::new();
        let paper=keys::identity_from_paper(&fs::read_to_string(person.join("recovery.txt")).unwrap()).unwrap();
        session.unlock(&paper,"paper",&fs::read_to_string(person.join("person-view.json")).unwrap()).unwrap();
        let slot=person::slot_name(&session.login,&keys::recipient_of(session.person.as_ref().unwrap()).unwrap()).unwrap();
        session.add_store("test",&fs::read_to_string(root.join("store.json")).unwrap(),&fs::read(root.join("slots").join(slot)).unwrap()).unwrap();
        let load=|session:&mut Session| {
            for machine in fs::read_dir(root.join("records")).unwrap() {
                for entry in fs::read_dir(machine.unwrap().path()).unwrap() {
                    let path=entry.unwrap().path();
                    let name=path.strip_prefix(&root).unwrap().to_str().unwrap();
                    session.segment("test",name,&fs::read(&path).unwrap()).unwrap();
                }
            }
            session.finish("test").unwrap();
        };
        load(&mut session);
        assert!(session.stores["test"].keys.len()>=2);
        for machine in fs::read_dir(root.join("index/heads")).unwrap() {
            let mut entries=fs::read_dir(machine.unwrap().path()).unwrap().map(|entry|entry.unwrap().path()).collect::<Vec<_>>();
            entries.sort();let path=entries.last().unwrap();let name=path.strip_prefix(&root).unwrap().to_str().unwrap();
            session.cache_heads("test",name,&fs::read(path).unwrap()).unwrap_or_else(|error|panic!("index open {name}: {error:#}"));
        }
        assert!(session.select_heads("test").unwrap());
        let snapshot=session.stores["test"].cache.heads.clone().unwrap();
        let mut lines=Vec::new();
        for machine in fs::read_dir(root.join("records")).unwrap() {
            for entry in fs::read_dir(machine.unwrap().path()).unwrap() {
                let bytes=fs::read(entry.unwrap().path()).unwrap();
                let text=String::from_utf8(keys::decrypt(&session.stores["test"].identity,&bytes).unwrap()).unwrap();
                lines.extend(text.lines().map(String::from));
            }
        }
        layr_model::index::reconcile(&snapshot,&lines).unwrap_or_else(|error|panic!("index reconcile: {error:#}"));
        load(&mut session);
        assert!(session.stores["test"].cache.note.is_none());
    }
    #[test]
    fn provisional_heads_are_reconciled_and_signed_lies_fall_back_to_records() {
        for tamper in ["none", "count", "watermark", "head"] {
            let (mut s, vault, _, key) = crate::tests::fixture();
            let lines = records(&key);
            install(&mut s, &vault, &lines);
            let p = s.project("test").unwrap();
            let mut heads =
                layr_model::index::heads(p, layr_model::index::watermark(&lines).unwrap(), 1)
                    .unwrap();
            if tamper == "count" {
                heads.counts.states += 1;
            }
            if tamper == "watermark" {
                heads.watermark.get_mut(MACHINE).unwrap().hash = "0".repeat(64);
            }
            if tamper == "head" {
                heads.lines.push(store_index::Line {
                    name: "main".into(),
                    id: PROJECT.into(),
                    owner: 0,
                    machine: MACHINE.into(),
                    head: MACHINE.into(),
                });
            }
            let snapshot = store_index::Index {
                v: 1,
                project: PROJECT.into(),
                machine: MACHINE.into(),
                data: store_index::Data::Heads(heads),
            };
            let cipher = snapshot
                .seal(&keys::recipient_of(&vault).unwrap(), &key)
                .unwrap();
            s.cache_heads("test", &snapshot.name(), &cipher).unwrap();
            assert!(s.select_heads("test").unwrap());
            assert_eq!(s.project_view("test", 0).unwrap()["provisional"], true);
            install(&mut s, &vault, &lines);
            assert!(s.stores["test"].cache.heads.is_none());
            assert_eq!(s.stores["test"].cache.note.is_some(), tamper != "none");
            assert_eq!(s.project_view("test", 0).unwrap()["name"], "Indexed");
        }
    }
    #[test]
    fn unknown_index_signer_is_refused_before_provisional_data() {
        let (mut s, vault, _, key) = crate::tests::fixture();
        let lines = records(&key);
        install(&mut s, &vault, &lines);
        let heads = layr_model::index::heads(
            s.project("test").unwrap(),
            layr_model::index::watermark(&lines).unwrap(),
            1,
        )
        .unwrap();
        let snapshot = store_index::Index {
            v: 1,
            project: PROJECT.into(),
            machine: MACHINE.into(),
            data: store_index::Data::Heads(heads),
        };
        let cipher = snapshot
            .seal(
                &keys::recipient_of(&vault).unwrap(),
                &SigningKey::from_bytes(&[8; 32]),
            )
            .unwrap();
        assert!(s.cache_heads("test", &snapshot.name(), &cipher).is_err());
        assert!(s.stores["test"].cache.heads.is_none());
    }
    #[test]
    fn anonymous_reader_requires_manifest_writer_trust_from_records() {
        let (_, vault, _, key) = crate::tests::fixture();
        let descriptor = format::Descriptor {
            format: 2,
            project: PROJECT.into(),
            recipient: keys::recipient_of(&vault).unwrap(),
        };
        let opener = keys::generate_identity();
        let slot = keys::seal_slot(&vault, &keys::recipient_of(&opener).unwrap()).unwrap();
        let other = SigningKey::from_bytes(&[8; 32]);
        for trusted in [true, false] {
            let manifest = public::Manifest::new(
                &descriptor,
                opener.clone(),
                MACHINE.into(),
                if trusted { &key } else { &other },
            )
            .unwrap();
            let mut s = Session::new();
            s.add_public_store(
                "test",
                &serde_json::to_string(&descriptor).unwrap(),
                &serde_json::to_vec(&manifest).unwrap(),
                &slot,
            )
            .unwrap();
            let lines = records(&key);
            let object = format::segment_name(MACHINE, 1, 2);
            let cipher = keys::encrypt(&descriptor.recipient, lines.join("\n").as_bytes()).unwrap();
            s.segment("test", &object, &cipher).unwrap();
            assert_eq!(s.finish("test").is_ok(), trusted);
            if trusted {
                assert_eq!(
                    s.answer("", "projects", &json!([]), 0).unwrap()[0]["public"],
                    true
                );
                assert_eq!(s.answer("", "you", &json!([]), 0).unwrap()["login"], "");
            }
        }
    }
}
