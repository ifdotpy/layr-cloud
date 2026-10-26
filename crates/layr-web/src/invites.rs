//! Browser acceptance uses the CLI's shared pin, slot and encrypted-index formats.
use super::*;
use sha2::{Digest, Sha256};

#[wasm_bindgen]
impl Session {
    pub fn invite_project(&mut self, view: &str, id: &str) -> std::result::Result<String, JsValue> {
        self.read_invite(view, id)
            .map(|(_, pin)| pin.project)
            .map_err(js)
    }

    /// Prepare ciphertext only. The new pin becomes active after the gateway readback.
    pub fn prepare_invite_accept(
        &mut self,
        view: &str,
        id: &str,
        descriptor: &str,
        slot: &[u8],
    ) -> std::result::Result<String, JsValue> {
        self.accept_invite(view, id, descriptor, slot).map_err(js)
    }
}

impl Session {
    fn read_invite(&mut self, view: &str, id: &str) -> Result<(Value, person::Pin)> {
        let view: Value = serde_json::from_str(view)?;
        self.refresh_person_view(&view)?;
        let invite = view["inbox"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"] == id))
            .context("This invite is no longer available. Ask for a new invite.")?;
        let pin = person::open_invite(
            self.person.as_ref().context("Your session is locked.")?,
            &unb64(invite["data"].as_str().context("invite has no data")?)?,
        )?;
        Ok((view, pin))
    }

    pub(super) fn accept_invite(
        &mut self,
        view: &str,
        id: &str,
        descriptor: &str,
        slot: &[u8],
    ) -> Result<String> {
        let (view, pin) = self.read_invite(view, id)?;
        let descriptor: format::Descriptor = serde_json::from_str(descriptor)?;
        if descriptor.format != 2 {
            bail!("invalid store descriptor");
        }
        pin.check_store(&descriptor)?;
        let identity = self.person.as_ref().context("Your session is locked.")?;
        let opened = Zeroizing::new(keys::open_slot(slot, identity)?);
        if keys::recipient_of(&opened)? != pin.recipient {
            bail!("person slot does not match store recipient");
        }
        let mut index = self.index.clone();
        index.set(pin);
        let previous = view["index"]
            .as_str()
            .map(|data| unb64(data).map(|bytes| hex::encode(Sha256::digest(bytes))))
            .transpose()?;
        encoded(
            json!({"data":B64.encode(person::seal_index(identity, &index)?),"previous":previous}),
        )
    }
}
