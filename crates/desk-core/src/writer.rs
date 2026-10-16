//! The writer (docs/design.md, "Writer"): one push puts a state of a layr project into its
//! vault. It holds only the vault recipient, and its bucket key may only put new objects and
//! list names.
//!
//! The state folder is the one of the Python writer (python/vault_writer.py), so either writer
//! continues the other:
//! - `writer.json`: the last push, `{"seq", "attempt", "state", "side", "since_full"}`;
//! - `tree.json`: the canonical tree of that push, the base of the next delta tree;
//! - `a/` and `b/`: two `layr export` folders that take turns; `side` names the one with the
//!   last pushed state, whose files are the bases of file deltas.

use crate::canonical::canonical;
use crate::format::{self, Entry, Link, Tree};
use crate::zst;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// A data part holds frames up to this size; a larger frame has a part of its own.
pub const PART_LIMIT: usize = 16 << 20;
/// A full tree at least every this many pushes.
pub const FULL_TREE_EVERY: u64 = 32;
/// A file delta only on a chain shorter than this.
pub const MAX_DEPTH: usize = 16;
/// A file delta only for content up to this size.
pub const DELTA_LIMIT: usize = 32 << 20;
/// The file that `layr export` adds to its folder.
pub const MARKER: &str = ".layr-export.json";

/// The layr command of one project: `run(args)` returns its standard output.
pub trait Layr {
    fn run(&self, args: &[&str]) -> Result<String>;
}

