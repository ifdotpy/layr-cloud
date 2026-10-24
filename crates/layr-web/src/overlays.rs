use super::*;
use layr_model::authority::Authority;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub(super) struct OverlayKey {
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
            let mut row = json!({"id":id,"name":overlay,"patterns":info["patterns"].as_array().cloned().unwrap_or_default(),"readers":readers,"managers":managers,"readable":model.is_some(),"hidden":model.is_none()});
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
            }
            rows.push(row);
        }
        Ok(json!(rows))
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
        let scoped = format!("stack.{line}@{machine}");
        let plain = format!("stack.{line}");
        let stack = p
            .config
            .get(&scoped)
            .or_else(|| p.config.get(&plain))
            .cloned()
            .unwrap_or(json!([]));
        let stack: Vec<layr_model::validation::Layer> = serde_json::from_value(stack)?;
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
pub(super) type Parent = (String, Published, Authority);

#[cfg(test)]
mod tests {
    use super::*;
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
