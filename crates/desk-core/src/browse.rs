//! Optional encrypted directory pages. The marker is the commit point of the index.
use crate::format::Entry;
use anyhow::{bail, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Maximum direct children in one encrypted directory object.
pub const PAGE_ENTRIES: usize = 256;
/// New states at this size automatically receive a browser index.
pub const AUTO_INDEX_FILES: usize = 100_000;
/// Bound concurrent index PUTs and the corresponding encrypted buffers.
pub const UPLOAD_CONCURRENCY: usize = 8;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Child<'a> {
    Directory { name: &'a str, count: usize },
    File { name: &'a str, entry: &'a Entry },
}

pub fn marker_name(seq: u64, attempt: &str) -> String {
    format!("{seq:010}.{attempt}.browse.age")
}

pub fn page_name(seq: u64, attempt: &str, path: &str, page: usize) -> String {
    let digest = hex::encode(Sha256::digest(path.as_bytes()));
    format!("{seq:010}.{attempt}.{digest}.{page:08}.dir.age")
}

/// Borrow the tree entries rather than duplicate file chains in memory.
/// Directory counts include all descendant files, not only direct children.
pub fn directories(
    files: &BTreeMap<String, Entry>,
) -> Result<BTreeMap<&str, BTreeMap<&str, Child<'_>>>> {
    let mut dirs: BTreeMap<&str, BTreeMap<&str, Child<'_>>> = BTreeMap::new();
    dirs.insert("", BTreeMap::new());
    for (path, entry) in files {
        if path.is_empty()
            || path
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == ".." || s.contains('\0'))
        {
            bail!("the tree has an invalid file path");
        }
        let mut parent = "";
        for (offset, _) in path.match_indices('/') {
            let directory = &path[..offset];
            let name = directory.rsplit('/').next().unwrap();
            let children = dirs.entry(parent).or_default();
            match children
                .entry(name)
                .or_insert(Child::Directory { name, count: 0 })
            {
                Child::Directory { count, .. } => *count += 1,
                Child::File { .. } => bail!("the tree uses a path as both a file and a directory"),
            }
            dirs.entry(directory).or_default();
            parent = directory;
        }
        let name = path.rsplit('/').next().unwrap();
        if dirs
            .entry(parent)
            .or_default()
            .insert(name, Child::File { name, entry })
            .is_some()
        {
            bail!("the tree uses a path as both a file and a directory");
        }
    }
    Ok(dirs)
}

/// Physical state-relative key; API callers continue to use the flat logical name.
pub fn object_name(name: &str) -> String {
    if name.ends_with(".dir.age") {
        format!("browse/{name}")
    } else {
        name.to_string()
    }
}
