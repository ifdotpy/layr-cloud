//! The layr-desk vault (docs/design.md): keys and slots, the object format, a reader that
//! decrypts states from any store of objects, and the writer that pushes them.

pub mod aws;
pub mod canonical;
pub mod format;
pub mod keys;
pub mod range;
#[cfg(feature = "zstd")]
pub mod reader;
#[cfg(all(unix, feature = "zstd"))]
pub mod writer;
#[cfg(feature = "zstd")]
pub mod zst;

pub use format::{Entry, Header, Link};
#[cfg(feature = "zstd")]
pub use reader::{Reader, Store};
