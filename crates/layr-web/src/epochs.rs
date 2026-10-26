//! Epoch routing is provisional until discovery and the preserved root log are authenticated.
use super::*;
use layr_store::epochs::Key;

pub(super) struct Published {
    key: Key,
    record: Record,
    line: String,
}
pub(super) struct Epochs {
    chain: Vec<Published>,
    identities: BTreeMap<String, Zeroizing<String>>,
    root_segments: BTreeMap<String, Zeroizing<String>>,
    pub current: format::Descriptor,
}
pub(super) fn split(object: &str) -> Result<(&str, &str)> {
    if let Some(rest) = object.strip_prefix("project/epochs/") {
        let (epoch, inner) = rest.split_once('/').context("invalid epoch object")?;
        if !format::is_uuid(epoch) || inner.is_empty() || inner.starts_with("project/") {
            bail!("invalid epoch object");
        }
        Ok((epoch, inner))
    } else {
        Ok(("root", object))
    }
}
impl Epochs {
    fn parse(
        root: &format::Descriptor,
        entries: &str,
        current: format::Descriptor,
    ) -> Result<Self> {
        let objects: BTreeMap<String, String> = serde_json::from_str(entries)?;
        if objects.is_empty() || objects.len() > 128 {
            bail!("invalid epoch discovery count");
        }
        let mut pending = Vec::new();
        for (object, line) in objects {
            let line = line.trim_end_matches('\n').to_owned();
            let record: Record = serde_json::from_str(record::parse_line(&line, None)?.get())?;
            let key: Key = serde_json::from_value(record.data.clone())?;
            key.validate()?;
            if record.op != "project.key"
                || record.project != root.project
                || key.project != root.project
                || key.object(&record.id) != object
            {
                bail!("epoch discovery binding differs");
            }
            pending.push(Published { key, record, line });
        }
        let mut chain = Vec::new();
        let mut parent = "root".to_string();
        let mut recipient = root.recipient.clone();
        loop {
            let children = pending
                .iter()
                .enumerate()
                .filter(|(_, p)| p.key.parent == parent)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if children.len() > 1 {
                bail!("project epochs forked");
            }
            let Some(&i) = children.first() else { break };
            let p = pending.remove(i);
            if p.key.parent_recipient != recipient {
                bail!("epoch recipient lineage differs");
            }
            parent = p.key.epoch.clone();
            recipient = p.key.recipient.clone();
            chain.push(p);
        }
        if !pending.is_empty()
            || current.format != 2
            || current.project != root.project
            || current.recipient != recipient
        {
            bail!("epoch descriptor or lineage differs");
        }
        Ok(Self {
            chain,
            identities: BTreeMap::new(),
            root_segments: BTreeMap::new(),
            current,
        })
    }
    pub fn epoch(&self) -> &str {
        &self.chain.last().expect("nonempty epoch chain").key.epoch
    }
    fn recipients(&self, root: &format::Descriptor) -> BTreeMap<String, String> {
        std::iter::once(("root".into(), root.recipient.clone()))
            .chain(
                self.chain
                    .iter()
                    .take(self.chain.len() - 1)
                    .map(|p| (p.key.epoch.clone(), p.key.recipient.clone())),
            )
            .collect()
    }
    pub fn validate(
        &self,
        root: &format::Descriptor,
        exact: &[String],
        trusted: &BTreeMap<String, VerifyingKey>,
    ) -> Result<()> {
        if self.identities.len() != self.chain.len() {
            bail!("sealed epoch history is incomplete");
        }
        let original = verified_segments(&root.project, &self.root_segments)?;
        if original.is_empty() || original.iter().any(|line| !exact.contains(line)) {
            bail!("epoch rewrites root history");
        }
        let root_writers = layr_store::public::trusted_writers(&root.project, &original)?;
        if root_writers
            .iter()
            .any(|(id, key)| trusted.get(id) != Some(key))
        {
            bail!("epoch changes root writer identity");
        }
        for p in &self.chain {
            record::verify_line(
                p.line.as_bytes(),
                trusted
                    .get(&p.record.machine)
                    .context("epoch signer is not trusted")?,
            )?;
            if !exact.contains(&p.line) {
                bail!("epoch discovery is absent from encrypted history");
            }
        }
        let readers = exact
            .iter()
            .rev()
            .map(|line| {
                serde_json::from_str::<Record>(record::parse_line(line, None)?.get())
                    .map_err(Into::into)
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .find(|r| r.op == "project.readers" && r.data["epoch"] == self.epoch())
            .context("epoch has no encrypted reader set")?;
        let readers: BTreeMap<String, String> =
            serde_json::from_value(readers.data["readers"].clone())?;
        if layr_store::epochs::readers_hash(&readers)?
            != self.chain.last().unwrap().key.readers_hash
        {
            bail!("epoch reader hash differs");
        }
        Ok(())
    }
}
/// Verify complete root machine sequences independently of the new epoch's trust graph.
fn verified_segments(
    project: &str,
    segments: &BTreeMap<String, Zeroizing<String>>,
) -> Result<Vec<String>> {
    let mut machines: BTreeMap<String, BTreeMap<u64, String>> = BTreeMap::new();
    for (object, text) in segments {
        let (machine, first, last) =
            format::parse_segment_name(object).context("invalid root segment")?;
        let mut seq = first;
        for line in text.lines() {
            let r: Record = serde_json::from_str(record::parse_line(line, None)?.get())?;
            if r.project != project || r.machine != machine || r.seq != seq || seq > last {
                bail!("invalid root record binding");
            }
            if machines
                .entry(machine.clone())
                .or_default()
                .insert(seq, line.into())
                .is_some_and(|old| old != line)
            {
                bail!("root sequence conflicts");
            }
            seq += 1;
        }
        if seq != last + 1 {
            bail!("root segment is incomplete");
        }
    }
    let mut out = Vec::new();
    for (_, lines) in machines {
        let raw = record::parse_line(lines.get(&1).context("root announcement missing")?, None)?;
        let key = record::machine_key(raw)?;
        for (i, (seq, line)) in lines.into_iter().enumerate() {
            if seq != i as u64 + 1 {
                bail!("root history has a gap");
            }
            record::verify_line(line.as_bytes(), &key)?;
            out.push(line);
        }
    }
    Ok(out)
}
impl Store {
    pub(super) fn source<'a, 'b>(&'a self, object: &'b str) -> Result<(&'a str, &'b str)> {
        let (epoch, logical) = split(object)?;
        let identity = match &self.epochs {
            Some(epochs) if epoch == epochs.epoch() => self.identity.as_str(),
            Some(epochs) => epochs
                .identities
                .get(epoch)
                .context("epoch identity is unavailable")?
                .as_str(),
            None if epoch == "root" => self.identity.as_str(),
            None => bail!("object belongs to another epoch"),
        };
        Ok((identity, logical))
    }
}
#[wasm_bindgen]
impl Session {
    pub fn open_epoch_store(
        &mut self,
        name: &str,
        root: &str,
        discovery: &str,
        current: &str,
        slot: &[u8],
    ) -> std::result::Result<String, JsValue> {
        self.epoch_store(name, root, discovery, current, slot, None)
            .map_err(js)
    }
    pub fn open_public_epoch_store(
        &mut self,
        name: &str,
        root: &str,
        discovery: &str,
        current: &str,
        manifest: &[u8],
        slot: &[u8],
    ) -> std::result::Result<String, JsValue> {
        self.epoch_store(name, root, discovery, current, slot, Some(manifest))
            .map_err(js)
    }
    pub fn add_epoch_history(
        &mut self,
        name: &str,
        epoch: &str,
        sealed: &[u8],
    ) -> std::result::Result<(), JsValue> {
        (|| -> Result<()> {
            let store = self.stores.get_mut(name).context("store is locked")?;
            let epochs = store.epochs.as_mut().context("store has no epochs")?;
            let expected = epochs
                .recipients(&store.descriptor)
                .remove(epoch)
                .context("unknown history epoch")?;
            let identity = Zeroizing::new(keys::open_slot(sealed, &store.identity)?);
            if keys::recipient_of(&identity)? != expected {
                bail!("history identity differs from lineage");
            }
            epochs.identities.insert(epoch.into(), identity);
            Ok(())
        })()
        .map_err(js)
    }
    pub fn add_root_segment(
        &mut self,
        name: &str,
        object: &str,
        ciphertext: &[u8],
    ) -> std::result::Result<(), JsValue> {
        (|| -> Result<()> {
            format::parse_segment_name(object).context("invalid root segment")?;
            let store = self.stores.get_mut(name).context("store is locked")?;
            let epochs = store.epochs.as_mut().context("store has no epochs")?;
            let identity = epochs
                .identities
                .get("root")
                .context("sealed root identity absent")?;
            let text = Zeroizing::new(String::from_utf8(keys::decrypt(identity, ciphertext)?)?);
            if epochs
                .root_segments
                .get(object)
                .is_some_and(|old| old.as_str() != text.as_str())
            {
                bail!("root segment changed");
            }
            epochs.root_segments.insert(object.into(), text);
            Ok(())
        })()
        .map_err(js)
    }
}
impl Session {
    fn epoch_store(
        &mut self,
        name: &str,
        root: &str,
        discovery: &str,
        current: &str,
        slot: &[u8],
        manifest: Option<&[u8]>,
    ) -> Result<String> {
        let descriptor: format::Descriptor = serde_json::from_str(root)?;
        if descriptor.format != 2 || !format::is_uuid(&descriptor.project) {
            bail!("invalid root descriptor");
        }
        let epochs = Epochs::parse(&descriptor, discovery, serde_json::from_str(current)?)?;
        let (identity, manifest) = if let Some(bytes) = manifest {
            let manifest = layr_store::public::Manifest::parse(bytes, &epochs.current)?;
            (Zeroizing::new(manifest.open(slot)?), Some(manifest))
        } else {
            self.index
                .pin(&descriptor.project)
                .context("store has no person pin")?
                .check_store(&descriptor)?;
            (
                Zeroizing::new(keys::open_slot(
                    slot,
                    self.person.as_ref().context("locked")?,
                )?),
                None,
            )
        };
        if keys::recipient_of(&identity)? != epochs.current.recipient {
            bail!("current epoch slot differs");
        }
        let project = descriptor.project.clone();
        let mut cache = index::Cache::default();
        cache.public = manifest;
        self.stores.insert(
            name.into(),
            Store {
                epochs: Some(epochs),
                cache,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    const MACHINE: &str = "01a12290-0000-7000-8000-000000000002";
    const EPOCH: &str = "01a12290-0000-7000-8000-000000000008";
    fn line(
        project: &str,
        seq: u64,
        op: &str,
        data: Value,
        key: &ed25519_dalek::SigningKey,
    ) -> String {
        let body=json!({"id":format!("01a12290-0000-7000-8001-{seq:012}"),"project":project,"machine":MACHINE,"seq":seq,"time":seq,"op":op,"actor":{"uid":0,"name":"Owner"},"states":[],"lines":{},"data":data}).to_string();
        format!(
            "{{\"r\":{body},\"s\":\"{}\"}}",
            keys::sign_bytes(body.as_bytes(), key)
        )
    }
    #[test]
    fn epoch_reader_authenticates_anchor_lineage_readers_and_history() {
        for tamper in [
            "none",
            "missing-root",
            "reader-hash",
            "discovery-absent",
            "history-recipient",
            "fork",
        ] {
            let (mut s, vault, _, key) = crate::tests::fixture();
            let root = serde_json::to_string(&s.stores["test"].descriptor).unwrap();
            let descriptor: &format::Descriptor = &s.stores["test"].descriptor;
            let project = descriptor.project.clone();
            let root_recipient = descriptor.recipient.clone();
            let fresh = keys::generate_identity();
            let recipient = keys::recipient_of(&fresh).unwrap();
            let readers = BTreeMap::from([(format!("machine:{MACHINE}"), root_recipient.clone())]);
            let publication = Key {
                project: project.clone(),
                parent: "root".into(),
                parent_recipient: root_recipient.clone(),
                epoch: EPOCH.into(),
                recipient: recipient.clone(),
                mode: layr_store::epochs::Mode::Private,
                readers_hash: if tamper == "reader-hash" {
                    "0".repeat(64)
                } else {
                    layr_store::epochs::readers_hash(&readers).unwrap()
                },
            };
            let announcement = line(
                &project,
                1,
                "machine",
                json!({"public_key":key.verifying_key().to_bytes().iter().map(|b|format!("{b:02x}")).collect::<String>()}),
                &key,
            );
            let created = line(
                &project,
                2,
                "project",
                json!({"id":project,"name":"epoch-test"}),
                &key,
            );
            let audience = line(
                &project,
                3,
                "project.readers",
                json!({"epoch":EPOCH,"readers":readers}),
                &key,
            );
            let published = line(
                &project,
                4,
                "project.key",
                serde_json::to_value(&publication).unwrap(),
                &key,
            );
            let record: Record =
                serde_json::from_str(record::parse_line(&published, None).unwrap().get()).unwrap();
            let mut discovery =
                BTreeMap::from([(publication.object(&record.id), published.clone())]);
            if tamper == "fork" {
                let mut fork = publication.clone();
                fork.epoch = "01a12290-0000-7000-8000-000000000009".into();
                let fork_line = line(
                    &project,
                    5,
                    "project.key",
                    serde_json::to_value(&fork).unwrap(),
                    &key,
                );
                discovery.insert(
                    fork.object("01a12290-0000-7000-8001-000000000005"),
                    fork_line,
                );
            }
            let current = serde_json::to_string(&format::Descriptor {
                format: 2,
                project: project.clone(),
                recipient: recipient.clone(),
            })
            .unwrap();
            let slot = keys::seal_slot(
                &fresh,
                &keys::recipient_of(s.person.as_ref().unwrap()).unwrap(),
            )
            .unwrap();
            let opened = s.epoch_store(
                "epoch",
                &root,
                &serde_json::to_string(&discovery).unwrap(),
                &current,
                &slot,
                None,
            );
            if tamper == "fork" {
                assert!(opened.is_err());
                continue;
            }
            opened.unwrap();
            assert!(
                keys::decrypt(&vault, &keys::encrypt(&recipient, b"new data").unwrap()).is_err()
            );
            let old = if tamper == "history-recipient" {
                keys::generate_identity()
            } else {
                vault.clone()
            };
            let sealed = keys::seal_slot(&old, &recipient).unwrap();
            // Validate the history opener before retaining it, as add_epoch_history does.
            let opened = keys::open_slot(&sealed, &fresh).unwrap();
            if tamper == "history-recipient" {
                assert_ne!(keys::recipient_of(&opened).unwrap(), root_recipient);
                continue;
            }
            s.add_epoch_history("epoch", "root", &sealed).unwrap();
            if tamper != "missing-root" {
                let bytes = keys::encrypt(
                    &root_recipient,
                    format!("{announcement}\n{created}\n").as_bytes(),
                )
                .unwrap();
                s.add_root_segment("epoch", &format::segment_name(MACHINE, 1, 2), &bytes)
                    .unwrap();
            }
            let mut lines = vec![announcement, created, audience];
            if tamper != "discovery-absent" {
                lines.push(published)
            }
            let bytes = keys::encrypt(&recipient, lines.join("\n").as_bytes()).unwrap();
            let object = format!(
                "project/epochs/{EPOCH}/{}",
                format::segment_name(MACHINE, 1, lines.len() as u64)
            );
            s.segment("epoch", &object, &bytes).unwrap();
            let result = s.finish("epoch");
            assert_eq!(result.is_ok(), tamper == "none", "{tamper}: {result:?}");
            if tamper == "none" {
                let view = s.project_view("epoch", 0).unwrap();
                assert_eq!(view["epochs"][1]["epoch"], EPOCH);
                assert_eq!(s.stores["epoch"].source(&object).unwrap().0, fresh);
                assert_eq!(
                    s.stores["epoch"].source("records/root.age").unwrap().0,
                    vault
                );
                assert!(s.stores["epoch"]
                    .source("project/epochs/01a12290-0000-7000-8000-000000000099/states/x")
                    .is_err());
                s.lock();
                assert!(s.stores.is_empty());
            }
        }
    }
}
