//! Authenticate the edge before trusting its viewer address.
use crate::server::Reply;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
};

pub const HEADER: &str = "x-layr-origin";

pub struct Origin {
    secret: String,
}

impl Origin {
    pub fn new(secret: String) -> anyhow::Result<Self> {
        anyhow::ensure!(
            secret.len() == 64 && secret.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid origin secret"
        );
        Ok(Self { secret })
    }

    pub fn client(&self, headers: &BTreeMap<String, String>) -> Result<IpAddr, Reply> {
        let given = headers.get(HEADER).map(String::as_str).unwrap_or("");
        // Compare keyed digests with the MAC library's constant-time verification.
        let tag = |key: &[u8]| {
            let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC key");
            mac.update(b"layr-cloud-origin");
            mac
        };
        if given.len() != 64
            || tag(self.secret.as_bytes())
                .verify_slice(&tag(given.as_bytes()).finalize().into_bytes())
                .is_err()
        {
            return Err(Reply::error(403, "origin access denied"));
        }
        let address = headers
            .get("cloudfront-viewer-address")
            .and_then(|value| viewer_address(value))
            .filter(|address| address.port() != 0)
            .ok_or_else(|| Reply::error(400, "invalid viewer address"))?;
        Ok(address.ip())
    }
}

// CloudFront emits address:port, including unbracketed IPv6. Accept bracketed
// IPv6 too; the final component is always the source port, never part of the IP.
fn viewer_address(value: &str) -> Option<SocketAddr> {
    if value.starts_with('[') {
        return value.parse().ok();
    }
    let (ip, port) = value.rsplit_once(':')?;
    Some(SocketAddr::new(ip.parse().ok()?, port.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_secret_before_accepting_viewer_address() {
        let origin = Origin::new("ab".repeat(32)).unwrap();
        let mut headers = BTreeMap::from([(
            "cloudfront-viewer-address".into(),
            "198.51.100.4:1234".into(),
        )]);
        assert_eq!(origin.client(&headers).unwrap_err().status, 403);
        headers.insert(HEADER.into(), "ac".repeat(32));
        assert_eq!(origin.client(&headers).unwrap_err().status, 403);
        headers.insert(HEADER.into(), "ab".repeat(32));
        assert_eq!(
            origin.client(&headers).ok().unwrap(),
            "198.51.100.4".parse::<IpAddr>().unwrap()
        );
        headers.insert(
            "cloudfront-viewer-address".into(),
            "[2001:db8::4]:4321".into(),
        );
        assert_eq!(
            origin.client(&headers).ok().unwrap(),
            "2001:db8::4".parse::<IpAddr>().unwrap()
        );
        headers.insert(
            "cloudfront-viewer-address".into(),
            "2001:db8::4:4321".into(),
        );
        assert_eq!(
            origin.client(&headers).ok().unwrap(),
            "2001:db8::4".parse::<IpAddr>().unwrap()
        );
        for invalid in [
            "",
            "198.51.100.4",
            "198.51.100.4:0",
            "198.51.100.4:123, 198.51.100.5:123",
            "2001:db8::4",
        ] {
            headers.insert("cloudfront-viewer-address".into(), invalid.into());
            assert_eq!(origin.client(&headers).unwrap_err().status, 400);
        }
    }
}
