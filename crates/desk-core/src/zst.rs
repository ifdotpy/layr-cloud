//! zstd frames of the vault. A delta frame takes the previous content as a raw dictionary, as
//! `zstd --patch-from` and the writer make it.

use anyhow::{anyhow, bail, Result};
use zstd_safe::{DCtx, InBuffer, OutBuffer};

/// The largest content one frame may decode to; more is refused as a damaged or hostile frame.
pub const LIMIT: usize = 4 << 30;

fn error(code: usize) -> anyhow::Error {
    anyhow!("zstd: {}", zstd_safe::get_error_name(code))
}

/// One frame decoded; `base` is the raw dictionary of a delta frame.
pub fn decompress(frame: &[u8], base: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut dctx = DCtx::create();
    if let Some(base) = base.filter(|b| !b.is_empty()) {
        // A prefix is a raw-content dictionary for one frame, whatever its first bytes are.
        dctx.ref_prefix(base).map_err(error)?;
    }
    let mut out = Vec::new();
    let mut input = InBuffer::around(frame);
    let mut chunk = vec![0u8; DCtx::out_size()];
    loop {
        let mut output = OutBuffer::around(&mut chunk[..]);
        let hint = dctx
            .decompress_stream(&mut output, &mut input)
            .map_err(error)?;
        let written = output.pos();
        out.extend_from_slice(&chunk[..written]);
        if out.len() > LIMIT {
            bail!("a zstd frame decodes to more than {LIMIT} bytes");
        }
        if hint == 0 {
            if input.pos() != frame.len() {
                bail!("bytes after the zstd frame");
            }
            return Ok(out);
        }
        if input.pos() == frame.len() && written == 0 {
            bail!("the zstd frame is cut short");
        }
    }
}
