//! Browser adapter. Keys and decrypted model data stay in the worker that owns this object.
#![forbid(unsafe_code)]
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::VerifyingKey;
use layr_model::{probes, Model, Project, Record, Subject};
use layr_store::{format, keys, person, reader, record};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use wasm_bindgen::prelude::*;
use zeroize::Zeroizing;
mod changes;
mod metadata;
mod screens;
mod overlays;

fn js(error: anyhow::Error) -> JsValue {
    JsValue::from_str(&format!("{error:#}"))
}
fn encoded(value: impl serde::Serialize) -> Result<String> {
    Ok(serde_json::to_string(&value)?)
}
fn unb64(value: &str) -> Result<Vec<u8>> {
    Ok(B64.decode(value)?)
}

struct Store {
    authorities: BTreeMap<String, layr_model::authority::Authority>,
    published: BTreeMap<String, overlays::Published>,
    parent: Option<overlays::Parent>,
    identity: Zeroizing<String>,
    descriptor: format::Descriptor,
    segments: BTreeMap<String, Zeroizing<String>>,
    keys: BTreeMap<String, VerifyingKey>,
    model: Model,
    trees: BTreeMap<String, (Vec<u8>, format::Tree)>,
    parts: BTreeMap<String, Vec<u8>>,
}

#[wasm_bindgen]
pub struct Session {
    person: Option<Zeroizing<String>>,
    login: String,
    index: person::Index,
    invites: Vec<person::Pin>,
    invite_errors: Vec<Value>,
    stores: BTreeMap<String, Store>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            person: None,
            login: String::new(),
            index: person::Index::default(),
            invites: Vec::new(),
            invite_errors: Vec::new(),
            stores: BTreeMap::new(),
        }
    }
}
#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn unlock_recovery(&mut self, words: &str, view: &str) -> std::result::Result<(), JsValue> {
        let opener = Zeroizing::new(keys::identity_from_paper(words).map_err(js)?);
        self.unlock(&opener, "paper", view).map_err(js)
    }
    pub fn unlock_prf(
        &mut self,
        secret: &[u8],
        slot: &str,
        view: &str,
    ) -> std::result::Result<(), JsValue> {
        let opener = Zeroizing::new(keys::identity_from_secret(secret).map_err(js)?);
        self.unlock(&opener, slot, view).map_err(js)
    }
    pub fn lock(&mut self) {
        *self = Self::default();
    }
    pub fn slot_name(&self) -> std::result::Result<String, JsValue> {
        person::slot_name(
            &self.login,
            &keys::recipient_of(self.person.as_ref().context("locked").map_err(js)?).map_err(js)?,
        )
        .map_err(js)
    }
    /// A pin can come only from the authenticated index or an opened invite.
    pub fn open_store(
        &mut self,
        name: &str,
        descriptor: &str,
        slot: &[u8],
    ) -> std::result::Result<String, JsValue> {
        self.add_store(name, descriptor, slot).map_err(js)
    }
    pub fn challenge(&self, name: &str, ciphertext: &[u8]) -> std::result::Result<String, JsValue> {
        let store = self
            .stores
            .get(name)
            .context("store is locked")
            .map_err(js)?;
        String::from_utf8(keys::decrypt(&store.identity, ciphertext).map_err(js)?)
            .map_err(|e| js(e.into()))
    }
    pub fn add_segment(
        &mut self,
        name: &str,
        object: &str,
        ciphertext: &[u8],
    ) -> std::result::Result<(), JsValue> {
        self.segment(name, object, ciphertext).map_err(js)
    }
    pub fn finish_records(&mut self, name: &str) -> std::result::Result<(), JsValue> {
        self.finish(name).map_err(js)
    }
    /// The unverified header selects dependencies only. install_tree verifies all content.
    pub fn tree_base(
        &self,
        name: &str,
        object: &str,
        ciphertext: &[u8],
    ) -> std::result::Result<String, JsValue> {
        self.base(name, object, ciphertext)
            .and_then(encoded)
            .map_err(js)
    }
    pub fn install_tree(
        &mut self,
        name: &str,
        object: &str,
        ciphertext: &[u8],
    ) -> std::result::Result<(), JsValue> {
        self.install(name, object, ciphertext).map_err(js)
    }
    pub fn file_parts(
        &self,
        name: &str,
        tree: &str,
        path: &str,
    ) -> std::result::Result<String, JsValue> {
        self.parts_for(name, tree, path)
            .and_then(encoded)
            .map_err(js)
    }
    pub fn add_part(
        &mut self,
        name: &str,
        object: &str,
        ciphertext: &[u8],
    ) -> std::result::Result<(), JsValue> {
        if format::parse_data_name(object).is_none() {
            return Err(js(anyhow::anyhow!("invalid data object")));
        }
        self.stores
            .get_mut(name)
            .context("store is locked")
            .map_err(js)?
            .parts
            .insert(object.into(), ciphertext.to_vec());
        Ok(())
    }
    pub fn tree(
        &self,
        name: &str,
        object: &str,
        path: &str,
    ) -> std::result::Result<String, JsValue> {
        self.tree_view(name, object, path)
            .and_then(encoded)
            .map_err(js)
    }
    pub fn file(
        &self,
        name: &str,
        object: &str,
        path: &str,
    ) -> std::result::Result<String, JsValue> {
        self.file_view(name, object, path)
            .and_then(encoded)
            .map_err(js)
    }
    pub fn diff(
        &self,
        name: &str,
        a: &str,
        b: &str,
        path: &str,
    ) -> std::result::Result<String, JsValue> {
        self.diff_view(name, a, b, path)
            .and_then(encoded)
            .map_err(js)
    }
    pub fn query(
        &self,
        name: &str,
        method: &str,
        args: &str,
        now: f64,
    ) -> std::result::Result<String, JsValue> {
        self.answer(
            name,
            method,
            &serde_json::from_str(args).map_err(|e| js(e.into()))?,
            now as i64,
        )
        .and_then(encoded)
        .map_err(js)
    }
}
impl Session {
    fn unlock(&mut self, opener: &str, slot_name: &str, view: &str) -> Result<()> {
        self.lock();
        let view: Value = serde_json::from_str(view)?;
        let login = view["login"].as_str().context("person has no login")?;
        if !person::login_name(login) {
            bail!("invalid login");
        }
        let slot = view["slots"]
            .as_array()
            .context("no person slots")?
            .iter()
            .find(|s| s["name"] == slot_name)
            .context("person slot is absent")?;
        let identity = Zeroizing::new(keys::open_slot(
            &unb64(slot["data"].as_str().context("invalid slot")?)?,
            opener,
        )?);
        if Some(keys::recipient_of(&identity)?.as_str()) != view["recipient"].as_str() {
            bail!("person recipient differs from slot");
        }
        let index = match view["index"].as_str() {
            Some(data) => person::open_index(&identity, &unb64(data)?)?,
            None => person::Index::default(),
        };
        // Pending invites do not replace the authenticated index or authorize a store.
        let mut invites = Vec::new();
        let mut invite_errors = Vec::new();
        for invite in view["inbox"].as_array().into_iter().flatten() {
            let opened = (|| -> Result<person::Pin> {
                person::open_invite(
                    &identity,
                    &unb64(invite["data"].as_str().context("invalid invite")?)?,
                )
            })();
            match opened {
                Ok(pin) => invites.push(pin),
                Err(error) => {
                    invite_errors.push(json!({"id":invite["id"],"reason":format!("{error:#}")}))
                }
            }
        }
        self.invites = invites;
        self.invite_errors = invite_errors;
        self.person = Some(identity);
        self.login = login.into();
        self.index = index;
        Ok(())
    }
    fn add_store(&mut self, name: &str, descriptor: &str, slot: &[u8]) -> Result<String> {
        let descriptor: format::Descriptor = serde_json::from_str(descriptor)?;
        if descriptor.format != 2 || !format::is_uuid(&descriptor.project) {
            bail!("invalid store descriptor");
        }
        // Index names are gateway aliases. v2 descriptors contain the signed project UUID.
        let pin = self
            .index
            .pin(&descriptor.project)
            .context("store has no person pin")?;
        pin.check_store(&descriptor)?;
        let identity = Zeroizing::new(keys::open_slot(
            slot,
            self.person.as_ref().context("locked")?,
        )?);
        if keys::recipient_of(&identity)? != descriptor.recipient {
            bail!("store slot opens another recipient");
        }
        let project = descriptor.project.clone();
        self.stores.insert(
            name.into(),
            Store {
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
    fn segment(&mut self, name: &str, object: &str, ciphertext: &[u8]) -> Result<()> {
        format::parse_segment_name(object).context("invalid record segment name")?;
        let store = self.stores.get_mut(name).context("store is locked")?;
        let text = Zeroizing::new(String::from_utf8(keys::decrypt(
            &store.identity,
            ciphertext,
        )?)?);
        if let Some(old) = store.segments.get(object) {
            if old.as_str() != text.as_str() {
                bail!("record segment changed");
            }
        }
        store.segments.insert(object.into(), text);
        Ok(())
    }
    fn finish(&mut self, name: &str) -> Result<()> {
        self.prepare_hidden_records(name)?;
        let parent_keys = self
            .stores
            .get(name)
            .and_then(|s| s.parent.as_ref())
            .and_then(|(parent, _, _)| self.stores.get(parent))
            .map(|s| s.keys.clone());
        let store = self.stores.get_mut(name).context("store is locked")?;
        let mut machines: BTreeMap<String, BTreeMap<u64, &str>> = BTreeMap::new();
        for (object, text) in &store.segments {
            let (machine, first, last) =
                format::parse_segment_name(object).context("invalid segment")?;
            let mut expected = first;
            for line in text.lines() {
                let raw = record::parse_line(line, None)?;
                let r: Record = serde_json::from_str(raw.get())?;
                if r.project != store.descriptor.project
                    || r.machine != machine
                    || r.seq != expected
                    || expected > last
                {
                    bail!("record segment has an invalid project or sequence");
                }
                let by_seq = machines.entry(machine.clone()).or_default();
                if let Some(old) = by_seq.insert(expected, line) {
                    if old != line {
                        bail!("conflicting record sequence");
                    }
                }
                expected = expected.checked_add(1).context("sequence overflow")?;
            }
            if expected - 1 != last {
                bail!("incomplete record segment");
            }
        }
        let mut verified = Vec::new();
        let mut machine_keys = BTreeMap::new();
        for (machine, lines) in machines {
            let first = lines.get(&1).context("machine has no first record")?;
            let raw = record::parse_line(first, None)?;
            let r: Record = serde_json::from_str(raw.get())?;
            if r.op != "machine" {
                bail!("first record is not a machine announcement");
            }
            let key = record::machine_key(raw)?;
            if parent_keys
                .as_ref()
                .is_some_and(|keys| keys.get(&machine) != Some(&key))
            {
                bail!("overlay signer is not in the parent store");
            }
            for (index, (seq, line)) in lines.iter().enumerate() {
                if *seq != index as u64 + 1 {
                    bail!("record history has a gap");
                }
                record::verify_line(line.as_bytes(), &key)?;
                verified.push(*line);
            }
            machine_keys.insert(machine, key);
        }
        let exact: Vec<String> = verified.iter().map(|line| (*line).to_string()).collect();
        let authorities = if let Some((parent_name, published, authority)) = &store.parent {
            let _ = parent_name;
            if !exact.contains(&published.line) {
                bail!("published overlay key is absent from its log");
            }
            authority.validate(&exact, &published.key.overlay, &machine_keys)?;
            BTreeMap::new()
        } else {
            layr_model::authority::from_lines(&store.descriptor.project, &exact, &machine_keys)?
        };
        let perspective = if machine_keys.len() == 1 {
            machine_keys.keys().next().map(String::as_str).unwrap_or("")
        } else {
            ""
        };
        let model = Model::from_verified_lines(verified, perspective)?;
        store.authorities = authorities;
        store.keys = machine_keys;
        store.model = model;
        // The model is the retained metadata. Discard the decrypted segment copies.
        store.segments.clear();
        Ok(())
    }
    fn base(&self, name: &str, object: &str, ciphertext: &[u8]) -> Result<Option<String>> {
        let store = self.stores.get(name).context("store is locked")?;
        let plain = keys::decrypt(&store.identity, ciphertext)?;
        let (header, _) = format::split_tree(&plain)?;
        if format::parse_tree_name(object) != Some((header.state.clone(), header.attempt.clone())) {
            bail!("tree object name differs from header");
        }
        Ok(header
            .base
            .map(|(state, attempt)| format::tree_name(&state, &attempt)))
    }
    fn install(&mut self, name: &str, object: &str, ciphertext: &[u8]) -> Result<()> {
        let store = self.stores.get_mut(name).context("store is locked")?;
        let plain = keys::decrypt(&store.identity, ciphertext)?;
        let (header, _) = format::split_tree(&plain)?;
        if format::parse_tree_name(object) != Some((header.state.clone(), header.attempt.clone())) {
            bail!("tree object name differs from header");
        }
        let key = store
            .keys
            .get(&header.machine)
            .context("tree signer has no verified machine key")?;
        let base = header
            .base
            .as_ref()
            .map(|(s, a)| {
                store
                    .trees
                    .get(&format::tree_name(s, a))
                    .map(|t| t.0.as_slice())
                    .context("base tree is absent")
            })
            .transpose()?;
        let (_, canonical, tree) = reader::tree(&store.identity, ciphertext, base, key)?;
        store.trees.insert(object.into(), (canonical, tree));
        Ok(())
    }
    fn parts_for(&self, name: &str, object: &str, path: &str) -> Result<Vec<String>> {
        let store = self.stores.get(name).context("store is locked")?;
        let tree = &store.trees.get(object).context("tree is absent")?.1;
        let entry = tree.files.get(path).context("file is absent")?;
        Ok(entry
            .3
            .iter()
            .map(|link| link.object())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
    fn tree_view(&self, name: &str, object: &str, path: &str) -> Result<Value> {
        let files = self.view_files(name, object)?;
        if !path.is_empty() && !format::safe_path(path) {
            bail!("invalid directory path");
        }
        if files.contains_key(path) {
            bail!("path is a file");
        }
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        let mut entries = BTreeMap::new();
        for (file, (_, _, entry, overlay)) in &files {
            let Some(rest) = file.strip_prefix(&prefix) else {
                continue;
            };
            let first = rest.split('/').next().context("empty tree path")?;
            let full = format!("{prefix}{first}");
            entries.entry(first).or_insert_with(|| {
                let mut row = if rest.contains('/') {
                    json!({"name":first,"path":full,"kind":"directory"})
                } else {
                    json!({"name":first,"path":full,"kind":if entry.0=="120000" {"symlink"}else{"file"},"size":entry.1})
                };
                if let Some(overlay) = overlay {
                    row["overlay"] = json!(overlay);
                }
                row
            });
        }
        if !path.is_empty() && entries.is_empty() {
            bail!("directory is absent");
        }
        let hidden = self
            .overlay_views(name, 0)
            .unwrap_or(json!([]))
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["readable"] != true)
            .filter_map(|v| v["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        Ok(
            json!({"state":format::parse_tree_name(object).context("invalid tree")?.0,"path":path,"entries":entries.into_values().collect::<Vec<_>>(),"hidden_overlays":hidden}),
        )
    }
    fn read_file(&self, name: &str, object: &str, path: &str) -> Result<Vec<u8>> {
        let files = self.view_files(name, object)?;
        let (source, _, entry, _) = files.get(path).context("file is absent")?;
        let store = self.stores.get(source).context("store is locked")?;
        reader::file(&store.identity, entry, &store.parts)
    }
    fn file_view(&self, name: &str, object: &str, path: &str) -> Result<Value> {
        let data = self.read_file(name, object, path)?;
        let mut out = json!({"state":format::parse_tree_name(object).context("invalid tree")?.0,"path":path,"size":data.len()});
        if !data.contains(&0) {
            if let Ok(text) = std::str::from_utf8(&data) {
                out["text"] = json!(text);
                return Ok(out);
            }
        }
        out["base64"] = json!(B64.encode(data));
        Ok(out)
    }
    fn diff_view(&self, name: &str, a: &str, b: &str, path: &str) -> Result<Value> {
        let left = self.view_files(name, a)?;
        let right = self.view_files(name, b)?;
        let mut files = Vec::new();
        for p in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
            if !path.is_empty() && p != path && !p.starts_with(&format!("{path}/")) {
                continue;
            }
            if left
                .get(p)
                .zip(right.get(p))
                .is_some_and(|(a, b)| a.2 .0 == b.2 .0 && a.2 .1 == b.2 .1 && a.2 .2 == b.2 .2)
            {
                continue;
            }
            if path.is_empty() || p != path {
                let mut row = json!({"path":p});
                if let Some(overlay) = right
                    .get(p)
                    .or_else(|| left.get(p))
                    .and_then(|v| v.3.as_ref())
                {
                    row["overlay"] = json!(overlay);
                }
                files.push(row);
                continue;
            }
            let before = if left.contains_key(p) {
                Some(self.read_file(name, a, p)?)
            } else {
                None
            };
            let after = if right.contains_key(p) {
                Some(self.read_file(name, b, p)?)
            } else {
                None
            };
            fn text(v: &Option<Vec<u8>>) -> Option<&str> {
                v.as_ref().and_then(|v| {
                    if v.contains(&0) {
                        None
                    } else {
                        std::str::from_utf8(v).ok()
                    }
                })
            }
            let bt = text(&before);
            let at = text(&after);
            let diff = similar::TextDiff::from_lines(bt.unwrap_or(""), at.unwrap_or(""));
            let added = diff
                .iter_all_changes()
                .filter(|c| c.tag() == similar::ChangeTag::Insert)
                .count();
            let removed = diff
                .iter_all_changes()
                .filter(|c| c.tag() == similar::ChangeTag::Delete)
                .count();
            let mut row = json!({"path":p,"added":added,"removed":removed});
            if before.is_some() && bt.is_none() || after.is_some() && at.is_none() {
                row["binary"] = json!(true);
            }
            if let Some(text) = bt {
                row["before"] = json!(text);
            }
            if let Some(text) = at {
                row["after"] = json!(text);
            }
            files.push(row);
        }
        Ok(
            json!({"base":format::parse_tree_name(a).context("invalid tree")?.0,"head":format::parse_tree_name(b).context("invalid tree")?.0,"files":files}),
        )
    }
    fn project(&self, name: &str) -> Result<&Project> {
        let store = self.stores.get(name).context("store is locked")?;
        store
            .model
            .projects
            .get(&store.descriptor.project)
            .context("project has no verified records")
    }
    fn answer(&self, name: &str, method: &str, args: &Value, now: i64) -> Result<Value> {
        self.person.as_ref().context("locked")?;
        if method == "layers" {
            return Ok(json!(
                self.layers(name, args[0].as_str().context("state is absent")?)?
            ));
        }
        if method == "pins" {
            return Ok(json!(self
                .index
                .vaults
                .iter()
                .map(|pin| &pin.project)
                .collect::<Vec<_>>()));
        }
        if method == "projects" {
            return Ok(json!(self
                .stores
                .iter()
                .filter(|(_, s)| s.parent.is_none()
                    && s.model.projects.contains_key(&s.descriptor.project))
                .map(|(name, _)| self.project_view(name, now))
                .collect::<Result<Vec<_>>>()?));
        }
        if method == "you" {
            return Ok(
                json!({"login":self.login,"recipient":keys::recipient_of(self.person.as_ref().context("locked")?)?,"invites":self.invites.iter().map(|pin|json!({"project":pin.project})).collect::<Vec<_>>(),"invite_errors":self.invite_errors,"disk":self.stores.iter().filter(|(_,s)|s.parent.is_none()).map(|(n,_)|json!({"store":n,"locked":false})).collect::<Vec<_>>()}),
            );
        }
        if method == "people" {
            return Ok(
                json!([{"login":self.login,"recipient":keys::recipient_of(self.person.as_ref().context("locked")?)?}]),
            );
        }
        if method == "stores" {
            return Ok(json!(self
                .stores
                .iter()
                .filter(|(_, s)| s.parent.is_none())
                .map(|(name, s)| json!({"id":name,"project":s.descriptor.project}))
                .collect::<Vec<_>>()));
        }
        if method == "machines" {
            let mut rows = BTreeMap::new();
            for s in self.stores.values() {
                for p in s.model.projects.values() {
                    for (id, v) in &p.machines {
                        let mut row = json!({"id":id});
                        if let Some(name) = v["name"].as_str().or(v["hostname"].as_str()) {
                            row["name"] = json!(name);
                        }
                        for field in ["os", "backend"] {
                            if let Some(value) = v[field].as_str() {
                                row[field] = json!(value);
                            }
                        }
                        rows.insert(id.clone(), row);
                    }
                }
            }
            return Ok(json!(rows.into_values().collect::<Vec<_>>()));
        }
        let p = self.project(name)?;
        let arg = |i: usize| args[i].as_str().unwrap_or("");
        let mut context = probes::Context {
            now,
            ..Default::default()
        };
        let store = self.stores.get(name).context("store is locked")?;
        for (object, (_, tree)) in &store.trees {
            let state = format::parse_tree_name(object).context("invalid tree")?.0;
            context.sources.insert(state.clone(), "available".into());
            if p.state(&state)
                .is_some_and(|s| s.kind == layr_model::StateKind::Derived)
            {
                let files = tree
                    .files
                    .iter()
                    .map(|(path, entry)| {
                        Ok((
                            path.clone(),
                            reader::file(&store.identity, entry, &store.parts)?,
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>>>();
                if let Ok(files) = files {
                    context.outputs.insert(state.clone(), files);
                }
            }
            if let Some(entry) = tree.files.get("layr.toml") {
                if let Ok(bytes) = reader::file(&store.identity, entry, &store.parts) {
                    #[derive(serde::Deserialize)]
                    struct Manifest {
                        #[serde(default)]
                        probe: BTreeMap<String, probes::Probe>,
                    }
                    let manifest: Manifest = toml::from_str(std::str::from_utf8(&bytes)?)?;
                    context.definitions.insert(state, manifest.probe);
                }
            }
        }
        Ok(match method {
            "changeInputs" => self.change_inputs(name, arg(0), arg(1), arg(2))?,
            "changeDiff" => self.change_diff(name, arg(0), arg(1), arg(2), arg(3))?,
            "mergeStatus" => {
                let base = p
                    .line_list()
                    .into_iter()
                    .find(|l| l.name == arg(1) || l.id == arg(1))
                    .map(|l| l.head)
                    .or_else(|| {
                        p.ref_list(arg(1), now)
                            .into_iter()
                            .find(|r| r.name == arg(1))
                            .map(|r| r.target)
                    });
                let mut changed_paths = Vec::new();
                if let Some(base) = base {
                    let find = |state: &str| {
                        store.trees.keys().find(|object| {
                            format::parse_tree_name(object).is_some_and(|v| v.0 == state)
                        })
                    };
                    if let (Some(a), Some(b)) = (find(&base), find(arg(0))) {
                        let left = self.view_files(name, a)?;
                        let right = self.view_files(name, b)?;
                        for path in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
                            if !left.get(path).zip(right.get(path)).is_some_and(|(a, b)| {
                                a.2 .0 == b.2 .0 && a.2 .1 == b.2 .1 && a.2 .2 == b.2 .2
                            }) {
                                changed_paths.push(path.clone());
                            }
                        }
                    } else {
                        bail!("merge path requirements need both verified source trees");
                    }
                }
                json!(p.merge_status(
                    arg(0),
                    arg(1),
                    &layr_model::protection::Context {
                        now,
                        actor: None,
                        approver_groups: BTreeMap::new(),
                        changed_paths,
                        candidate: None
                    }
                )?)
            }
            "mergeInputs" => {
                json!({"state":arg(0),"base":p.line_list().into_iter().find(|l|l.name==arg(1)||l.id==arg(1)).map(|l|l.head).or_else(||p.ref_list(arg(1),now).into_iter().find(|r|r.name==arg(1)).map(|r|r.target))})
            }
            "probeInputs" => {
                let mut subjects = BTreeSet::new();
                if matches!(arg(0), "probeDiff" | "screens") {
                    subjects.insert(arg(1).to_string());
                    subjects.insert(arg(2).to_string());
                } else {
                    let ids = if arg(0) == "changes" {
                        p.changes()
                    } else {
                        vec![arg(1).to_string()]
                    };
                    for id in ids {
                        if let Some(state) = p.change_versions(&id).last() {
                            subjects.insert(state.id.clone());
                            let line = state.line.as_ref().and_then(|id| p.lines.get(id));
                            let upstream = line
                                .and_then(|l| p.setting(&format!("line.{}.upstream", l.line.name)))
                                .and_then(Value::as_str)
                                .unwrap_or("main");
                            if let Some(target) =
                                p.line_list().into_iter().find(|l| l.name == upstream)
                            {
                                if let Some(base) = p.merge_base(&target.head, &state.id) {
                                    subjects.insert(base);
                                }
                            }
                        }
                    }
                }
                let sources = subjects.clone();
                for r in p.op_log(None, usize::MAX) {
                    if r.op == "view.save"
                        && r.data["view"]["shared"]
                            .as_str()
                            .is_some_and(|id| sources.contains(id))
                    {
                        if let Some(id) = r.data["view_id"].as_str() {
                            subjects.insert(id.into());
                        }
                    }
                }
                let mut outputs = BTreeSet::new();
                for subject in subjects {
                    for result in p.probe_results(&subject) {
                        let (Some(name), Some(output)) =
                            (result["probe"].as_str(), result["output_state"].as_str())
                        else {
                            continue;
                        };
                        let reference = format!("derived/probe-v1/{name}/{subject}");
                        if p.ref_list(&reference, now)
                            .iter()
                            .any(|r| r.name == reference && r.target == output)
                        {
                            outputs.insert(output.to_string());
                        }
                    }
                }
                json!({"sources":sources,"outputs":outputs})
            }
            "lines" => serde_json::to_value(p.line_list())?,
            "log" => {
                let l = p
                    .line_list()
                    .into_iter()
                    .find(|l| l.id == arg(0) || l.name == arg(0));
                let start = match &l {
                    Some(l) => l.head.clone(),
                    None => p
                        .state(arg(0))
                        .context("line or state is absent")?
                        .id
                        .clone(),
                };
                let offset = args[1]["offset"].as_u64().unwrap_or(0) as usize;
                let limit = args[1]["limit"].as_u64().unwrap_or(100) as usize;
                json!(p
                    .log(
                        &[start],
                        if args[1]["auto"] == true {
                            l.as_ref().map(|l| l.id.as_str())
                        } else {
                            None
                        },
                        limit.saturating_add(offset)
                    )
                    .into_iter()
                    .skip(offset)
                    .collect::<Vec<_>>())
            }
            "changes" => json!(p
                .changes()
                .into_iter()
                .map(|id| p.change_show(&id, &context))
                .collect::<Result<Vec<_>>>()?),
            "changeShow" => p.change_show(arg(0), &context)?,
            "refs" => json!(p.ref_list("", now)),
            "refLog" => json!(p.ref_log(arg(0))),
            "queue" => json!(p.queue_list(None)),
            "attestations" => json!(p.attestations(arg(0), None)),
            "opLog" => json!(p.op_log(None, usize::MAX)),
            "probeDiff" => json!(probes::diff(
                p,
                &Subject {
                    kind: "state".into(),
                    id: arg(0).into()
                },
                &Subject {
                    kind: "state".into(),
                    id: arg(1).into()
                },
                &context
            )?),
            "copies" => json!(p.copies()),
            "screens" => screens::read(p, &context, arg(0), arg(1))?,
            _ => bail!("unknown model query {method}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PROJECT: &str = "01a12290-0000-7000-8000-000000000001";
    const MACHINE: &str = "01a12290-0000-7000-8000-000000000002";
    pub(super) fn fixture() -> (Session, String, Vec<u8>, ed25519_dalek::SigningKey) {
        let person_identity = keys::generate_identity();
        let recovery = keys::generate_identity();
        let vault = keys::generate_identity();
        let recipient = keys::recipient_of(&person_identity).unwrap();
        let descriptor = format::Descriptor {
            format: 2,
            project: PROJECT.into(),
            recipient: keys::recipient_of(&vault).unwrap(),
        };
        let index = person::Index {
            vaults: vec![person::Pin::of_store(&descriptor)],
        };
        let view = json!({"login":"owner@example.com","recipient":recipient,"slots":[{"name":"paper","data":B64.encode(keys::seal_slot(&person_identity,&keys::recipient_of(&recovery).unwrap()).unwrap())}],"index":B64.encode(person::seal_index(&person_identity,&index).unwrap()),"inbox":[]});
        let mut session = Session::new();
        session
            .unlock(&recovery, "paper", &view.to_string())
            .unwrap();
        session
            .add_store(
                "test",
                &serde_json::to_string(&descriptor).unwrap(),
                &keys::seal_slot(&vault, &recipient).unwrap(),
            )
            .unwrap();
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        (session, vault, serde_json::to_vec(&view).unwrap(), key)
    }
    fn signed(key: &ed25519_dalek::SigningKey, seq: u64, op: &str, data: Value) -> String {
        let r = json!({"id":format!("01a12290-0000-7000-8000-{seq:012}"),"machine":MACHINE,"project":PROJECT,"seq":seq,"time":seq as i64,"op":op,"actor":{"uid":0,"name":"test"},"states":[],"lines":{},"data":data});
        let body = r.to_string();
        format!(
            "{{\"r\":{body},\"s\":\"{}\"}}",
            keys::sign_bytes(body.as_bytes(), key)
        )
    }
    #[test]
    fn recovery_and_prf_use_existing_store_keys() {
        let (mut s, _, _, _) = fixture();
        assert_eq!(s.login, "owner@example.com");
        assert!(s
            .slot_name()
            .unwrap()
            .starts_with("person-owner@example.com-"));
        let secret = [4u8; 32];
        let opener = keys::identity_from_secret(&secret).unwrap();
        let identity = keys::generate_identity();
        let view = json!({"login":"owner@example.com","recipient":keys::recipient_of(&identity).unwrap(),"slots":[{"name":"passkey-test","data":B64.encode(keys::seal_slot(&identity,&keys::recipient_of(&opener).unwrap()).unwrap())}],"index":null,"inbox":[]});
        s.unlock(&opener, "passkey-test", &view.to_string())
            .unwrap();
        assert_eq!(s.person.as_ref().unwrap().as_str(), identity);
        s.lock();
        assert!(s.person.is_none());
        assert!(s.stores.is_empty());
    }
    #[test]
    fn inbox_invites_stay_pending_and_bad_invites_do_not_block_the_index() {
        let (mut session, _, view, _) = fixture();
        let identity = session.person.as_ref().unwrap().as_str().to_string();
        let opener = keys::identity_from_secret(&[9; 32]).unwrap();
        let mut view: Value = serde_json::from_slice(&view).unwrap();
        view["slots"] = json!([{"name":"passkey-test","data":B64.encode(keys::seal_slot(&identity,&keys::recipient_of(&opener).unwrap()).unwrap())}]);
        let other = keys::generate_identity();
        let descriptor = format::Descriptor {
            format: 2,
            project: MACHINE.into(),
            recipient: keys::recipient_of(&other).unwrap(),
        };
        let pin = person::Pin::of_store(&descriptor);
        view["inbox"] = json!([{"id":"valid","data":B64.encode(person::seal_invite(&keys::recipient_of(&identity).unwrap(),&pin).unwrap())},{"id":"bad","data":"%%%"}]);
        session
            .unlock(&opener, "passkey-test", &view.to_string())
            .unwrap();
        assert!(session.index.pin(PROJECT).is_some());
        assert!(session.index.pin(MACHINE).is_none());
        assert_eq!(session.invites.len(), 1);
        assert_eq!(session.invite_errors.len(), 1);
        let slot = keys::seal_slot(&other, &keys::recipient_of(&identity).unwrap()).unwrap();
        assert!(session
            .add_store(
                "pending",
                &serde_json::to_string(&descriptor).unwrap(),
                &slot
            )
            .is_err());
        let you = session.answer("", "you", &json!([]), 0).unwrap();
        assert_eq!(you["invites"][0]["project"], MACHINE);
        assert_eq!(you["invite_errors"][0]["id"], "bad");
    }
    #[test]
    fn records_refuse_forgery_sequence_gap_and_other_project() {
        let (mut s, vault, _, key) = fixture();
        let first = signed(&key, 1, "machine", json!({"public_key":hex_key(&key)}));
        let second = signed(&key, 2, "project", json!({"name":"Example"}));
        let object = format::segment_name(MACHINE, 1, 2);
        let seal = |text: &str| {
            keys::encrypt(&keys::recipient_of(&vault).unwrap(), text.as_bytes()).unwrap()
        };
        s.segment("test", &object, &seal(&format!("{first}\n{second}\n")))
            .unwrap();
        s.finish("test").unwrap();
        assert_eq!(
            s.answer("", "projects", &json!([]), 0).unwrap()[0]["name"],
            "Example"
        );
        let forged = second.replace("Example", "Changed");
        s.segment("test", &object, &seal(&format!("{first}\n{forged}\n")))
            .unwrap();
        assert!(s.finish("test").is_err());
        s.stores.get_mut("test").unwrap().segments.clear();
        let gap = signed(&key, 3, "project", json!({"name":"Example"}));
        s.segment("test", &format::segment_name(MACHINE, 1, 1), &seal(&first))
            .unwrap();
        s.segment("test", &format::segment_name(MACHINE, 3, 3), &seal(&gap))
            .unwrap();
        assert!(s.finish("test").is_err());
        s.stores.get_mut("test").unwrap().segments.clear();
        s.segment(
            "test",
            &format::segment_name(MACHINE, 1, 1),
            &seal(&first.replace(PROJECT, MACHINE)),
        )
        .unwrap();
        assert!(s.finish("test").is_err());
    }
    fn hex_key(key: &ed25519_dalek::SigningKey) -> String {
        key.verifying_key()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    #[test]
    fn pins_bind_the_project_and_the_recipient() {
        let (mut s, _, _, _) = fixture();
        let descriptor = format::Descriptor {
            format: 2,
            project: MACHINE.into(),
            recipient: keys::recipient_of(&keys::generate_identity()).unwrap(),
        };
        assert!(s
            .add_store("bad", &serde_json::to_string(&descriptor).unwrap(), &[])
            .is_err());
        assert!(s
            .unlock(
                &keys::generate_identity(),
                "paper",
                r#"{"login":"owner@example.com","slots":[]}"#
            )
            .is_err());
        assert!(s.person.is_none());
    }
    #[test]
    #[ignore = "requires LAYR_WEB_TEST_STORE and LAYR_WEB_TEST_PERSON made from a real CLI store"]
    fn cli_store_unlocks_and_returns_lines() {
        use std::{env, fs, path::PathBuf};
        let root = PathBuf::from(env::var("LAYR_WEB_TEST_STORE").unwrap());
        let person = PathBuf::from(env::var("LAYR_WEB_TEST_PERSON").unwrap());
        let view = fs::read_to_string(person.join("person-view.json")).unwrap();
        let words = fs::read_to_string(person.join("recovery.txt")).unwrap();
        let mut session = Session::new();
        session
            .unlock(&keys::identity_from_paper(&words).unwrap(), "paper", &view)
            .unwrap();
        let slot = person::slot_name(
            &session.login,
            &keys::recipient_of(session.person.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        session
            .add_store(
                "test",
                &fs::read_to_string(root.join("store.json")).unwrap(),
                &fs::read(root.join("slots").join(slot)).unwrap(),
            )
            .unwrap();
        for machine in fs::read_dir(root.join("records")).unwrap() {
            for entry in fs::read_dir(machine.unwrap().path()).unwrap() {
                let entry = entry.unwrap();
                let file = entry.path();
                let key = file.strip_prefix(&root).unwrap().to_str().unwrap();
                session
                    .segment("test", key, &fs::read(&file).unwrap())
                    .unwrap();
            }
        }
        session.finish("test").unwrap();
        let lines = session.answer("test", "lines", &json!([]), 0).unwrap();
        assert!(lines
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["name"] == "main"));
        let expected = fs::read_to_string(person.join("lines.json")).unwrap();
        let mut expected: Value = serde_json::from_str(&expected).unwrap();
        for row in expected.as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("path");
        }
        assert_eq!(lines, expected);
        if let Ok(fixtures) = env::var("LAYR_WEB_TEST_FIXTURES") {
            let fixtures = PathBuf::from(fixtures);
            fn install_all(session: &mut Session, root: &std::path::Path, object: &str) {
                if session.stores["test"].trees.contains_key(object) { return; }
                let bytes = fs::read(root.join(object)).unwrap();
                if let Some(base) = session.base("test", object, &bytes).unwrap() {
                    install_all(session, root, &base);
                }
                session.install("test", object, &bytes).unwrap();
            }
            for state in fs::read_dir(root.join("states")).unwrap() {
                for entry in fs::read_dir(state.unwrap().path()).unwrap() {
                    let path = entry.unwrap().path();
                    let object = path.strip_prefix(&root).unwrap().to_str().unwrap();
                    if object.ends_with(".tree.age") {
                        install_all(&mut session, &root, object);
                    } else if object.ends_with(".data.age") {
                        session.stores.get_mut("test").unwrap().parts.insert(object.into(), fs::read(&path).unwrap());
                    }
                }
            }
            let request: Value = serde_json::from_str(&fs::read_to_string(fixtures.join("request.json")).unwrap()).unwrap();
            let args = json!([request["probe_base"], request["probe_head"]]);
            let behavior = session.answer("test", "probeDiff", &args, i64::MAX / 2).unwrap();
            let expected: Value = serde_json::from_str(&fs::read_to_string(fixtures.join("probe-diff.json")).unwrap()).unwrap();
            assert_eq!(behavior, expected);
            let rows = session.answer("test", "screens", &args, i64::MAX / 2).unwrap();
            assert_eq!(rows.as_array().unwrap().len(), 5);
            let home = rows.as_array().unwrap().iter().find(|r| r["screen"] == "Home@Phone").unwrap();
            assert_eq!(home["changed_share"], 0.25);
            assert_eq!(home["text_changes"], 1);
            assert!(home["base_url"].as_str().unwrap().starts_with("data:image/png;base64,"));
            assert!(rows.as_array().unwrap().iter().any(|r| r["changed_share"].is_null()));
            assert_eq!(session.answer("test", "copies", &json!([]), 0).unwrap(), serde_json::from_str::<Value>(&fs::read_to_string(fixtures.join("copies.json")).unwrap()).unwrap());
            assert_eq!(session.project("test").unwrap().bisect_results().len(), 2);
            if let Ok(output) = env::var("LAYR_WEB_TEST_SCREENS_OUT") {
                fs::write(output, serde_json::to_vec_pretty(&json!({"base":request["probe_base"],"head":request["probe_head"],"screens":rows})).unwrap()).unwrap();
            }
        }
        if let Ok(interdiff) = env::var("LAYR_WEB_TEST_INTERDIFF") {
            let expected: Value =
                serde_json::from_str(&fs::read_to_string(interdiff).unwrap()).unwrap();
            let id = expected["change_id"].as_str().unwrap();
            let from = expected["from"].as_str().unwrap();
            let to = expected["to"].as_str().unwrap();
            if env::var("LAYR_WEB_TEST_FIXTURES").is_err() {
                assert!(session.change_diff("test", id, from, to, "").is_err());
            }
            fn load(
                session: &mut Session,
                root: &std::path::Path,
                object: &str,
                seen: &mut BTreeSet<String>,
            ) {
                if session.stores["test"].trees.contains_key(object) {
                    return;
                }
                assert!(seen.insert(object.into()));
                assert!(seen.len() <= 32);
                let bytes = std::fs::read(root.join(object)).unwrap();
                if let Some(base) = session.base("test", object, &bytes).unwrap() {
                    load(session, root, &base, seen);
                }
                session.install("test", object, &bytes).unwrap();
            }
            for state in session
                .change_inputs("test", id, from, to)
                .unwrap()
                .as_array()
                .unwrap()
            {
                let folder = root.join("states").join(state.as_str().unwrap());
                let object = fs::read_dir(folder)
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .filter(|p| p.to_str().unwrap().ends_with(".tree.age"))
                    .min()
                    .unwrap();
                let object = object.strip_prefix(&root).unwrap().to_str().unwrap();
                load(&mut session, &root, object, &mut BTreeSet::new());
            }
            let summary = session.change_diff("test", id, from, to, "").unwrap();
            assert_eq!(summary["compared"], false);
            assert!(summary["files"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row.get("interdiff").is_none() && row.get("added").is_none()));
            for row in expected["files"].as_array().unwrap() {
                let path = row["path"].as_str().unwrap();
                let objects = session.stores["test"]
                    .trees
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                for object in objects {
                    if let Ok(parts) = session.parts_for("test", &object, path) {
                        for part in parts {
                            let bytes = fs::read(root.join(&part)).unwrap();
                            session
                                .stores
                                .get_mut("test")
                                .unwrap()
                                .parts
                                .insert(part, bytes);
                        }
                    }
                }
                let exact = session.change_diff("test", id, from, to, path).unwrap();
                assert_eq!(exact["compared"], true);
                assert_eq!(exact["files"][0], *row);
            }
        }
    }
}

#[cfg(test)]
mod tree_tests {
    use super::*;
    const STATE: &str = "01a12290-0000-7000-8000-000000000003";
    const MACHINE: &str = "01a12290-0000-7000-8000-000000000002";
    #[test]
    fn trees_and_files_refuse_changed_signatures_names_and_bytes() {
        use sha2::{Digest, Sha256};
        let identity = keys::generate_identity();
        let recipient = keys::recipient_of(&identity).unwrap();
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let data = b"before\n";
        let frame = zstd::stream::encode_all(&data[..], 1).unwrap();
        let link = format::Link(
            STATE.into(),
            "0123abcd".into(),
            0,
            0,
            frame.len() as u64,
            "full".into(),
        );
        let part = link.object();
        let hash = Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let entry = format::Entry("100644".into(), data.len() as u64, hash, vec![link]);
        let mut symlink = entry.clone();
        symlink.0 = "120000".into();
        let tree = format::Tree {
            files: BTreeMap::from([("README.md".into(), entry), ("link".into(), symlink)]),
        };
        let canonical = tree.canonical().unwrap();
        let mut header = format::Header {
            v: 2,
            state: STATE.into(),
            attempt: "0123abcd".into(),
            tree: "full".into(),
            base: None,
            parts: 1,
            machine: MACHINE.into(),
            btrfs: None,
            sig: String::new(),
        };
        header.sign(&canonical, &key).unwrap();
        let mut plain = serde_json::to_vec(&header).unwrap();
        plain.push(b'\n');
        plain.extend(zstd::stream::encode_all(canonical.as_slice(), 1).unwrap());
        let object = keys::encrypt(&recipient, &plain).unwrap();
        let mut session = Session::new();
        session.stores.insert(
            "test".into(),
            Store {
                authorities: BTreeMap::new(),
                published: BTreeMap::new(),
                parent: None,
                identity: Zeroizing::new(identity),
                descriptor: format::Descriptor {
                    format: 2,
                    project: STATE.into(),
                    recipient: recipient.clone(),
                },
                segments: BTreeMap::new(),
                keys: BTreeMap::from([(MACHINE.into(), key.verifying_key())]),
                model: Model::default(),
                trees: BTreeMap::new(),
                parts: BTreeMap::from([(part.clone(), keys::encrypt(&recipient, &frame).unwrap())]),
            },
        );
        let name = format::tree_name(STATE, "0123abcd");
        session.install("test", &name, &object).unwrap();
        assert_eq!(session.read_file("test", &name, "README.md").unwrap(), data);
        assert_eq!(session.read_file("test", &name, "link").unwrap(), data);
        assert_eq!(
            session.tree_view("test", &name, "").unwrap()["entries"][1]["kind"],
            "symlink"
        );
        assert!(session
            .install("test", &format::tree_name(STATE, "fedcba98"), &object)
            .is_err());
        session.stores.get_mut("test").unwrap().keys.insert(
            MACHINE.into(),
            ed25519_dalek::SigningKey::from_bytes(&[8; 32]).verifying_key(),
        );
        assert!(session.install("test", &name, &object).is_err());
        session
            .stores
            .get_mut("test")
            .unwrap()
            .parts
            .insert(part, keys::encrypt(&recipient, b"bad zstd").unwrap());
        assert!(session.read_file("test", &name, "README.md").is_err());
    }
}