/// The bucket of a vault as a write-only key sees it. Keys are full object names.
pub trait Bucket {
    /// Read an index object only with explicitly supplied recovery credentials.
    fn get_for_recovery(
        &self,
        _key: &str,
        _expected_size: usize,
    ) -> impl Future<Output = Result<Option<Vec<u8>>>> + Send {
        async { bail!("this bucket has no index recovery reader") }
    }
    /// Put an object that must not exist yet: true when written, false when it exists.
    fn put_new(&self, key: &str, body: Vec<u8>) -> impl Future<Output = Result<bool>> + Send;
    /// Every key below `prefix`.
    fn list(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>>> + Send;
    /// Direct objects only; cloud stores should use a server-side delimiter.
    fn list_direct(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>>> + Send {
        let listing = self.list(prefix);
        async move {
            Ok(listing
                .await?
                .into_iter()
                .filter(|key| {
                    key.strip_prefix(prefix)
                        .is_some_and(|rest| !rest.contains('/'))
                })
                .collect())
        }
    }
}

/// `writer.json`: the last push.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Record {
    pub seq: u64,
    pub attempt: String,
    pub state: String,
    /// The export folder (`a` or `b`) with the state of this push.
    pub side: String,
    /// Delta trees since the last full tree.
    pub since_full: u64,
}

impl Record {
    /// The record as Python's `json.dumps(record)` writes it.
    pub fn to_json(&self) -> String {
        let mut out = format!("{{\"seq\": {}, \"attempt\": ", self.seq);
        crate::canonical::string(&self.attempt, &mut out);
        out.push_str(", \"state\": ");
        crate::canonical::string(&self.state, &mut out);
        out.push_str(", \"side\": ");
        crate::canonical::string(&self.side, &mut out);
        out.push_str(&format!(", \"since_full\": {}}}", self.since_full));
        out
    }
}

/// What one push did.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub pushed: bool,
    pub seq: u64,
    pub state: String,
    /// New data frames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<u32>,
    /// Plaintext bytes of the data parts and the tree frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// Why a push after an earlier one is full.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The last push as the state folder holds it: its record, its canonical tree and the files of
/// that tree.
struct Last {
    record: Record,
    tree: Vec<u8>,
    files: BTreeMap<String, Entry>,
}

pub struct Writer<L, B, E> {
    dir: PathBuf,
    prefix: String,
    layr: L,
    bucket: B,
    encrypt: E,
    browse_scope: String,
}

impl<L, B, E> Writer<L, B, E>
where
    L: Layr,
    B: Bucket,
    E: Fn(&[u8]) -> Result<Vec<u8>>,
{
    /// `dir` is the state folder (made when missing, private to its owner); `prefix` is the
    /// vault's prefix in the bucket; `encrypt` encrypts to the vault recipient.
    /// `browse_scope` identifies the bucket, endpoint and recipient for journal replay.
    pub fn new(
        dir: impl AsRef<Path>,
        prefix: &str,
        layr: L,
        bucket: B,
        encrypt: E,
        browse_scope: &str,
    ) -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        // layr runs in another folder, so the state folder is absolute.
        let dir = std::path::absolute(dir.as_ref())?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .with_context(|| format!("making {}", dir.display()))?;
        Ok(Writer {
            dir,
            prefix: prefix.to_string(),
            layr,
            bucket,
            encrypt,
            browse_scope: browse_scope.to_string(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `writer.json`, when there is one.
    pub fn record(&self) -> Result<Option<Record>> {
        match std::fs::read(self.dir.join("writer.json")) {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes).context("writer.json is not a writer record")?,
            )),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("reading writer.json"),
        }
    }

    /// The last push, when the record and the tree of the state folder belong together; else
    /// why not. Without it the push is full and takes its `seq` from the bucket, which is
    /// right after any stop.
    fn last(&self) -> Result<(Option<Last>, Option<String>)> {
        let bytes = match std::fs::read(self.dir.join("writer.json")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok((None, None)),
            Err(e) => return Err(e).context("reading writer.json"),
        };
        let unusable = |why: &str| Ok((None, Some(format!("{why}, so this push is full"))));
        let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
            return unusable("writer.json is not a writer record");
        };
        if record.side != "a" && record.side != "b" {
            return unusable("writer.json names no export folder");
        }
        // The Python writer writes tree.json, then writer.tmp, then renames it: a left
        // writer.tmp means that tree.json may be newer than the record.
        if self.dir.join("writer.tmp").exists() {
            return unusable("writer.tmp is left from a push that stopped before its record");
        }
        let tree = match std::fs::read(self.dir.join("tree.json")) {
            Ok(tree) => tree,
            Err(e) if e.kind() == ErrorKind::NotFound => return unusable("tree.json is missing"),
            Err(e) => return Err(e).context("reading tree.json"),
        };
        let Ok(parsed) = serde_json::from_slice::<Tree>(&tree) else {
            return unusable("tree.json is not a tree");
        };
        if parsed
            .files
            .values()
            .any(|e| e.3.iter().any(|link| link.0 > record.seq))
        {
            return unusable("tree.json is newer than writer.json");
        }
        Ok((
            Some(Last {
                record,
                tree,
                files: parsed.files,
            }),
            None,
        ))
    }

    fn key(&self, seq: u64, attempt: &str, part: Option<u32>) -> String {
        format!(
            "{}/states/{}",
            self.prefix,
            format::name(seq, attempt, part)
        )
    }

    /// The `seq` after the highest tree in the bucket.
    async fn next_seq_without_record(&self) -> Result<u64> {
        let mut highest = 0;
        for key in self
            .bucket
            .list_direct(&format!("{}/states/", self.prefix))
            .await?
        {
            let name = key.rsplit('/').next().unwrap_or_default();
            if !name.ends_with(".tree.age") {
                continue;
            }
            let seq: u64 = name
                .split('.')
                .next()
                .unwrap_or_default()
                .parse()
                .with_context(|| format!("the tree object {key} has no number"))?;
            highest = highest.max(seq);
        }
        Ok(highest + 1)
    }

    async fn put_part(&self, seq: u64, attempt: &str, part: u32, data: &[u8]) -> Result<()> {
        // Nothing puts a key twice, so a part that exists belongs to another push that drew
        // the same attempt; this push stops, and the next one draws again.
        if part >= format::MAX_DATA_PARTS {
            bail!("the state exceeds the four-digit data part limit");
        }
        let key = self.key(seq, attempt, Some(part));
        if !self.bucket.put_new(&key, (self.encrypt)(data)?).await? {
            bail!("the part {key} exists before its first put");
        }
        Ok(())
    }

