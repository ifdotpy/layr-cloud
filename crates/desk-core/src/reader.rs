//! The vault reader (docs/design.md, "Reader"): the states of one vault, their trees and the
//! content of their files. It decrypts with the vault identity and checks each file against
//! the size and SHA-256 that its tree names.

use crate::format::{self, Entry, Header, Tree};
use crate::range::{Partial, RangeData, Sparse, BLOCK};
use crate::{keys, zst};
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

/// Where the encrypted objects of one vault come from (S3, a gateway, a folder). Names are
/// relative to `<prefix>/states/`.
pub trait Store: Send + Sync {
    fn get(&self, name: &str) -> impl Future<Output = Result<Vec<u8>>> + Send;
    /// A bounded inclusive ciphertext range. None means ranges are not supported.
    fn get_range(
        &self,
        _name: &str,
        _start: u64,
        _end: u64,
    ) -> impl Future<Output = Result<Option<RangeData>>> + Send {
        async { Ok(None) }
    }
}

/// A decoded tree: its header and canonical bytes.
type DecodedTree = (Header, Arc<Vec<u8>>);

/// Decrypted data parts kept in memory, at most this many bytes; trees are kept apart.
const PART_CACHE: usize = 512 << 20;

#[derive(Default)]
struct Cache {
    parts: HashMap<String, Arc<Vec<u8>>>,
    bytes: usize,
    partial: HashMap<String, Partial>,
    trees: HashMap<(u64, String), DecodedTree>,
}

pub struct Reader<S> {
    store: S,
    identity: age::x25519::Identity,
    trees: BTreeMap<u64, Vec<String>>,
    cache: Mutex<Cache>,
}

impl<S: Store> Reader<S> {
    /// `names` are the object names below `states/` (a listing); `identity` is the vault
    /// identity.
    pub fn new(store: S, identity: &str, names: &[String]) -> Result<Self> {
        let identity = age::x25519::Identity::from_str(identity.trim())
            .map_err(|e| anyhow!("not an age identity: {e}"))?;
        let mut trees: BTreeMap<u64, Vec<String>> = BTreeMap::new();
        for name in names {
            if let Some((seq, attempt)) = format::parse_tree_name(name) {
                trees.entry(seq).or_default().push(attempt);
            }
        }
        for attempts in trees.values_mut() {
            attempts.sort();
        }
        Ok(Reader {
            store,
            identity,
            trees,
            cache: Mutex::default(),
        })
    }

    /// Every pushed state, oldest first. A second tree of one `seq` (after a lost reply) is
    /// harmless; the first attempt in sort order stands for it.
    pub fn states(&self) -> Vec<(u64, String)> {
        self.trees
            .iter()
            .map(|(seq, a)| (*seq, a[0].clone()))
            .collect()
    }

    pub fn latest(&self) -> Result<(u64, String)> {
        self.states()
            .pop()
            .ok_or_else(|| anyhow!("the vault has no state yet"))
    }

    async fn decrypted(&self, name: &str) -> Result<Vec<u8>> {
        let data = self
            .store
            .get(name)
            .await
            .with_context(|| format!("reading {name}"))?;
        keys::decrypt_with(&self.identity, &data).with_context(|| format!("decrypting {name}"))
    }

    async fn part(&self, name: &str) -> Result<Arc<Vec<u8>>> {
        if let Some(part) = self.cache.lock().unwrap().parts.get(name) {
            return Ok(part.clone());
        }
        let part = Arc::new(self.decrypted(name).await?);
        self.remember_part(name, part.clone());
        Ok(part)
    }

    fn remember_part(&self, name: &str, part: Arc<Vec<u8>>) {
        let mut cache = self.cache.lock().unwrap();
        if part.len() > PART_CACHE {
            return;
        }
        if cache.bytes + part.len() > PART_CACHE {
            cache.parts.clear();
            cache.partial.clear();
            cache.bytes = 0;
        }
        if let Some(old) = cache.parts.insert(name.to_string(), part.clone()) {
            cache.bytes -= old.len();
        }
        if let Some(old) = cache.partial.remove(name) {
            cache.bytes -= old.blocks.values().map(|b| b.len()).sum::<usize>();
        }
        cache.bytes += part.len();
    }

