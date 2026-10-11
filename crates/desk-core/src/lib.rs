//! The layr-desk vault (docs/design.md): keys and slots, the object format, and a reader that
//! decrypts states from any store of objects.

pub mod format;
pub mod keys;
pub mod reader;
pub mod zst;

pub use format::{Entry, Header, Link};
pub use reader::{Reader, Store};