    /// Add an index to the last trusted local writer tree. No data objects are read.
    /// Existing or uncertain writes fail closed; no immutable object is overwritten.
    pub async fn reindex(&self) -> Result<serde_json::Value> {
        self.reindex_mode(false).await
    }

    /// Resolve pending index writes with explicit read credentials and their saved receipts.
    pub async fn resume_index(&self) -> Result<serde_json::Value> {
        self.reindex_mode(true).await
    }

    fn lock(&self) -> Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join("writer.lock"))?;
        file.try_lock()
            .context("another writer operation owns this state folder")?;
        Ok(file)
    }

    async fn reindex_mode(&self, resume: bool) -> Result<serde_json::Value> {
        let _lock = self.lock()?;
        let (last, why) = self.last()?;
        let last = last.ok_or_else(|| {
            anyhow!(
                "cannot index local writer state: {}",
                why.unwrap_or_else(|| "no writer record".into())
            )
        })?;
        if format::parse_tree_name(&format::name(last.record.seq, &last.record.attempt, None))
            .is_none()
        {
            bail!("the writer record has an invalid state identity");
        }
        let tree_key = self.key(last.record.seq, &last.record.attempt, None);
        if !self.bucket.list(&tree_key).await?.contains(&tree_key) {
            bail!("the writer record's tree is absent from the bucket");
        }
        let tree_sha256 = hex::encode(Sha256::digest(&last.tree));
        drop(last.tree);
        self.publish_browse(&last.record, &last.files, &tree_sha256, resume)
            .await
    }

    async fn publish_browse(
        &self,
        record: &Record,
        files: &BTreeMap<String, Entry>,
        tree_sha256: &str,
        resume: bool,
    ) -> Result<serde_json::Value> {
        use crate::browse::{directories, marker_name, page_name, PAGE_ENTRIES};
        let journal = self
            .dir
            .join(format!("browse-{:010}.{}", record.seq, record.attempt));
        std::fs::create_dir_all(&journal)?;
        let binding = serde_json::to_vec(
            &json!({"directoryPrefix": "states/browse/", "scope": self.browse_scope, "prefix": self.prefix, "record": record.to_json(),
            "treeSha256": tree_sha256}),
        )?;
        let binding_path = journal.join("binding.json");
        if binding_path.exists() {
            if std::fs::read(&binding_path)? != binding {
                bail!("the browse journal belongs to different writer inputs");
            }
        } else {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&binding_path)?;
            file.write_all(&binding)?;
            file.sync_all()?;
        }
        if journal
            .join(format!("{}.done", marker_name(record.seq, &record.attempt)))
            .is_file()
        {
            return Ok(
                json!({"indexed": true, "seq": record.seq, "attempt": record.attempt,
                "files": files.len(), "reused": true}),
            );
        }
        let dirs = directories(files)?;
        let total_pages: usize = dirs
            .values()
            .map(|children| children.len().div_ceil(PAGE_ENTRIES).max(1))
            .sum();
        eprintln!(
            "browse index: {} files, {} directories, {total_pages} pages",
            files.len(),
            dirs.len()
        );
        let mut pages = 0usize;
        let mut pending = Vec::new();
        let journal_ref = &journal;
        for (path, children) in dirs {
            let children: Vec<_> = children.into_values().collect();
            let count = children.len().div_ceil(PAGE_ENTRIES).max(1);
            if count > 100_000_000 {
                bail!("directory exceeds the eight-digit page limit");
            }
            for page in 0..count {
                let start = page * PAGE_ENTRIES;
                let entries = &children[start..(start + PAGE_ENTRIES).min(children.len())];
                let object = json!({"version": 1, "seq": record.seq, "attempt": record.attempt,
                    "path": path, "page": page, "total": children.len(), "entries": entries,
                    "next": if page + 1 < count { Some(page + 1) } else { None }});
                let name = page_name(record.seq, &record.attempt, path, page);
                pending.push(
                    async move { self.put_browse(journal_ref, &name, &object, resume).await },
                );
                pages += 1;
                if pending.len() == crate::browse::UPLOAD_CONCURRENCY {
                    // Await every in-flight write even when one fails, so successful receipts persist.
                    for result in futures_util::future::join_all(pending.drain(..)).await {
                        result?;
                    }
                    if pages.is_multiple_of(256) {
                        eprintln!("browse index: {pages}/{total_pages} pages confirmed");
                    }
                }
            }
        }
        for result in futures_util::future::join_all(pending).await {
            result?;
        }
        let marker = json!({"version": 1, "seq": record.seq, "attempt": record.attempt,
            "state": record.state, "files": files.len()});
        self.put_browse(
            &journal,
            &marker_name(record.seq, &record.attempt),
            &marker,
            resume,
        )
        .await?;
        Ok(
            json!({"indexed": true, "seq": record.seq, "attempt": record.attempt,
            "files": files.len(), "pages": pages}),
        )
    }

    async fn put_browse(
        &self,
        journal: &Path,
        name: &str,
        object: &serde_json::Value,
        resume: bool,
    ) -> Result<()> {
        let key = format!(
            "{}/states/{}",
            self.prefix,
            crate::browse::object_name(name)
        );
        let done = journal.join(format!("{name}.done"));
        if done.exists() {
            return Ok(());
        }
        let pending = journal.join(format!("{name}.pending"));
        let receipt = journal.join(format!("{name}.receipt.json"));
        let plain = serde_json::to_vec(object)?;
        let plain_sha = hex::encode(Sha256::digest(&plain));
        let binding_sha = hex::encode(Sha256::digest(std::fs::read(journal.join("binding.json"))?));
        let expected = |sealed: &[u8]| {
            json!({"version": 1, "key": key, "bindingSha256": binding_sha,
            "plaintextSha256": plain_sha, "ciphertextSha256": hex::encode(Sha256::digest(sealed)),
            "ciphertextBytes": sealed.len()})
        };
        let mut sealed = None;
        let mut needs_verification = false;
        if pending.exists() {
            if !resume {
                bail!("browse write {key} has an uncertain prior outcome; use reindex --resume with recovery read credentials");
            }
            let saved = std::fs::read(&pending)?;
            if receipt.exists() {
                let recorded: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&receipt)?)?;
                if recorded != expected(&saved) {
                    bail!("the pending index receipt does not match its expected object or ciphertext");
                }
                sealed = Some(saved);
                needs_verification = true;
            } else {
                // Old journals can contain a partial file from a crash during write.
                // An absent remote object permits a new seal of the expected plaintext.
                // Keep the old ciphertext for inspection; never treat it as authenticated.
                if self
                    .bucket
                    .get_for_recovery(&key, saved.len())
                    .await?
                    .is_some()
                {
                    bail!("legacy pending index object exists remotely without an authenticated receipt; refuse recovery");
                }
                let legacy = journal.join(format!(
                    "{name}.legacy.{}",
                    hex::encode(Sha256::digest(&saved))
                ));
                if legacy.exists() {
                    bail!("the legacy index recovery archive already exists");
                }
                std::fs::rename(&pending, legacy)?;
                std::fs::File::open(journal)?.sync_all()?;
            }
        }
        let sealed = match sealed {
            Some(bytes) => bytes,
            None => {
                let bytes = (self.encrypt)(&plain)?;
                // The receipt precedes the atomic pending file. No PUT starts until both
                // files and the directory are durable. Interrupted temporary files are inert.
                self.browse_atomic(&receipt, &serde_json::to_vec(&expected(&bytes))?)?;
                self.browse_atomic(&pending, &bytes)?;
                bytes
            }
        };
        if needs_verification {
            if let Some(remote) = self.bucket.get_for_recovery(&key, sealed.len()).await? {
                if remote != sealed {
                    bail!("the remote index ciphertext differs from the pending write");
                }
                std::fs::rename(pending, done)?;
                std::fs::File::open(journal)?.sync_all()?;
                return Ok(());
            }
        }
        if !self.bucket.put_new(&key, sealed.clone()).await? {
            if !resume {
                bail!("browse object {key} already exists; refuse overwrite or unverified retry");
            }
            match self.bucket.get_for_recovery(&key, sealed.len()).await? {
                Some(remote) if remote == sealed => {},
                _ => bail!("the conditional index write conflicts with a different or missing remote object"),
            }
        }
        std::fs::rename(pending, done)?;
        std::fs::File::open(journal)?.sync_all()?;
        Ok(())
    }

    fn browse_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let temporary = path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        std::fs::File::open(path.parent().unwrap())?.sync_all()?;
        Ok(())
    }

    /// Push the layr state `head`. The parts go first and the tree last: a push that stops
    /// before its tree leaves no state, and the next push uses a new attempt.
    pub async fn push(&self, head: &str) -> Result<Summary> {
        self.push_with_index_threshold(head, crate::browse::AUTO_INDEX_FILES)
            .await
    }

    async fn push_with_index_threshold(
        &self,
        head: &str,
        index_threshold: usize,
    ) -> Result<Summary> {
        let _lock = self.lock()?;
        let (last, note) = self.last()?;
        if let Some(last) = &last {
            if last.record.state == head {
                if last.files.len() >= index_threshold {
                    self.publish_browse(
                        &last.record,
                        &last.files,
                        &hex::encode(Sha256::digest(&last.tree)),
                        false,
                    )
                    .await
                    .context("the state was pushed, but its browser index is incomplete")?;
                }
                return Ok(Summary {
                    pushed: false,
                    seq: last.record.seq,
                    state: head.to_string(),
                    files: None,
                    parts: None,
                    bytes: None,
                    note: None,
                });
            }
        }
        let seq = match &last {
            Some(last) => last.record.seq + 1,
            None => self.next_seq_without_record().await?,
        };
        let mut random = [0u8; 4];
        getrandom::getrandom(&mut random).map_err(|e| anyhow!("no random bytes: {e}"))?;
        let attempt = hex::encode(random);
        let side = match &last {
            Some(last) if last.record.side == "a" => "b",
            _ => "a",
        };
        let new_dir = self.dir.join(side);
        let base_dir = self.dir.join(if side == "a" { "b" } else { "a" });
        if last.is_none() {
            match std::fs::remove_dir_all(&new_dir) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        let new_path = new_dir
            .to_str()
            .ok_or_else(|| anyhow!("the state folder is not UTF-8"))?;
        self.layr.run(&["export", head, new_path, "--all", "-q"])?;
        let new_files = walk(&new_dir)?;
        let part_limit = data_part_limit(&new_files)?;
        let none = BTreeMap::new();
        let old_files = last.as_ref().map_or(&none, |l| &l.files);
        let changed = match &last {
            Some(last) => {
                let diff = self
                    .layr
                    .run(&["diff", "--name-status", &last.record.state, head])?;
                changed_paths(&diff, &new_files, old_files)
            }
            None => new_files.keys().cloned().collect(),
        };

        let mut files: BTreeMap<String, Entry> = old_files
            .iter()
            .filter(|(path, _)| new_files.contains_key(*path))
            .map(|(path, entry)| (path.clone(), entry.clone()))
            .collect();
        let (mut current, mut put, mut frames, mut bytes) = (Vec::new(), 0u32, 0usize, 0u64);
        for path in &changed {
            let Some(source) = new_files.get(path) else {
                files.remove(path);
                continue;
            };
            let (data, mode) = content(source)?;
            let digest = hex::encode(Sha256::digest(&data));
            let previous = old_files.get(path);
            if let Some(previous) = previous.filter(|p| p.2 == digest) {
                files.insert(
                    path.clone(),
                    Entry(mode.into(), data.len() as u64, digest, previous.3.clone()),
                );
                continue;
            }
            let (frame, chain) = frame(&data, previous, &base_dir.join(path))?;
            if !current.is_empty() && current.len() + frame.len() > part_limit {
                self.put_part(seq, &attempt, put, &current).await?;
                bytes += current.len() as u64;
                put += 1;
                current.clear();
            }
            let kind = if chain.is_empty() { "full" } else { "delta" };
            let link = Link(
                seq,
                attempt.clone(),
                put,
                current.len() as u64,
                frame.len() as u64,
                kind.into(),
            );
            current.extend_from_slice(&frame);
            let chain = std::iter::once(link).chain(chain).collect();
            files.insert(
                path.clone(),
                Entry(mode.into(), data.len() as u64, digest, chain),
            );
            frames += 1;
        }
        if !current.is_empty() || put == 0 {
            self.put_part(seq, &attempt, put, &current).await?;
            bytes += current.len() as u64;
            put += 1;
        }

        let tree = canonical(&tree_value(&files))?;
        let base = match &last {
            Some(last) if last.record.since_full + 1 < FULL_TREE_EVERY => Some(last),
            _ => None,
        };
        let body = zst::compress(&tree, base.map(|l| l.tree.as_slice()))?;
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let header = json!({
            "v": 1, "seq": seq, "attempt": attempt, "state": head, "time": time,
            "tree": if base.is_some() { "delta" } else { "full" },
            "base": base.map(|l| json!([l.record.seq, l.record.attempt])),
            "parts": put,
        });
        let mut object = canonical(&header)?;
        object.push(b'\n');
        object.extend_from_slice(&body);
        if !self
            .bucket
            .put_new(&self.key(seq, &attempt, None), (self.encrypt)(&object)?)
            .await?
        {
            bail!("the tree object exists before its first put");
        }
        let record = Record {
            seq,
            attempt,
            state: head.to_string(),
            side: side.to_string(),
            since_full: base.map_or(0, |l| l.record.since_full + 1),
        };
        self.commit(&tree, &record)?;
        if files.len() >= index_threshold {
            self.publish_browse(&record, &files, &hex::encode(Sha256::digest(&tree)), false)
                .await
                .context("the state was pushed, but its browser index is incomplete")?;
        }
        Ok(Summary {
            pushed: true,
            seq,
            state: head.to_string(),
            files: Some(frames),
            parts: Some(put),
            bytes: Some(bytes + body.len() as u64),
            note,
        })
    }

    /// Keep the pushed tree and its record. The old record goes first: a stop before the new
    /// one is written leaves no record, and the next push is full.
    fn commit(&self, tree: &[u8], record: &Record) -> Result<()> {
        match std::fs::remove_file(self.dir.join("writer.json")) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        replace(&self.dir, "tree.tmp", "tree.json", tree)?;
        replace(
            &self.dir,
            "writer.tmp",
            "writer.json",
            record.to_json().as_bytes(),
        )?;
        std::fs::File::open(&self.dir)?.sync_all()?;
        Ok(())
    }
}