    async fn frame(&self, link: &format::Link, selective: bool) -> Result<Vec<u8>> {
        let name = link.object();
        let end = link
            .3
            .checked_add(link.4)
            .ok_or_else(|| anyhow!("frame range overflow"))?;
        let full = |part: &[u8]| -> Result<Vec<u8>> {
            let start = usize::try_from(link.3)?;
            let end = usize::try_from(end)?;
            Ok(part
                .get(start..end)
                .ok_or_else(|| anyhow!("a frame is outside its part"))?
                .to_vec())
        };
        if !selective || link.4 >= 512 * 1024 {
            return full(&self.part(&name).await?);
        }
        for attempt in 0..=8 {
            let partial = {
                let cache = self.cache.lock().unwrap();
                if let Some(part) = cache.parts.get(&name) {
                    return full(part);
                }
                cache.partial.get(&name).cloned().unwrap_or_default()
            };
            let missing = Arc::new(Mutex::new(None));
            let source = Sparse {
                partial: partial.clone(),
                missing: missing.clone(),
                pos: 0,
            };
            let decoded = (|| -> Result<Vec<u8>> {
                let mut reader = age::Decryptor::new_buffered(BufReader::new(source))?
                    .decrypt(std::iter::once(&self.identity as &dyn age::Identity))?;
                reader.seek(SeekFrom::Start(link.3))?;
                let mut bytes = vec![0; link.4 as usize];
                reader.read_exact(&mut bytes)?;
                Ok(bytes)
            })();
            let needed = *missing.lock().unwrap();
            let Some(start) = needed else {
                return decoded.context("decrypting file frame");
            };
            // Cached blocks remain useful after the budget; only another miss loads the full part.
            if partial.requests >= 8 || attempt == 8 {
                return full(&self.part(&name).await?);
            }
            let range_end = start
                .checked_add(BLOCK - 1)
                .ok_or_else(|| anyhow!("range overflow"))?;
            let Some(response) = self.store.get_range(&name, start, range_end).await? else {
                return full(&self.part(&name).await?);
            };
            response.validate(start, range_end)?;
            if partial.total.is_some_and(|n| n != response.total) {
                bail!("object length changed");
            }
            if response.start == 0 && response.bytes.len() as u64 == response.total {
                let part = Arc::new(keys::decrypt_with(&self.identity, &response.bytes)?);
                let frame = full(&part)?;
                self.remember_part(&name, part);
                return Ok(frame);
            }
            let mut cache = self.cache.lock().unwrap();
            if cache.bytes + response.bytes.len() > PART_CACHE {
                cache.parts.clear();
                cache.partial.clear();
                cache.bytes = 0;
            }
            let bytes = Arc::new(response.bytes);
            let p = cache.partial.entry(name.clone()).or_default();
            if p.total.is_some_and(|n| n != response.total) {
                bail!("object length changed");
            }
            p.total = Some(response.total);
            p.requests += 1;
            let old = p.blocks.insert(start, bytes.clone()).map_or(0, |b| b.len());
            cache.bytes = cache.bytes - old + bytes.len();
        }
        unreachable!("the last miss uses a full-object read")
    }

    /// The header of one state, without decoding its tree.
    pub async fn header(&self, seq: u64, attempt: &str) -> Result<Header> {
        if let Some((header, _)) = self
            .cache
            .lock()
            .unwrap()
            .trees
            .get(&(seq, attempt.to_string()))
        {
            return Ok(header.clone());
        }
        let object = self.decrypted(&format::name(seq, attempt, None)).await?;
        Ok(format::split_tree(&object)?.0)
    }

