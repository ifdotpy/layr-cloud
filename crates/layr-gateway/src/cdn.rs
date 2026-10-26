//! Only create-once ciphertext may leave the direct gateway route.
use anyhow::{bail, Context, Result};
use base64::Engine;
use rsa::{
    pkcs8::DecodePrivateKey,
    signature::{SignatureEncoding, Signer as _},
    traits::PublicKeyParts,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTENT_TYPE: &str = "application/vnd.layr.cdn+json";
pub const CAPABILITY: &str = "x-layr-cdn";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub host: String,
    pub key_id: String,
    pub key_parameter: String,
    pub region: String,
    pub stores: BTreeMap<String, Store>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Store {
    pub bucket: String,
    pub prefix: String,
    pub region: String,
}

pub struct Signer {
    settings: Settings,
    key: rsa::pkcs1v15::SigningKey<sha2::Sha256>,
}
impl Settings {
    pub fn validate(&self, public_host: &str) -> Result<()> {
        if self.host != public_host
            || self.host.is_empty()
            || self
                .host
                .bytes()
                .any(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'.' | b'-'))
            || self.key_id.is_empty()
            || !self.key_id.bytes().all(|b| b.is_ascii_alphanumeric())
            || !matches!(
                self.key_parameter.as_str(),
                "/layr-store/gateway/cdn-signing-a" | "/layr-store/gateway/cdn-signing-b"
            )
            || self.region != "eu-north-1"
            || self.stores.is_empty()
        {
            bail!("invalid CDN signing configuration");
        }
        let mut prefixes = std::collections::BTreeSet::new();
        for (alias, store) in &self.stores {
            if !crate::config::project_name(alias)
                || !crate::config::project_name(&store.prefix)
                || store.bucket.is_empty()
                || store.region != self.region
                || !prefixes.insert(&store.prefix)
            {
                bail!("invalid CDN store binding");
            }
        }
        Ok(())
    }
}
impl Signer {
    pub fn new(settings: Settings, pem: &str, public_host: &str) -> Result<Self> {
        settings.validate(public_host)?;
        let key = rsa::RsaPrivateKey::from_pkcs8_pem(pem).context("invalid CDN signing key")?;
        if key.n().bits() != 2048 {
            bail!("CDN signing key must be RSA 2048");
        }
        Ok(Self {
            settings,
            key: rsa::pkcs1v15::SigningKey::new(key),
        })
    }
    fn object(&self, project: &crate::config::Project, alias: &str, key: &str) -> Option<String> {
        let store = self.settings.stores.get(alias)?;
        if project.bucket != store.bucket || project.region != store.region {
            return None;
        }
        let relative = if project.prefix == store.prefix {
            key.to_owned()
        } else {
            let epoch_prefix = project.prefix.strip_prefix(&format!("{}/", store.prefix))?;
            format!("{epoch_prefix}/{key}")
        };
        cacheable(&relative).then_some(relative)
    }
    pub fn reply(
        &self,
        gateway: &crate::server::Gateway,
        project: &crate::config::Project,
        alias: &str,
        key: &str,
        request: &layr_store::http::Request,
    ) -> Result<Option<crate::server::Reply>> {
        if request.header(CAPABILITY) != Some("1") || request.header("range").is_some() {
            return Ok(None);
        }
        let Some(relative) = self.object(project, alias, key) else {
            return Ok(None);
        };
        let Some(bytes) = gateway.storage.object_length(project, key)? else {
            return Ok(Some(crate::server::Reply::error(404, "no such object")));
        };
        let expires = crate::auth::now() / 1000 + 300;
        let url = self.url(alias, &relative, expires);
        let mut reply = crate::server::Reply::bytes(
            200,
            serde_json::to_vec(&serde_json::json!({"url":url,"expires":expires,"bytes":bytes}))?,
            CONTENT_TYPE,
        );
        reply
            .headers
            .push(("Cache-Control".into(), "no-store".into()));
        Ok(Some(reply))
    }
    fn url(&self, alias: &str, key: &str, expires: u64) -> String {
        let url = format!("https://{}/o/{alias}/{key}", self.settings.host);
        #[derive(Serialize)]
        struct Statement<'a> {
            #[serde(rename = "Resource")]
            resource: &'a str,
            #[serde(rename = "Condition")]
            condition: serde_json::Value,
        }
        let statement = Statement {
            resource: &url,
            condition: serde_json::json!({"DateLessThan":{"AWS:EpochTime":expires}}),
        };
        let policy = format!(
            "{{\"Statement\":[{}]}}",
            serde_json::to_string(&statement).expect("serializable canned policy")
        );
        let signature = self.key.sign(policy.as_bytes());
        let signature = base64::engine::general_purpose::STANDARD
            .encode(signature.to_bytes())
            .replace('+', "-")
            .replace('=', "_")
            .replace('/', "~");
        format!(
            "{url}?Expires={expires}&Signature={signature}&Key-Pair-Id={}&Hash-Algorithm=SHA256",
            self.settings.key_id
        )
    }
}