/// Write `name` through a temporary file, so that it is whole or absent after a stop.
fn replace(dir: &Path, temporary: &str, name: &str, data: &[u8]) -> Result<()> {
    let mut file = std::fs::File::create(dir.join(temporary))?;
    file.write_all(data)?;
    file.sync_all()?;
    std::fs::rename(dir.join(temporary), dir.join(name))?;
    Ok(())
}

/// The frame of one changed file and the chain that it continues (empty for a full frame): a
/// delta against the version of the last push when its chain is short, the base folder still
/// holds that version, and the delta is smaller.
fn frame(data: &[u8], previous: Option<&Entry>, base: &Path) -> Result<(Vec<u8>, Vec<Link>)> {
    let full = zst::compress(data, None)?;
    let Some(previous) = previous else {
        return Ok((full, Vec::new()));
    };
    if previous.3.len() > MAX_DEPTH
        || data.len() > DELTA_LIMIT
        || previous.size() > DELTA_LIMIT as u64
    {
        return Ok((full, Vec::new()));
    }
    // The base folder only helps compression; a base that is missing or not that version
    // gives a full frame.
    let Ok((base, _)) = content(base) else {
        return Ok((full, Vec::new()));
    };
    if base.is_empty() || hex::encode(Sha256::digest(&base)) != previous.2 {
        return Ok((full, Vec::new()));
    }
    let delta = zst::compress(data, Some(&base))?;
    if delta.len() < full.len() {
        Ok((delta, previous.3.clone()))
    } else {
        Ok((full, Vec::new()))
    }
}

