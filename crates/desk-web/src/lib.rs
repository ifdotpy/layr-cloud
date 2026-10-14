//! Browser bindings to the same authenticated age reader used by native clients.
use wasm_bindgen::prelude::*;

fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[wasm_bindgen]
pub struct BlockReader(desk_core::range::FrameDecoder);

#[wasm_bindgen]
impl BlockReader {
    #[wasm_bindgen(constructor)]
    pub fn new(identity: &str) -> Result<BlockReader, JsValue> {
        desk_core::range::FrameDecoder::new(identity)
            .map(Self)
            .map_err(error)
    }
    pub fn insert(&mut self, start: u64, total: u64, bytes: Vec<u8>) -> Result<(), JsValue> {
        self.0.insert(start, total, bytes).map_err(error)
    }
    pub fn frame(&mut self, offset: u64, length: u32) -> Result<Option<Vec<u8>>, JsValue> {
        self.0.frame(offset, length as usize).map_err(error)
    }
    pub fn needed(&self) -> Option<u64> {
        self.0.needed()
    }
}

#[wasm_bindgen]
pub fn decrypt_object(identity: &str, bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    desk_core::keys::decrypt(identity, bytes).map_err(error)
}