    /// The header and the canonical tree bytes of one state.
    pub async fn tree(&self, seq: u64, attempt: &str) -> Result<(Header, Arc<Vec<u8>>)> {
        // Walk back to the nearest full tree (or one already decoded), then decode forward.
        let mut pending = Vec::new();
        let mut at = (seq, attempt.to_string());
        let mut canonical = loop {
            if let Some((header, bytes)) = self.cache.lock().unwrap().trees.get(&at) {
                if pending.is_empty() {
                    return Ok((header.clone(), bytes.clone()));
                }
                break bytes.clone();
            }
            let object = self.decrypted(&format::name(at.0, &at.1, None)).await?;
            let (header, frame) = format::split_tree(&object)?;
            if (header.seq, header.attempt.as_str()) != (at.0, at.1.as_str()) {
                bail!(
                    "tree {}.{} names itself {}.{}",
                    at.0,
                    at.1,
                    header.seq,
                    header.attempt
                );
            }
            if header.tree == "full" {
                let bytes = Arc::new(zst::decompress(&frame, None)?);
                self.remember(&header, &bytes);
                if pending.is_empty() {
                    return Ok((header, bytes));
                }
                break bytes;
            }
            if pending.len() > self.trees.len() {
                bail!("the tree chain of {seq}.{attempt} has a loop");
            }
            let base = header.base.clone().expect("checked in split_tree");
            pending.push((header, frame.to_vec()));
            at = base;
        };
        let mut last = None;
        while let Some((header, frame)) = pending.pop() {
            canonical = Arc::new(zst::decompress(&frame, Some(&canonical))?);
            self.remember(&header, &canonical);
            last = Some(header);
        }
        Ok((last.expect("one delta at least"), canonical))
    }

    fn remember(&self, header: &Header, bytes: &Arc<Vec<u8>>) {
        let key = (header.seq, header.attempt.clone());
        self.cache
            .lock()
            .unwrap()
            .trees
            .insert(key, (header.clone(), bytes.clone()));
    }

    /// `path -> entry` of one state (the latest when `state` is none).
    pub async fn files(&self, state: Option<(u64, String)>) -> Result<BTreeMap<String, Entry>> {
        let (seq, attempt) = match state {
            Some(s) => s,
            None => self.latest()?,
        };
        let (_, canonical) = self.tree(seq, &attempt).await?;
        let tree: Tree = serde_json::from_slice(&canonical).context("the tree is not valid")?;
        for path in tree.files.keys() {
            if !safe_path(path) {
                bail!("the tree names an unsafe path {path:?}");
            }
        }
        Ok(tree.files)
    }

    /// The content of one tree entry: its chain decoded from the full frame up, checked against
    /// the entry's size and SHA-256.
    pub async fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        self.read_inner(entry, true).await
    }

    /// Full-object reads for restore and verification of a complete state.
    pub async fn read_full(&self, entry: &Entry) -> Result<Vec<u8>> {
        self.read_inner(entry, false).await
    }

    async fn read_inner(&self, entry: &Entry, selective: bool) -> Result<Vec<u8>> {
        let chain = &entry.3;
        if chain.is_empty() || chain.last().is_some_and(|l| l.is_delta()) {
            bail!("a file chain must end with a full frame");
        }
        let mut data: Option<Vec<u8>> = None;
        for link in chain.iter().rev() {
            let frame = self.frame(link, selective).await?;
            data = Some(if link.is_delta() {
                zst::decompress(&frame, data.as_deref())?
            } else {
                zst::decompress(&frame, None)?
            });
        }
        let data = data.unwrap_or_default();
        if data.len() as u64 != entry.size() || hex::encode(Sha256::digest(&data)) != entry.sha256()
        {
            bail!("the content does not match its tree entry");
        }
        Ok(data)
    }
}

/// A relative path without `.`, `..` or empty parts, as the writer makes them.
pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
