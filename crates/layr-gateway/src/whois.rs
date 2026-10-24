//! Tailnet logins come from tailscaled, never from request headers.
use anyhow::{bail, Result};
use serde_json::Value;
use std::{io::Read, net::SocketAddr, path::Path, time::Duration};
pub const SOCKET: &str = "/var/run/tailscale/tailscaled.sock";
pub fn login(socket: &Path, address: SocketAddr) -> Result<Option<String>> {
    let client = reqwest::blocking::Client::builder()
        .unix_socket(socket)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()?;
    let response = client
        .get("http://local-tailscaled.sock/localapi/v0/whois")
        .query(&[("addr", address.to_string())])
        .send()
        .map_err(reqwest::Error::without_url)?;
    match response.status().as_u16() {
        404 => Ok(None),
        200 => {
            let mut body = Vec::new();
            response.take((1 << 20) + 1).read_to_end(&mut body)?;
            if body.len() > 1 << 20 {
                bail!("tailscaled response is too large");
            }
            Ok(login_of(&serde_json::from_slice(&body)?))
        }
        status => bail!("tailscaled whois: HTTP {status}"),
    }
}
pub fn login_of(reply: &Value) -> Option<String> {
    let tagged = reply["Node"]["Tags"]
        .as_array()
        .is_some_and(|tags| !tags.is_empty());
    let login = reply["UserProfile"]["LoginName"].as_str()?;
    (!tagged && layr_store::person::login_name(login)).then(|| login.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tagged_unknown_and_invalid_logins_have_no_person() {
        assert_eq!(
            login_of(&serde_json::json!({"UserProfile":{"LoginName":"ann@example.com"}})),
            Some("ann@example.com".into())
        );
        for value in [
            serde_json::json!({}),
            serde_json::json!({"UserProfile":{"LoginName":"ann@example.com"},"Node":{"Tags":["tag:server"]}}),
            serde_json::json!({"UserProfile":{"LoginName":"../ann"}}),
        ] {
            assert_eq!(login_of(&value), None);
        }
    }
    #[test]
    fn localapi_names_the_peer_and_ignores_tagged_devices() {
        use std::io::{BufRead, BufReader};
        let root =
            std::path::PathBuf::from("/tmp").join(format!("lw-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let socket = root.join("tailscaled.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let serving = std::thread::spawn(move || {
            for n in 1..=3 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(
                    line.starts_with(&format!(
                        "GET /localapi/v0/whois?addr=100.64.0.{n}%3A50000 "
                    )),
                    "{line}"
                );
                let mut host = false;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if line
                        .trim()
                        .eq_ignore_ascii_case("host: local-tailscaled.sock")
                    {
                        host = true;
                    }
                }
                assert!(host);
                let body = serde_json::json!({"UserProfile":{"LoginName":"ann@example.com"},"Node":{"Tags":if n == 2 {vec!["tag:server"]} else {vec![]}}});
                layr_store::http::response(
                    &mut stream,
                    if n == 3 { 404 } else { 200 },
                    &[("Content-Type".into(), "application/json".into())],
                    body.to_string().as_bytes(),
                )
                .unwrap();
            }
        });
        assert_eq!(
            login(&socket, "100.64.0.1:50000".parse().unwrap()).unwrap(),
            Some("ann@example.com".into())
        );
        assert_eq!(
            login(&socket, "100.64.0.2:50000".parse().unwrap()).unwrap(),
            None
        );
        assert_eq!(
            login(&socket, "100.64.0.3:50000".parse().unwrap()).unwrap(),
            None
        );
        serving.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
