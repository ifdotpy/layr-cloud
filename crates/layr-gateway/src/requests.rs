//! Create-only signed person requests and executor claims.
use crate::{
    auth,
    config::Project,
    server::{Gateway, Reply, JSON_LIMIT},
};
use anyhow::Result;
use base64::Engine;
use layr_store::{
    format::Descriptor,
    http,
    request::{Claim, Signed},
};

pub fn path(name: &str) -> Option<(bool, &str)> {
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\Arequests/(claims/)?([0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})\.json\z").unwrap()
    });
    let matched = PATTERN.captures(name)?;
    Some((matched.get(1).is_some(), matched.get(2)?.as_str()))
}

impl Gateway {
    pub fn put_request(
        &self,
        request: &http::Request,
        project: &Project,
        key: &str,
        claim: bool,
        id: &str,
        login: Option<&str>,
        write: bool,
    ) -> Result<Reply> {
        if request.body.len() > layr_store::request::MAX_BYTES {
            return Ok(Reply::error(413, "request is too large"));
        }
        if !matches!(
            request.header("content-type"),
            Some("application/json" | "application/octet-stream")
        ) {
            return Ok(Reply::error(415, "send JSON or application/octet-stream"));
        }
        if request.header("content-length").is_none() {
            return Ok(Reply::error(411, "Content-Length is required"));
        }
        let signed = match Signed::parse(&request.body) {
            Ok(signed) => signed,
            Err(_) => return Ok(Reply::error(400, "invalid signed request envelope")),
        };
        let Some(descriptor) = self.storage.get(project, "store.json", None, JSON_LIMIT)? else {
            return Ok(Reply::error(404, "store is not initialized"));
        };
        let descriptor: Descriptor = serde_json::from_slice(&descriptor.body)?;
        if descriptor.format != 2 || descriptor.recipient != project.recipient {
            return Ok(Reply::error(
                502,
                "store descriptor does not match gateway project",
            ));
        }
        if claim {
            if !write {
                return Ok(Reply::error(403, "claims require an executor write token"));
            }
            let claim: Claim = match serde_json::from_str(signed.r.get()) {
                Ok(claim) => claim,
                Err(_) => return Ok(Reply::error(400, "invalid claim structure")),
            };
            if claim.validate().is_err()
                || claim.project != descriptor.project
                || claim.request != id
                || !base64::engine::general_purpose::STANDARD
                    .decode(&signed.s)
                    .is_ok_and(|s| s.len() == 64)
            {
                return Ok(Reply::error(400, "claim does not match project or path"));
            }
            // The writer bearer is the gateway authority. Canonical machine trust and the
            // signature are verified by the executor from its decrypted store, not a second registry.
        } else {
            let Some(login) = login.filter(|login| layr_store::person::login_name(login)) else {
                return Ok(Reply::error(
                    403,
                    "person requests require a tailnet person",
                ));
            };
            let config = self.config.get()?;
            let Some(people_config) = &config.people else {
                return Ok(Reply::error(404, "this gateway keeps no person keys"));
            };
            let people = crate::people::People::new(
                self.storage.people(people_config)?,
                &people_config.prefix,
                self.person_secret.clone(),
                std::sync::Arc::new(auth::now),
                self.person_writing.clone(),
            );
            let Some(public) = people.record(login)?.and_then(|record| record.signing_key) else {
                return Ok(Reply::error(403, "register a person signing key first"));
            };
            let value = match layr_store::person::verifying_key(&public)
                .and_then(|key| signed.verify(&key))
            {
                Ok(value) => value,
                Err(_) => {
                    return Ok(Reply::error(
                        403,
                        "person request signature does not verify",
                    ))
                }
            };
            if value.person != login || value.project != descriptor.project || value.id != id {
                return Ok(Reply::error(
                    403,
                    "person request does not match caller, project or path",
                ));
            }
        }
        Ok(if self.storage.put(project, key, request.body.clone())? {
            Reply::bytes(201, Vec::new(), "application/json")
        } else {
            Reply::error(412, "object already exists")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_anchored_canonical_uuidv7_names() {
        let id = "01a12290-0000-7000-8000-000000000001";
        assert_eq!(path(&format!("requests/{id}.json")), Some((false, id)));
        assert_eq!(
            path(&format!("requests/claims/{id}.json")),
            Some((true, id))
        );
        for value in [
            format!("xrequests/{id}.json"),
            format!("requests/{id}.json/extra"),
            format!("requests/{id}.json\n"),
            format!("requests/../{id}.json"),
            format!("requests//{id}.json"),
            format!("requests/{id}.age"),
            format!("requests/{id}.json").replace("7000", "4000"),
            format!("requests/{id}.json").to_uppercase(),
        ] {
            assert!(path(&value).is_none(), "{value}");
        }
    }
}
