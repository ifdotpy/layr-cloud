use anyhow::{bail, Result};
use desk_core::range::{ContentRange, RangeData};
use desk_core::{keys, zst, Entry, Link, Reader, Store};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Memory {
    bytes: Arc<Vec<u8>>,
    calls: Arc<Mutex<Vec<(u64, u64)>>>,
    full: Arc<Mutex<usize>>,
    mode: u8,
}
impl Store for Memory {
    async fn get(&self, _: &str) -> Result<Vec<u8>> {
        *self.full.lock().unwrap() += 1;
        Ok(self.bytes.as_ref().clone())
    }
    async fn get_range(&self, _: &str, start: u64, end: u64) -> Result<Option<RangeData>> {
        if self.mode == 1 {
            return Ok(None);
        }
        self.calls.lock().unwrap().push((start, end));
        if self.mode == 2 {
            return Ok(Some(RangeData {
                start: 0,
                total: self.bytes.len() as u64,
                bytes: self.bytes.as_ref().clone(),
            }));
        }
        if start >= self.bytes.len() as u64 {
            bail!("416");
        }
        let end = (end as usize + 1).min(self.bytes.len());
        Ok(Some(RangeData {
            start: if self.mode == 3 { start + 1 } else { start },
            total: self.bytes.len() as u64,
            bytes: self.bytes[start as usize..end].to_vec(),
        }))
    }
}
fn random(n: usize) -> Vec<u8> {
    let mut x = 1234567u64;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}
fn fixture() -> (String, Memory, Vec<Entry>, Vec<Vec<u8>>) {
    let id = keys::generate_identity();
    let recipient = keys::recipient_of(&id).unwrap();
    let mut plain = random(2 * 1024 * 1024);
    let mut entries = Vec::new();
    let mut expected = Vec::new();
    for i in 0..12 {
        let data = random(2000 + i * 11);
        let frame = zst::compress(&data, None).unwrap();
        entries.push(Entry(
            "100644".into(),
            data.len() as u64,
            hex::encode(Sha256::digest(&data)),
            vec![Link(
                1,
                "aaaaaaaa".into(),
                0,
                plain.len() as u64,
                frame.len() as u64,
                "full".into(),
            )],
        ));
        plain.extend(frame);
        plain.extend(random(256 * 1024));
        expected.push(data);
    }
    let memory = Memory {
        bytes: Arc::new(keys::encrypt(&recipient, &plain).unwrap()),
        calls: Arc::default(),
        full: Arc::default(),
        mode: 0,
    };
    (id, memory, entries, expected)
}
#[tokio::test]
async fn selective_reads_fetch_only_needed_blocks_and_reuse_them() {
    let (id, store, entries, expected) = fixture();
    let r = Reader::new(store.clone(), &id, &[]).unwrap();
    assert_eq!(r.read(&entries[0]).await.unwrap(), expected[0]);
    let calls = store.calls.lock().unwrap().len();
    assert!(calls <= 4, "{calls}");
    assert_eq!(*store.full.lock().unwrap(), 0);
    assert_eq!(r.read(&entries[0]).await.unwrap(), expected[0]);
    assert_eq!(store.calls.lock().unwrap().len(), calls);
    // Scattered reads stop issuing ranges after the per-part request budget.
    for (entry, want) in entries.iter().zip(&expected) {
        assert_eq!(r.read(entry).await.unwrap(), *want);
    }
    assert!(store.calls.lock().unwrap().len() <= 8);
    assert_eq!(*store.full.lock().unwrap(), 1);
}
#[tokio::test]
async fn full_reads_and_old_stores_keep_full_object_behavior() {
    for mode in [0, 1, 2] {
        let (id, mut store, entries, expected) = fixture();
        store.mode = mode;
        let r = Reader::new(store.clone(), &id, &[]).unwrap();
        if mode == 0 {
            assert_eq!(r.read_full(&entries[0]).await.unwrap(), expected[0]);
        } else {
            assert_eq!(r.read(&entries[0]).await.unwrap(), expected[0]);
        }
        assert_eq!(r.read_full(&entries[1]).await.unwrap(), expected[1]);
        assert_eq!(*store.full.lock().unwrap(), if mode == 2 { 0 } else { 1 });
        if mode == 0 {
            assert!(store.calls.lock().unwrap().is_empty());
        }
    }
}
#[tokio::test]
async fn bad_ranges_corruption_and_bad_file_hash_fail() {
    let (id, mut store, entries, _) = fixture();
    store.mode = 3;
    assert!(Reader::new(store, &id, &[])
        .unwrap()
        .read(&entries[0])
        .await
        .is_err());
    let (id, mut store, entries, _) = fixture();
    let mut bad = store.bytes.as_ref().clone();
    // A byte in the encrypted chunk containing the first requested frame.
    bad[2 * 1024 * 1024 + 1000] ^= 1;
    store.bytes = Arc::new(bad);
    assert!(Reader::new(store, &id, &[])
        .unwrap()
        .read(&entries[0])
        .await
        .is_err());
    let (id, store, mut entries, _) = fixture();
    entries[0].2 = "00".repeat(32);
    assert!(Reader::new(store, &id, &[])
        .unwrap()
        .read(&entries[0])
        .await
        .is_err());
}
#[test]
fn content_ranges_have_strict_bounds() {
    assert_eq!(ContentRange::parse("bytes 4-8/10").unwrap().len(), 5);
    for value in [
        "bytes 8-4/10",
        "bytes 0-10/10",
        "bytes 0-1/*",
        "bytes */10",
        "bytes 0-0/0",
    ] {
        assert!(ContentRange::parse(value).is_err(), "{value}");
    }
    assert!(RangeData {
        start: 0,
        total: 100,
        bytes: vec![0; 10]
    }
    .validate(1, 10)
    .is_err());
    assert!(RangeData {
        start: 90,
        total: 100,
        bytes: vec![0; 10]
    }
    .validate(90, 120)
    .is_ok());
}

#[tokio::test]
async fn frames_cross_age_chunks_and_end_at_object_boundary() {
    let id = keys::generate_identity();
    let recipient = keys::recipient_of(&id).unwrap();
    let data = random(90_000);
    let frame = zst::compress(&data, None).unwrap();
    for offset in [0, 65_520, 131_040, 300_000] {
        let mut plain = random(offset);
        plain.extend_from_slice(&frame);
        let store = Memory {
            bytes: Arc::new(keys::encrypt(&recipient, &plain).unwrap()),
            calls: Arc::default(),
            full: Arc::default(),
            mode: 0,
        };
        let entry = Entry(
            "100644".into(),
            data.len() as u64,
            hex::encode(Sha256::digest(&data)),
            vec![Link(
                1,
                "aaaaaaaa".into(),
                0,
                offset as u64,
                frame.len() as u64,
                "full".into(),
            )],
        );
        let reader = Reader::new(store.clone(), &id, &[]).unwrap();
        assert_eq!(reader.read(&entry).await.unwrap(), data, "offset {offset}");
        assert_eq!(*store.full.lock().unwrap(), 0);
    }
}
