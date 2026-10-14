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
