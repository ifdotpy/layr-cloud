//! Epoch discovery is routing metadata. Readers authenticate its exact signed records
//! against decrypted project history; the gateway keeps no second writer registry.
use crate::{
    auth::Proof,
    config::Project,
    server::{Gateway, Role, JSON_LIMIT},
};
use anyhow::{bail, Context, Result};
use layr_store::{epochs::Key, format, record};
use serde_json::Value;

pub fn split(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("project/epochs/")?;
    let (epoch, inner) = rest.split_once('/')?;
    (format::is_uuid(epoch) && !inner.is_empty()).then_some((epoch, inner))
}

pub fn discovery(path: &str) -> bool {
    let Some(file) = path
        .strip_prefix("keys/")
        .and_then(|s| s.strip_suffix(".json"))
    else {
        return false;
    };
    file.is_ascii()
        && file.len() == 73
        && file.as_bytes()[36] == b'-'
        && format::is_uuid(&file[..36])
        && format::is_uuid(&file[37..])
}

pub fn key_record(path: &str, bytes: &[u8], project: &str) -> Result<Key> {
    if !discovery(path) || bytes.len() > JSON_LIMIT {
        bail!("invalid epoch discovery")
    }
    let text = std::str::from_utf8(bytes)?;
    let value: Value = serde_json::from_str(record::parse_line(text, None)?.get())?;
    let key: Key = serde_json::from_value(value["data"].clone())?;
    key.validate()?;
    if value["op"] != "project.key"
        || value["project"] != project
        || key.project != project
        || value["id"]
            .as_str()
            .is_none_or(|id| !format::is_uuid(id) || key.object(id) != path)
    {
        bail!("epoch discovery binding differs")
    }
    Ok(key)
}

#[derive(Clone)]
pub struct Chain {
    root: Project,
    keys: Vec<Key>,
}
impl Chain {
    pub fn load(gateway: &Gateway, root: &Project) -> Result<Self> {
        let entries = gateway.storage.list(root, "keys/", Role::Reader)?;
        if entries.len() > 128 {
            bail!("too many project epochs")
        }
        if entries.is_empty() {
            return Ok(Self {
                root: root.clone(),
                keys: Vec::new(),
            });
        }
        let descriptor = gateway
            .storage
            .get(root, "store.json", None, JSON_LIMIT)?
            .context("project has no root descriptor")?;
        let descriptor: format::Descriptor = serde_json::from_slice(&descriptor.body)?;
        if descriptor.format != 2 || descriptor.recipient != root.recipient {
            bail!("project root descriptor differs")
        }
        let mut pending = Vec::new();
        for entry in entries {
            if !discovery(&entry.key) {
                bail!("invalid epoch discovery name")
            }
            let object = gateway
                .storage
                .get(root, &entry.key, None, JSON_LIMIT)?
                .context("epoch discovery disappeared")?;
            let key = key_record(&entry.key, &object.body, &descriptor.project)?;
            let mut scoped = root.clone();
            scoped.prefix = format!("{}/{}", root.prefix.trim_end_matches('/'), key.prefix());
            let current = gateway
                .storage
                .get(&scoped, "store.json", None, JSON_LIMIT)?
                .context("published epoch has no descriptor")?;
            let current: format::Descriptor = serde_json::from_slice(&current.body)?;
            if current.format != 2
                || current.project != descriptor.project
                || current.recipient != key.recipient
            {
                bail!("published epoch descriptor differs");
            }
            pending.push(key);
        }
        let mut keys = Vec::new();
        let mut parent = "root".to_string();
        let mut recipient = root.recipient.clone();
        loop {
            let children = pending
                .iter()
                .enumerate()
                .filter(|(_, key)| key.parent == parent)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if children.len() > 1 {
                bail!("project epochs forked")
            }
            let Some(&index) = children.first() else {
                break;
            };
            let key = pending.remove(index);
            if key.parent_recipient != recipient {
                bail!("epoch recipient lineage differs")
            }
            parent = key.epoch.clone();
            recipient = key.recipient.clone();
            keys.push(key);
        }
        if !pending.is_empty() {
            bail!("epoch has no parent")
        }
        Ok(Self {
            root: root.clone(),
            keys,
        })
    }
    pub fn current(&self) -> &str {
        self.keys.last().map_or("root", |key| key.epoch.as_str())
    }
    pub fn public(&self, gateway: &Gateway) -> Result<bool> {
        let current = self.project(self.current()).expect("current epoch exists");
        let Some(manifest) = gateway
            .storage
            .get(&current, "public.json", None, JSON_LIMIT)?
        else {
            return Ok(false);
        };
        let Some(descriptor) = gateway
            .storage
            .get(&current, "store.json", None, JSON_LIMIT)?
        else {
            return Ok(false);
        };
        let descriptor: format::Descriptor = serde_json::from_slice(&descriptor.body)?;
        Ok(descriptor.recipient == current.recipient
            && layr_store::public::Manifest::parse(&manifest.body, &descriptor).is_ok())
    }
    pub fn project(&self, epoch: &str) -> Option<Project> {
        if epoch == "root" {
            return Some(self.root.clone());
        }
        let key = self.keys.iter().find(|key| key.epoch == epoch)?;
        let mut project = self.root.clone();
        project.prefix = format!("{}/{}", project.prefix.trim_end_matches('/'), key.prefix());
        project.recipient = key.recipient.clone();
        Some(project)
    }
    /// Writers stage immutable objects before publishing discovery. A pending epoch
    /// grants no read proof or visibility until its discovery commit exists.
    pub fn staging(&self, epoch: &str) -> Project {
        let mut project = self.root.clone();
        project.prefix = format!(
            "{}/project/epochs/{epoch}",
            project.prefix.trim_end_matches('/')
        );
        project
    }
    /// A later reader can obtain earlier identities from sealed history. Never reverse this.
    pub fn reads(&self, proof: &Proof, alias: &str, epoch: &str, token: &str, now: u64) -> bool {
        let epochs = std::iter::once("root")
            .chain(self.keys.iter().map(|key| key.epoch.as_str()))
            .collect::<Vec<_>>();
        let Some(first) = epochs.iter().position(|candidate| *candidate == epoch) else {
            return false;
        };
        epochs[first..].iter().any(|epoch| {
            self.project(epoch)
                .is_some_and(|project| proof.valid("read", &project.binding(alias), token, now))
        })
    }
}