/// The paths to look at: the paths that `layr diff --name-status` names (both of a rename),
/// every added or removed path, and every path with a tab or a line break, which a diff line
/// cannot carry. A looked-at file with unchanged content keeps its frames.
fn changed_paths(
    diff: &str,
    new: &BTreeMap<String, PathBuf>,
    old: &BTreeMap<String, Entry>,
) -> BTreeSet<String> {
    let mut changed: BTreeSet<String> = diff
        .split('\n')
        .flat_map(|line| line.split('\t').skip(1))
        .map(str::to_string)
        .collect();
    changed.extend(
        new.keys()
            .filter(|p| !old.contains_key(*p) || p.contains(['\t', '\n']))
            .cloned(),
    );
    changed.extend(old.keys().filter(|p| !new.contains_key(*p)).cloned());
    changed.retain(|p| new.contains_key(p) || old.contains_key(p));
    changed
}

/// `{"files": {path: [mode, size, sha256, chain]}}`.
fn tree_value(files: &BTreeMap<String, Entry>) -> Value {
    let files: Map<String, Value> = files
        .iter()
        .map(|(path, Entry(mode, size, sha, chain))| {
            let chain: Vec<Value> = chain
                .iter()
                .map(|Link(seq, attempt, part, offset, length, kind)| {
                    json!([seq, attempt, part, offset, length, kind])
                })
                .collect();
            (path.clone(), json!([mode, size, sha, chain]))
        })
        .collect();
    json!({ "files": files })
}