#[cfg(test)]
pub fn test_signer(project: &crate::config::Project) -> Signer {
    use rsa::pkcs8::EncodePrivateKey;
    static KEY: std::sync::OnceLock<rsa::RsaPrivateKey> = std::sync::OnceLock::new();
    let key = KEY.get_or_init(|| rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap());
    let settings = Settings {
        host: "localhost".into(),
        key_id: "PUBLICKEY".into(),
        key_parameter: "/layr-store/gateway/cdn-signing-a".into(),
        region: "eu-north-1".into(),
        stores: [(
            "example".into(),
            Store {
                bucket: project.bucket.clone(),
                prefix: project.prefix.clone(),
                region: project.region.clone(),
            },
        )]
        .into(),
    };
    Signer::new(
        settings,
        key.to_pkcs8_pem(Default::default()).unwrap().as_str(),
        "localhost",
    )
    .unwrap()
}

pub fn cacheable(key: &str) -> bool {
    if key.len() > 1024
        || key
            .split('/')
            .any(|s| s.is_empty() || matches!(s, "." | ".."))
    {
        return false;
    }
    let (epoch, inner) = match crate::epochs::split(key) {
        Some((_, inner)) => (true, inner),
        None => (false, key),
    };
    use layr_store::format;
    crate::server::object_name(inner)
        && (format::parse_tree_name(inner).is_some()
            || format::parse_data_name(inner).is_some()
            || format::parse_segment_name(inner).is_some()
            || inner.starts_with("index/paths/")
            || inner.starts_with("index/graph/")
            || layr_store::format::release_file(inner)
            || (epoch && inner.starts_with("history/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "01a177f1-4bd0-7029-b488-6f00793f9798";
    #[test]
    fn canned_signature_matches_rsa_sha256_and_scoped_store_mapping() {
        use rsa::signature::{Keypair, Verifier};
        let config = crate::config::sample();
        let project = &config.projects["example"];
        let signer = test_signer(project);
        let key = format!("files/{ID}.age");
        let url = signer.url("example", &key, 2000);
        let parsed = reqwest::Url::parse(&url).unwrap();
        let query: BTreeMap<_, _> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(query["Hash-Algorithm"], "SHA256");
        let signature = base64::engine::general_purpose::STANDARD
            .decode(
                query["Signature"]
                    .replace('-', "+")
                    .replace('_', "=")
                    .replace('~', "/"),
            )
            .unwrap();
        let signature = rsa::pkcs1v15::Signature::try_from(signature.as_slice()).unwrap();
        let resource = format!("https://localhost/o/example/{key}");
        let policy = [
            "{\"Statement\":[{\"Resource\":\"",
            &resource,
            "\",\"Condition\":{\"DateLessThan\":{\"AWS:EpochTime\":2000}}}]}",
        ]
        .concat();
        signer
            .key
            .verifying_key()
            .verify(policy.as_bytes(), &signature)
            .unwrap();
        assert!(signer
            .key
            .verifying_key()
            .verify(policy.replace("files/", "states/").as_bytes(), &signature)
            .is_err());
        assert_eq!(signer.object(project, "example", &key), Some(key.clone()));
        let mut scoped = project.clone();
        scoped.prefix += &format!("/project/epochs/{ID}");
        assert_eq!(
            signer.object(&scoped, "example", &key),
            Some(format!("project/epochs/{ID}/{key}"))
        );
        scoped.bucket = "another-bucket".into();
        assert!(signer.object(&scoped, "example", &key).is_none());
        let mut settings = signer.settings.clone();
        settings.key_parameter = "/layr-store/gateway/session-secret".into();
        assert!(settings.validate("localhost").is_err());
        assert!(signer.settings.validate("other-host").is_err());
    }
    #[test]
    fn allowlist_accepts_only_valid_immutable_object_classes() {
        for key in [
            format!("states/{ID}/12345678.tree.age"),
            format!("states/{ID}/12345678.0000.data.age"),
            layr_store::format::segment_name(ID, 1, 2),
            format!("index/paths/{ID}.age"),
            format!("index/graph/{ID}.age"),
            format!("files/{ID}.age"),
        ] {
            assert!(cacheable(&key), "{key}");
            assert!(cacheable(&format!("project/epochs/{ID}/{key}")));
        }
        assert!(cacheable(&format!("project/epochs/{ID}/history/root.age")));
        for key in [
            "store.json".into(),
            "public.json".into(),
            "keys/epoch.json".into(),
            format!("slots/machine-{ID}.age"),
            format!("index/heads/{ID}/1.age"),
            format!("requests/{ID}.json"),
            format!("claims/{ID}.json"),
            "history/root.age".into(),
            "states/../key.age".into(),
            format!("project/epochs/invalid/states/{ID}/12345678.tree.age"),
            format!("project/epochs/{ID}/project/epochs/{ID}/files/{ID}.age"),
            format!("overlays/{ID}/keys/{ID}.json"),
            "records/a.age".into(),
        ] {
            assert!(!cacheable(&key), "{key}");
        }
    }
}
