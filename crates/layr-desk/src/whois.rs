//! Who is calling the gateway (docs/design.md, "Person key"): the tailnet login of a peer
//! address, from the local tailscaled (`GET /localapi/v0/whois`). The gateway listens on its
//! tailnet address itself, so a `Tailscale-User-Login` header would come from the caller; only
//! tailscaled knows who owns the address.

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Limited};
use hyper::Request;
use hyper_util::rt::TokioIo;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::Value;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

/// The default socket of tailscaled on Linux.
pub const SOCKET: &str = "/var/run/tailscale/tailscaled.sock";
/// The Host header that tailscaled wants on its socket.
const HOST: &str = "local-tailscaled.sock";
const LIMIT: Duration = Duration::from_secs(3);

/// The login of the person whose device has `address`; none for a tagged device or an address
/// that tailscaled does not know.
pub async fn login(socket: &Path, address: SocketAddr) -> Result<Option<String>> {
    tokio::time::timeout(LIMIT, ask(socket, address))
        .await
        .context("tailscaled does not answer")?
}

async fn ask(socket: &Path, address: SocketAddr) -> Result<Option<String>> {
    let stream = tokio::net::UnixStream::connect(socket)
        .await
        .with_context(|| format!("connecting to {}", socket.display()))?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(connection);
    let target = utf8_percent_encode(&address.to_string(), NON_ALPHANUMERIC).to_string();
    let request = Request::get(format!("/localapi/v0/whois?addr={target}"))
        .header("host", HOST)
        .body(Empty::<Bytes>::new())?;
    let response = sender.send_request(request).await?;
    let status = response.status().as_u16();
    let body = Limited::new(response.into_body(), 1 << 20)
        .collect()
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .to_bytes();
    match status {
        200 => Ok(login_of(&serde_json::from_slice(&body)?)),
        // No device has this address.
        404 => Ok(None),
        _ => bail!(
            "tailscaled whois: HTTP {status} {}",
            String::from_utf8_lossy(&body).trim()
        ),
    }
}

/// The login in a whois reply: the user of an untagged device. A tagged device acts for its tags,
/// not for the person who added it.
pub fn login_of(reply: &Value) -> Option<String> {
    let tagged = reply["Node"]["Tags"]
        .as_array()
        .is_some_and(|tags| !tags.is_empty());
    let login = reply["UserProfile"]["LoginName"].as_str()?;
    (!tagged && desk_core::person::login_name(login)).then(|| login.to_string())
}