/// Every file and link below `root` by its relative path, without the export marker. A link to
/// a folder is a link. Paths are UTF-8, as the tree names them in JSON.
pub fn walk(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut found = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries =
            std::fs::read_dir(&folder).with_context(|| format!("reading {}", folder.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                folders.push(path);
                continue;
            }
            let relative = path.strip_prefix(root)?;
            let relative = relative
                .to_str()
                .ok_or_else(|| anyhow!("the path {relative:?} is not UTF-8"))?;
            if !(kind.is_file() || kind.is_symlink()) {
                bail!("{relative} is neither a file, a folder nor a link");
            }
            if relative != MARKER {
                found.insert(relative.to_string(), path);
            }
        }
    }
    Ok(found)
}

/// The bytes and mode of a file or link: the content of a link is its target.
pub fn content(path: &Path) -> Result<(Vec<u8>, &'static str)> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok((
            std::fs::read_link(path)?.as_os_str().as_bytes().to_vec(),
            "120000",
        ));
    }
    let mode = if meta.permissions().mode() & 0o111 != 0 {
        "100755"
    } else {
        "100644"
    };
    Ok((std::fs::read(path)?, mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_head_retry_does_not_hide_an_incomplete_browser_index() {
        use std::sync::Mutex;
        struct OneFile;
        impl Layr for OneFile {
            fn run(&self, args: &[&str]) -> Result<String> {
                match args {
                    ["export", _, path, "--all", "-q"] => {
                        std::fs::create_dir_all(path)?;
                        std::fs::write(Path::new(path).join("one.txt"), b"one")?;
                        Ok(String::new())
                    }
                    _ => bail!("unexpected layr call"),
                }
            }
        }
        #[derive(Default)]
        struct Memory {
            objects: Mutex<BTreeMap<String, Vec<u8>>>,
        }
        impl Bucket for Memory {
            async fn put_new(&self, key: &str, bytes: Vec<u8>) -> Result<bool> {
                if key.ends_with(".dir.age") {
                    bail!("lost index response");
                }
                let mut objects = self.objects.lock().unwrap();
                if objects.contains_key(key) {
                    return Ok(false);
                }
                objects.insert(key.into(), bytes);
                Ok(true)
            }
            async fn list(&self, prefix: &str) -> Result<Vec<String>> {
                Ok(self
                    .objects
                    .lock()
                    .unwrap()
                    .keys()
                    .filter(|key| key.starts_with(prefix))
                    .cloned()
                    .collect())
            }
        }
        struct Temporary(PathBuf);
        impl Drop for Temporary {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let dir = Temporary(
            std::env::temp_dir().join(format!("desk-index-retry-{}", hex::encode(random))),
        );
        let writer = Writer::new(
            &dir.0,
            "project",
            OneFile,
            Memory::default(),
            |data: &[u8]| Ok(data.to_vec()),
            "test",
        )
        .unwrap();
        // The production caller always uses AUTO_INDEX_FILES. This tiny fixture selects one.
        assert!(writer.push_with_index_threshold("s1", 1).await.is_err());
        assert_eq!(writer.record().unwrap().unwrap().state, "s1");
        let error = writer.push_with_index_threshold("s1", 1).await.unwrap_err();
        assert!(format!("{error:#}").contains("uncertain prior outcome"));
        assert!(!writer
            .bucket
            .objects
            .lock()
            .unwrap()
            .keys()
            .any(|key| key.ends_with(".browse.age")));
    }

    #[test]
    fn the_record_is_written_as_python_writes_it() {
        let record = Record {
            seq: 12,
            attempt: "0a1b2c3d".into(),
            state: "st\u{e9}".into(),
            side: "b".into(),
            since_full: 3,
        };
        // python3: json.dumps({"seq": 12, "attempt": "0a1b2c3d", "state": "sté", ...})
        assert_eq!(
            record.to_json(),
            r#"{"seq": 12, "attempt": "0a1b2c3d", "state": "st\u00e9", "side": "b", "since_full": 3}"#
        );
        assert_eq!(
            serde_json::from_str::<Record>(&record.to_json()).unwrap(),
            record
        );
    }

    #[test]
    fn diff_lines_and_names_that_they_cannot_carry() {
        let entry = || Entry("100644".into(), 0, String::new(), Vec::new());
        let new: BTreeMap<String, PathBuf> = ["kept", "changed", "added", "renamed", "tab\there"]
            .into_iter()
            .map(|p| (p.to_string(), PathBuf::new()))
            .collect();
        let old: BTreeMap<String, Entry> = ["kept", "changed", "removed", "old", "tab\there"]
            .into_iter()
            .map(|p| (p.to_string(), entry()))
            .collect();
        let diff = "M\tchanged\nR100\told\trenamed\nM\ttab\there\n";
        let changed = changed_paths(diff, &new, &old);
        let want = ["added", "changed", "old", "removed", "renamed", "tab\there"];
        assert_eq!(changed.into_iter().collect::<Vec<_>>(), want);
    }
}

/// Keep the existing four-digit object names for large trees. Two consecutive output
/// parts together exceed the target size, except the last part. A zstd bound per file
/// therefore gives a conservative target with room for both sides of each boundary.
fn data_part_limit(files: &BTreeMap<String, PathBuf>) -> Result<usize> {
    let mut bound = 0usize;
    for path in files.values() {
        let size = usize::try_from(std::fs::symlink_metadata(path)?.len())?;
        bound = bound
            .checked_add(zstd_safe::compress_bound(size))
            .context("the tree size exceeds this platform's address space")?;
    }
    Ok(PART_LIMIT.max(bound.div_ceil((format::MAX_DATA_PARTS as usize - 1) / 2)))
}
