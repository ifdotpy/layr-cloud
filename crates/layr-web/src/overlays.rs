use super::*;
use layr_model::authority::Authority;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub(super) struct OverlayKey {
    #[serde(default)]
    pub hidden: bool,
    pub overlay: String,
    pub name: String,
    pub epoch: String,
    pub parent: String,
    pub store: String,
    pub recipient: String,
    pub readers: BTreeMap<String, String>,
}
#[derive(Clone)]
pub(super) struct Published {
    pub key: OverlayKey,
    pub record: Record,
    pub line: String,
}
impl Published {
    fn prefix(&self) -> String {
        format!("overlays/{}/epochs/{}", self.key.overlay, self.key.epoch)
    }
    fn order(&self) -> (i64, &str, u64) {
        (self.record.time, &self.record.machine, self.record.seq)
    }
}
#[wasm_bindgen]
impl Session {
    pub fn overlay_key(
        &mut self,
        name: &str,
        object: &str,
        bytes: &[u8],
    ) -> std::result::Result<(), JsValue> {
        self.register_overlay(name, object, bytes).map_err(js)
    }
    pub fn overlay_slot_opens(&self, slot: &[u8]) -> bool {
        self.person
            .as_ref()
            .and_then(|person| keys::open_slot(slot, person).ok())
            .map(Zeroizing::new)
            .is_some()
    }
    /// A slot that is absent or belongs to another person reveals no overlay.
    pub fn open_hidden_overlay(
        &mut self,
        name: &str,
        object: &str,
        slot: &[u8],
        manifest: &[u8],
    ) -> std::result::Result<String, JsValue> {
        self.open_hidden_layer(name, object, slot, manifest)
            .map_err(js)
    }
    /// Current readable overlay trees. The caller resolves ciphertext attempts from its catalog.
    pub fn overlay_inputs(&self, name: &str) -> std::result::Result<String, JsValue> {
        self.current_overlay_inputs(name)
            .and_then(encoded)
            .map_err(js)
    }
    pub fn overlay_slots(&self, name: &str) -> std::result::Result<String, JsValue> {
        self.overlay_candidates(name).and_then(encoded).map_err(js)
    }
    pub fn open_overlay(
        &mut self,
        name: &str,
        overlay: &str,
        slot: &[u8],
    ) -> std::result::Result<String, JsValue> {
        self.open_layer(name, overlay, slot).map_err(js)
    }
    pub fn file_location(
        &self,
        name: &str,
        object: &str,
        path: &str,
    ) -> std::result::Result<String, JsValue> {
        let files = self.view_files(name, object).map_err(js)?;
        let (name, object, _, _) = files.get(path).context("file is absent").map_err(js)?;
        encoded(json!({"name":name,"object":object})).map_err(js)
    }
}
impl Session {
    fn register_overlay(&mut self, name: &str, object: &str, bytes: &[u8]) -> Result<()> {
        let store = self.stores.get_mut(name).context("store is locked")?;
        let line = std::str::from_utf8(bytes)?.trim_end_matches('\n');
        let raw = record::parse_line(line, None)?;
        let r: Record = serde_json::from_str(raw.get())?;
        let signer = store
            .keys
            .get(&r.machine)
            .context("overlay signer is not in the parent store")?;
        record::verify_line(line.as_bytes(), signer)?;
        layr_model::validation::validate(&r)?;
        if r.op != "overlay.key" {
            bail!("not an overlay key record");
        }
        let key: OverlayKey = serde_json::from_value(r.data.clone())?;
        layr_model::validation::check_name("overlay", &key.name)?;
        if key.parent != store.descriptor.project
            || key.store != store.descriptor.recipient
            || key.overlay != r.project
            || !format::is_uuid(&key.overlay)
            || !format::is_uuid(&key.epoch)
            || !keys::valid_recipient(&key.recipient)
        {
            bail!("overlay key has invalid project bindings");
        }
        if object != format!("overlays/{}/keys/{}-{}.json", key.overlay, key.epoch, r.id) {
            bail!("overlay key differs from object name");
        }
        if key.hidden {
            bail!("hidden overlay key was published without encryption");
        }
        let authority = store
            .authorities
            .get(&key.name)
            .context("overlay has no parent creator authority")?;
        if !authority.permits(&r, line) {
            bail!("overlay key signer is not a key manager");
        }
        let next = Published {
            key,
            record: r,
            line: line.into(),
        };
        if let Some(old) = store.published.get(&next.key.name) {
            if old.key.overlay != next.key.overlay {
                bail!("conflicting overlay identities");
            }
            if old.order() >= next.order() {
                return Ok(());
            }
        }
        store.published.insert(next.key.name.clone(), next);
        Ok(())
    }
    fn open_hidden_layer(
        &mut self,
        name: &str,
        object: &str,
        slot: &[u8],
        manifest: &[u8],
    ) -> Result<String> {
        let person = self.person.as_ref().context("locked")?;
        let identity = match keys::open_slot(slot, person) {
            Ok(identity) => Zeroizing::new(identity),
            Err(_) => return Ok(String::new()),
        };
        let parent = self.stores.get(name).context("store is locked")?;
        let plain = Zeroizing::new(keys::decrypt(&identity, manifest)?);
        let line = std::str::from_utf8(&plain)?.trim_end_matches('\n');
        let raw = record::parse_line(line, None)?;
        let r: Record = serde_json::from_str(raw.get())?;
        let signer = parent
            .keys
            .get(&r.machine)
            .context("hidden overlay signer is not trusted")?;
        record::verify_line(line.as_bytes(), signer)?;
        layr_model::validation::validate(&r)?;
        let key: OverlayKey = serde_json::from_value(r.data.clone())?;
        layr_model::validation::check_name("overlay", &key.name)?;
        if r.op != "overlay.key"
            || !key.hidden
            || key.parent != parent.descriptor.project
            || key.store != parent.descriptor.recipient
            || key.overlay != r.project
            || !format::is_uuid(&key.overlay)
            || !format::is_uuid(&key.epoch)
            || keys::recipient_of(&identity)? != key.recipient
            || object != format!("overlays/{}/keys/{}-{}.age", key.overlay, key.epoch, r.id)
        {
            bail!("hidden overlay manifest differs from its slot or object");
        }
        let published = Published {
            key,
            record: r,
            line: line.into(),
        };
        let existing = parent.published.get(&published.key.name).or_else(|| {
            self.stores
                .get(&format!("{name}::{}", published.key.name))
                .and_then(|store| store.parent.as_ref().map(|(_, published, _)| published))
        });
        if let Some(old) = existing {
            if old.key.overlay != published.key.overlay {
                bail!("conflicting overlay identities");
            }
            if old.order() >= published.order() {
                return Ok(String::new());
            }
        }
        let alias = format!("{name}::{}", published.key.name);
        // Authority is derived from the overlay's own log in finish_records, before folding.
        let authority: Authority = serde_json::from_value(json!({
            "machines":[], "previous":null, "retained":{}, "changes":{},
        }))?;
        self.stores.insert(
            alias.clone(),
            Store {
                identity,
                descriptor: format::Descriptor {
                    format: 2,
                    project: published.key.overlay.clone(),
                    recipient: published.key.recipient.clone(),
                },
                segments: BTreeMap::new(),
                keys: BTreeMap::new(),
                model: Model::default(),
                trees: BTreeMap::new(),
                parts: BTreeMap::new(),
                authorities: BTreeMap::new(),
                published: BTreeMap::new(),
                parent: Some((name.into(), published, authority)),
            },
        );
        Ok(alias)
    }
    pub(super) fn prepare_hidden_records(&mut self, name: &str) -> Result<()> {
        let store = self.stores.get(name).context("store is locked")?;
        let Some((parent_name, published, _)) = &store.parent else {
            return Ok(());
        };
        if !published.key.hidden {
            return Ok(());
        }
        let parent = self.stores.get(parent_name).context("parent is locked")?;
        let lines = Zeroizing::new(
            store
                .segments
                .values()
                .flat_map(|text| text.lines().map(str::to_owned))
                .collect::<Vec<_>>(),
        );
        let (overlay, authority) = layr_model::authority::from_overlay_lines(
            &parent.descriptor.project,
            &published.key.overlay,
            &lines,
            &parent.keys,
        )?;
        if overlay != published.key.name || !authority.permits(&published.record, &published.line) {
            bail!("hidden overlay key has no creator authority");
        }
        self.stores
            .get_mut(name)
            .context("store is locked")?
            .parent
            .as_mut()
            .context("parent is absent")?
            .2 = authority;
        Ok(())
    }
    fn overlay_candidates(&self, name: &str) -> Result<Value> {
        let store = self.stores.get(name).context("store is locked")?;
        let person = self.person.as_ref().context("locked")?;
        let recipient = keys::recipient_of(person)?;
        let principal = format!("person:{}={recipient}", self.login);
        Ok(json!(store.published.values().filter(|p|p.key.readers.contains_key(&principal)).map(|p|json!({"overlay":p.key.name,"prefix":p.prefix(),"slot":person::slot_name(&self.login,&recipient).expect("validated person")})).collect::<Vec<_>>()))
    }
    fn open_layer(&mut self, name: &str, overlay: &str, slot: &[u8]) -> Result<String> {
        let parent = self.stores.get(name).context("store is locked")?;
        let published = parent
            .published
            .get(overlay)
            .context("overlay has no verified key")?
            .clone();
        let person = self.person.as_ref().context("locked")?;
        if !published.key.readers.contains_key(&format!(
            "person:{}={}",
            self.login,
            keys::recipient_of(person)?
        )) {
            bail!("person is not an overlay reader");
        }
        let identity = Zeroizing::new(keys::open_slot(slot, person)?);
        if keys::recipient_of(&identity)? != published.key.recipient {
            bail!("overlay slot opens another recipient");
        }
        let alias = format!("{name}::{overlay}");
        let authority = parent
            .authorities
            .get(overlay)
            .context("overlay has no creator authority")?
            .clone();
        self.stores.insert(
            alias.clone(),
            Store {
                identity,
                descriptor: format::Descriptor {
                    format: 2,
                    project: published.key.overlay.clone(),
                    recipient: published.key.recipient.clone(),
                },
                segments: BTreeMap::new(),
                keys: BTreeMap::new(),
                model: Model::default(),
                trees: BTreeMap::new(),
                parts: BTreeMap::new(),
                authorities: BTreeMap::new(),
                published: BTreeMap::new(),
                parent: Some((name.into(), published, authority)),
            },
        );
        Ok(alias)
    }
    pub(super) fn overlay_views(&self, name: &str, now: i64) -> Result<Value> {
        let store = self.stores.get(name).context("store is locked")?;
        let p = self.project(name)?;
        let mut rows = Vec::new();
        for r in p
            .op_log(None, usize::MAX)
            .iter()
            .rev()
            .filter(|r| r.op == "overlay.create")
        {
            let Some(overlay) = r.data["name"].as_str() else {
                continue;
            };
            let Some(info) = p.overlay_setting(&format!("overlay.{overlay}")) else {
                continue;
            };
            let Some(id) = info["id"].as_str() else {
                continue;
            };
            let layer = self.stores.get(&format!("{name}::{overlay}"));
            let model = layer.and_then(|s| s.model.projects.get(id));
            let policy = model.map(|p| p.access_policy(now, true));
            let readers: Vec<_> = policy
                .as_ref()
                .map(|p| {
                    p.access
                        .iter()
                        .filter(|(_, role)| **role >= layr_model::policy::Role::Reader)
                        .map(|(name, _)| name.clone())
                        .collect()
                })
                .unwrap_or_else(|| {
                    store
                        .published
                        .get(overlay)
                        .map(|p| {
                            p.key
                                .readers
                                .iter()
                                .filter(|(_, role)| {
                                    layr_model::policy::Role::parse(role).is_some_and(|role| {
                                        role >= layr_model::policy::Role::Reader
                                    })
                                })
                                .map(|(name, _)| name.clone())
                                .collect()
                        })
                        .unwrap_or_default()
                });
            let managers = store
                .authorities
                .get(overlay)
                .map(|a| a.machines.clone())
                .unwrap_or_default();
            let mut row = json!({"id":id,"name":overlay,"patterns":info["patterns"].as_array().cloned().unwrap_or_default(),"readers":readers,"managers":managers,"readable":model.is_some(),"hidden":info["hidden"].as_bool().unwrap_or(false)});
            if let Some(published) = store.published.get(overlay) {
                row["epoch"] = json!(published.key.epoch);
            }
            for field in ["owner", "mode"] {
                if let Some(value) = info.get(field) {
                    row[field] = value.clone();
                }
            }
            if let Some(policy) = policy {
                row["policy"] = json!(policy);
                self.overlay_details(name, overlay, &mut row)?;
            } else {
                // Private overlays keep their public identity, but no private detail fields.
                row.as_object_mut()
                    .context("overlay is not an object")?
                    .remove("patterns");
            }
            rows.push(row);
        }
        // Hidden creation metadata exists only inside a verified, readable overlay.
        for layer in self.stores.values() {
            let Some((parent, published, authority)) = &layer.parent else {
                continue;
            };
            if parent != name || !published.key.hidden {
                continue;
            }
            let Some(project) = layer.model.projects.get(&published.key.overlay) else {
                continue;
            };
            let Some(info) = project.overlay_setting("overlay") else {
                continue;
            };
            if info["hidden"] != true || info["project"] != store.descriptor.project {
                continue;
            }
            let policy = project.access_policy(now, true);
            let readers: Vec<_> = policy
                .access
                .iter()
                .filter(|(_, role)| **role >= layr_model::policy::Role::Reader)
                .map(|(name, _)| name.clone())
                .collect();
            let settings = project.overlay_setting(&format!("overlay.{}", published.key.name));
            let mut row = json!({"id":published.key.overlay,"name":published.key.name,
                "patterns":settings.and_then(|v|v["patterns"].as_array()).cloned().unwrap_or_default(),
                "readers":readers,"managers":authority.machines,"readable":true,"hidden":true,
                "epoch":published.key.epoch,"policy":policy});
            for field in ["owner", "mode"] {
                if let Some(value) = settings
                    .and_then(|v| v.get(field))
                    .or_else(|| info.get(field))
                {
                    row[field] = value.clone();
                }
            }
            self.overlay_details(name, &published.key.name, &mut row)?;
            rows.push(row);
        }
        rows.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        Ok(json!(rows))
    }
    /// Use the parent default line's model stack, then the overlay's own CLI line fallback.
    fn overlay_current_state(&self, name: &str, overlay: &str) -> Result<Option<String>> {
        let parent = self.project(name)?;
        let alias = format!("{name}::{overlay}");
        let Some(layer) = self.stores.get(&alias) else {
            return Ok(None);
        };
        let Some(project) = layer.model.projects.get(&layer.descriptor.project) else {
            return Ok(None);
        };
        let settings = project
            .overlay_setting(&format!("overlay.{overlay}"))
            .or_else(|| parent.overlay_setting(&format!("overlay.{overlay}")));
        let metadata = project.overlay_setting("overlay");
        let lines = parent.line_list();
        let records = parent.op_log(None, usize::MAX);
        let default = records
            .iter()
            .rev()
            .flat_map(|r| r.lines.iter())
            .filter(|(_, change)| change.before.is_none() && change.after.is_some())
            .find_map(|(id, _)| lines.iter().find(|line| &line.id == id));
        let own_lines = project.line_list();
        if let Some(line) = default {
            if let Some(selected) = self
                .layers(name, &line.head)?
                .into_iter()
                .find(|layer| layer["overlay"] == overlay)
            {
                let state = selected["state"]
                    .as_str()
                    .context("overlay layer has no state")?;
                if project.states.contains_key(state) {
                    return Ok(Some(state.into()));
                }
            }
        }
        // CLI overlay reads fall back to an existing hidden line, or the owner's shared line.
        let hidden = metadata.is_some_and(|v| v["hidden"] == true);
        let owner = settings.and_then(|v| v["owner"].as_u64());
        let line = if hidden {
            own_lines.first()
        } else {
            owner.and_then(|uid| {
                own_lines
                    .iter()
                    .find(|line| line.name == format!("user-{uid}"))
            })
        };
        Ok(line
            .filter(|line| project.states.contains_key(&line.head))
            .map(|line| line.head.clone()))
    }
    fn current_overlay_inputs(&self, name: &str) -> Result<Value> {
        self.project(name)?;
        let mut inputs = Vec::new();
        for (alias, store) in &self.stores {
            let Some((parent, published, _)) = &store.parent else {
                continue;
            };
            if parent != name {
                continue;
            }
            if let Some(state) = self.overlay_current_state(name, &published.key.name)? {
                inputs.push(json!({"name":alias,"state":state}));
            }
        }
        Ok(json!(inputs))
    }
    fn overlay_details(&self, name: &str, overlay: &str, row: &mut Value) -> Result<()> {
        let alias = format!("{name}::{overlay}");
        let layer = self.stores.get(&alias).context("overlay is not open")?;
        let project = layer
            .model
            .projects
            .get(&layer.descriptor.project)
            .context("overlay has no verified records")?;
        let records = project.op_log(None, usize::MAX);
        row["history"] = json!(records);
        row["epochs"] = json!(epoch_views(project));
        // Only signed overlay settings or public parent settings supply routed patterns.
        let settings = project
            .overlay_setting(&format!("overlay.{overlay}"))
            .or_else(|| {
                self.project(name)
                    .ok()?
                    .overlay_setting(&format!("overlay.{overlay}"))
            });
        row["patterns"] = json!(settings
            .and_then(|v| v["patterns"].as_array())
            .cloned()
            .unwrap_or_default());
        if let Some(state) = self.overlay_current_state(name, overlay)? {
            row["state"] = json!(state);
            if let Some((_, (_, tree))) = layer
                .trees
                .iter()
                .find(|(object, _)| format::parse_tree_name(object).is_some_and(|v| v.0 == state))
            {
                row["paths"] = json!(tree.files.keys().collect::<Vec<_>>());
                row["files"] = json!(tree
                    .files
                    .iter()
                    .map(|(path, entry)| json!({"state":state,"path":path,"size":entry.1}))
                    .collect::<Vec<_>>());
            }
        }
        Ok(())
    }
    pub(super) fn layers(&self, name: &str, state: &str) -> Result<Vec<Value>> {
        let store = self.stores.get(name).context("store is locked")?;
        if store.parent.is_some() {
            return Ok(vec![]);
        }
        let Ok(p) = self.project(name) else {
            return Ok(vec![]);
        };
        let Some(s) = p.states.get(state) else {
            return Ok(vec![]);
        };
        let line = p.line_list().into_iter().find(|l| l.head == state);
        let (line, machine) = match &line {
            Some(l) => (Some(l.id.as_str()), l.machine.as_str()),
            None => (s.state.line.as_deref(), s.machine.as_str()),
        };
        let Some(line) = line else { return Ok(vec![]) };
        // The shared model handles visible stacks and hidden follow-mode selection.
        let mut model = store.model.clone();
        for layer in self.stores.values() {
            if layer
                .parent
                .as_ref()
                .is_some_and(|(parent, _, _)| parent == name)
            {
                model.projects.extend(layer.model.projects.clone());
            }
        }
        let selected = layr_model::Line {
            id: line.into(),
            name: String::new(),
            head: state.into(),
            owner: 0,
            machine: machine.into(),
        };
        let stack = model.layers(&store.descriptor.project, &selected)?;
        Ok(stack
            .into_iter()
            .filter_map(|l| {
                let alias = format!("{name}::{}", l.overlay);
                self.stores
                    .contains_key(&alias)
                    .then(|| json!({"name":alias,"state":l.state,"overlay":l.overlay}))
            })
            .collect())
    }
    pub(super) fn view_files(
        &self,
        name: &str,
        object: &str,
    ) -> Result<BTreeMap<String, (String, String, format::Entry, Option<String>)>> {
        let store = self.stores.get(name).context("store is locked")?;
        let tree = &store.trees.get(object).context("tree is absent")?.1;
        let mut files: BTreeMap<_, _> = tree
            .files
            .iter()
            .map(|(path, entry)| {
                (
                    path.clone(),
                    (name.into(), object.into(), entry.clone(), None),
                )
            })
            .collect();
        if store.parent.is_none() {
            let state = format::parse_tree_name(object).context("invalid tree")?.0;
            for layer in self.layers(name, &state)? {
                let alias = layer["name"].as_str().context("invalid layer")?;
                let state = layer["state"].as_str().context("invalid layer state")?;
                let store = self.stores.get(alias).context("layer is absent")?;
                let Some((object, (_, tree))) = store.trees.iter().find(|(object, _)| {
                    format::parse_tree_name(object).is_some_and(|v| v.0 == state)
                }) else {
                    bail!("overlay tree is unavailable")
                };
                for (path, entry) in &tree.files {
                    files.retain(|old, _| {
                        !old.starts_with(&format!("{path}/"))
                            && !path.starts_with(&format!("{old}/"))
                    });
                    files.insert(
                        path.clone(),
                        (
                            alias.into(),
                            object.clone(),
                            entry.clone(),
                            layer["overlay"].as_str().map(str::to_string),
                        ),
                    );
                }
            }
        }
        Ok(files)
    }
}
// A key can be republished (for reader additions or another store). Number distinct
// identities in signed sequence order; wall-clock skew never changes their ordinal.
fn key_epochs(records: &[Record]) -> Vec<Value> {
    let mut keys: Vec<_> = records.iter().filter(|r| r.op == "overlay.key").collect();
    keys.sort_by(|a, b| (a.seq, &a.machine, &a.id).cmp(&(b.seq, &b.machine, &b.id)));
    let mut seen = BTreeSet::new();
    let mut epochs = Vec::new();
    for record in keys {
        let Some(id) = record.data["epoch"].as_str() else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let mut epoch =
            json!({"id":id,"number":epochs.len()+1,"time":record.time,"machine":record.machine});
        if let Some(reason) = record.data.get("reason") {
            epoch["reason"] = reason.clone();
        }
        epochs.push(epoch);
    }
    epochs
}
pub(super) fn epoch_views(project: &Project) -> Vec<Value> {
    key_epochs(&project.op_log(None, usize::MAX))
}
pub(super) type Parent = (String, Published, Authority);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_discovery_is_silent_for_foreign_slots_and_requires_own_authority() {
        let (mut session, _, _, signer) = crate::tests::fixture();
        let machine = "01a12290-0000-7000-8000-000000000002";
        let overlay = "01a12290-0000-7000-8000-000000000004";
        let epoch = "01a12290-0000-7000-8000-000000000005";
        let id = "01a12290-0000-7000-8000-000000000006";
        let identity = Zeroizing::new(keys::generate_identity());
        let recipient = keys::recipient_of(&identity).unwrap();
        let person = session.person.as_ref().unwrap();
        let slot = keys::seal_slot(&identity, &keys::recipient_of(person).unwrap()).unwrap();
        let foreign = Zeroizing::new(keys::generate_identity());
        let foreign_slot =
            keys::seal_slot(&identity, &keys::recipient_of(&foreign).unwrap()).unwrap();
        assert!(!session.overlay_slot_opens(&foreign_slot));
        assert_eq!(
            session
                .open_hidden_layer("test", "invalid object", &foreign_slot, b"invalid manifest")
                .unwrap(),
            ""
        );
        assert_eq!(session.stores.len(), 1);
        assert!(session.overlay_slot_opens(&slot));
        let store = session.stores.get_mut("test").unwrap();
        store.keys.insert(machine.into(), signer.verifying_key());
        let data = json!({"hidden":true,"overlay":overlay,"name":"secret","epoch":epoch,"parent":store.descriptor.project,"store":store.descriptor.recipient,"recipient":recipient,"readers":{}});
        let r = json!({"id":id,"machine":machine,"project":overlay,"seq":3,"time":3,"op":"overlay.key","actor":{"uid":0,"name":"test"},"states":[],"lines":{},"data":data});
        let body = r.to_string();
        let line = format!(
            "{{\"r\":{body},\"s\":\"{}\"}}",
            keys::sign_bytes(body.as_bytes(), &signer)
        );
        let manifest = keys::encrypt(&recipient, line.as_bytes()).unwrap();
        let object = format!("overlays/{overlay}/keys/{epoch}-{id}.age");
        assert!(session
            .open_hidden_layer("test", &object.replace(epoch, id), &slot, &manifest)
            .is_err());
        assert!(session
            .register_overlay("test", &object.replace(".age", ".json"), line.as_bytes())
            .is_err());
        let forged =
            keys::encrypt(&recipient, line.replace("secret", "forged").as_bytes()).unwrap();
        assert!(session
            .open_hidden_layer("test", &object, &slot, &forged)
            .is_err());
        let alias = session
            .open_hidden_layer("test", &object, &slot, &manifest)
            .unwrap();
        assert_eq!(alias, "test::secret");
        // Possessing a slot is insufficient: its own creation and manager chain must verify.
        assert!(session.prepare_hidden_records(&alias).is_err());
        assert!(session.stores[&alias].model.projects.is_empty());
        assert!(session.stores["test"].published.is_empty());
        session.lock();
        assert!(session.stores.is_empty());
        assert!(session.person.is_none());
    }
    #[test]
    fn epoch_keys_require_parent_authority_and_exact_bindings() {
        let (mut session, _, _, signer) = crate::tests::fixture();
        let machine = "01a12290-0000-7000-8000-000000000002";
        let overlay = "01a12290-0000-7000-8000-000000000004";
        let epoch = "01a12290-0000-7000-8000-000000000005";
        let id = "01a12290-0000-7000-8000-000000000006";
        let recipient = keys::recipient_of(&keys::generate_identity()).unwrap();
        let store = session.stores.get_mut("test").unwrap();
        store.keys.insert(machine.into(), signer.verifying_key());
        let data = json!({"overlay":overlay,"name":"private","epoch":epoch,"parent":store.descriptor.project,"store":store.descriptor.recipient,"recipient":recipient,"readers":{}});
        let r = json!({"id":id,"machine":machine,"project":overlay,"seq":3,"time":3,"op":"overlay.key","actor":{"uid":0,"name":"test"},"states":[],"lines":{},"data":data});
        let body = r.to_string();
        let line = format!(
            "{{\"r\":{body},\"s\":\"{}\"}}",
            keys::sign_bytes(body.as_bytes(), &signer)
        );
        let object = format!("overlays/{overlay}/keys/{epoch}-{id}.json");
        assert!(session
            .register_overlay("test", &object, line.as_bytes())
            .is_err());
        let authority: Authority = serde_json::from_value(
            json!({"machines":[machine],"previous":null,"retained":{},"changes":{}}),
        )
        .unwrap();
        session
            .stores
            .get_mut("test")
            .unwrap()
            .authorities
            .insert("private".into(), authority);
        session
            .register_overlay("test", &object, line.as_bytes())
            .unwrap();
        assert!(session
            .register_overlay("test", &object.replace(epoch, id), line.as_bytes())
            .is_err());
        let forged = line.replace("private", "changed");
        assert!(session
            .register_overlay("test", &object, forged.as_bytes())
            .is_err());
        assert!(session.open_layer("test", "private", &[]).is_err());
        assert!(session
            .overlay_candidates("test")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
    }
}

#[cfg(test)]
#[path = "../tests/support/overlay-details.rs"]
mod detail_tests;
