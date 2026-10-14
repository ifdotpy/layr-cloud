//! Sparse ciphertext for age's authenticated seek reader. No encryption format changes.
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};

pub const BLOCK: u64 = 128 * 1024;

/// Inclusive HTTP byte range, with a known complete object length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: u64,
}

impl ContentRange {
    pub fn parse(text: &str) -> Result<Self> {
        let Some((span, total)) = text.strip_prefix("bytes ").and_then(|s| s.split_once('/'))
        else {
            bail!("invalid Content-Range");
        };
        let Some((start, end)) = span.split_once('-') else {
            bail!("invalid Content-Range");
        };
        let range = Self {
            start: start.parse()?,
            end: end.parse()?,
            total: total.parse()?,
        };
        if range.start > range.end || range.end >= range.total {
            bail!("invalid Content-Range bounds");
        }
        Ok(range)
    }
    pub fn header(self) -> String {
        format!("bytes {}-{}/{}", self.start, self.end, self.total)
    }
    pub fn len(self) -> u64 {
        self.end - self.start + 1
    }
}

/// A response to a bounded range request. A server may return the complete object instead.
#[derive(Debug)]
pub struct RangeData {
    pub start: u64,
    pub total: u64,
    pub bytes: Vec<u8>,
}

impl RangeData {
    pub fn validate(&self, start: u64, end: u64) -> Result<()> {
        if self.total == 0 || self.bytes.is_empty() {
            bail!("empty range response");
        }
        if self.start == 0 && self.bytes.len() as u64 == self.total {
            return Ok(());
        }
        if start >= self.total
            || self.start != start
            || end < start
            || self.bytes.len() as u64 != end.min(self.total - 1) - start + 1
        {
            bail!("range response does not match request");
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub(crate) struct Partial {
    pub total: Option<u64>,
    pub blocks: BTreeMap<u64, Arc<Vec<u8>>>,
    pub requests: usize,
}

pub(crate) struct Sparse {
    pub partial: Partial,
    pub missing: Arc<Mutex<Option<u64>>>,
    pub pos: u64,
}

impl Read for Sparse {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.partial.total.is_some_and(|n| self.pos >= n) {
            return Ok(0);
        }
        let start = self.pos / BLOCK * BLOCK;
        let Some(bytes) = self.partial.blocks.get(&start) else {
            *self.missing.lock().unwrap() = Some(start);
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "ciphertext block not fetched",
            ));
        };
        let at = (self.pos - start) as usize;
        if at >= bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short ciphertext block",
            ));
        }
        let n = out.len().min(bytes.len() - at);
        out[..n].copy_from_slice(&bytes[at..at + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Sparse {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let pos = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.pos) + i128::from(n),
            SeekFrom::End(n) => {
                let Some(total) = self.partial.total else {
                    *self.missing.lock().unwrap() = Some(0);
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "object length not fetched",
                    ));
                };
                i128::from(total) + i128::from(n)
            }
        };
        self.pos = u64::try_from(pos)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"))?;
        Ok(self.pos)
    }
}

/// Decode an authenticated age frame, or report the next ciphertext block required.
pub(crate) fn decode_frame(
    identity: &age::x25519::Identity,
    partial: Partial,
    offset: u64,
    length: usize,
) -> (Result<Vec<u8>>, Option<u64>) {
    use std::io::BufReader;
    let missing = Arc::new(Mutex::new(None));
    let source = Sparse {
        partial,
        missing: missing.clone(),
        pos: 0,
    };
    let result = (|| -> Result<Vec<u8>> {
        let mut reader = age::Decryptor::new_buffered(BufReader::new(source))?
            .decrypt(std::iter::once(identity as &dyn age::Identity))?;
        reader.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes)?;
        Ok(bytes)
    })();
    let needed = *missing.lock().unwrap();
    (result, needed)
}

/// Bounded ciphertext cache for a browser that fetches missing blocks asynchronously.
pub struct FrameDecoder {
    identity: age::x25519::Identity,
    partial: Partial,
    needed: Option<u64>,
}
impl FrameDecoder {
    pub fn new(identity: &str) -> Result<Self> {
        Ok(Self {
            identity: identity
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid identity: {e}"))?,
            partial: Partial::default(),
            needed: None,
        })
    }
    pub fn insert(&mut self, start: u64, total: u64, bytes: Vec<u8>) -> Result<()> {
        if start % BLOCK != 0 || self.partial.blocks.len() >= 8 {
            bail!("ciphertext block limit or alignment");
        }
        let data = RangeData {
            start,
            total,
            bytes,
        };
        data.validate(
            start,
            start
                .checked_add(BLOCK - 1)
                .ok_or_else(|| anyhow::anyhow!("range overflow"))?,
        )?;
        if data.bytes.len() as u64 > BLOCK || self.partial.total.is_some_and(|n| n != total) {
            bail!("incorrect ciphertext block or object length changed");
        }
        self.partial.total = Some(total);
        self.partial.blocks.insert(start, Arc::new(data.bytes));
        Ok(())
    }
    pub fn frame(&mut self, offset: u64, length: usize) -> Result<Option<Vec<u8>>> {
        if length >= 512 * 1024 || offset.checked_add(length as u64).is_none() {
            bail!("frame exceeds selective read limit");
        }
        let (result, needed) = decode_frame(&self.identity, self.partial.clone(), offset, length);
        self.needed = needed;
        if needed.is_some() {
            Ok(None)
        } else {
            result.map(Some)
        }
    }
    pub fn needed(&self) -> Option<u64> {
        self.needed
    }
}
