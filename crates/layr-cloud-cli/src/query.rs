//! Signed AWS Query requests. This module can move to layr-store.
use anyhow::{bail, Context, Result};
use layr_store::aws::{self, Credentials};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const RESPONSE_LIMIT: u64 = 4 * 1024 * 1024;

fn endpoint(variable: &str, default: String) -> String {
    std::env::var(variable)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .trim_end_matches('/')
        .into()
}

fn clean(credentials: &Credentials, value: &str) -> String {
    let mut text = value.to_string();
    for secret in [
        Some(&credentials.key_id),
        Some(&credentials.secret),
        credentials.token.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if !secret.is_empty() {
            text = text.replace(secret, "[redacted]");
        }
    }
    text
}

pub fn send(
    http: &Client,
    credentials: &Credentials,
    region: &str,
    service: &str,
    version: &str,
    params: &[(&str, &str)],
    limit: Duration,
) -> Result<(u16, String)> {
    let base = endpoint(
        &format!("AWS_ENDPOINT_URL_{}", service.to_uppercase()),
        format!("https://{service}.{}.amazonaws.com", region),
    );
    let url = reqwest::Url::parse(&base).context("invalid AWS endpoint")?;
    if url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("AWS endpoint must contain only the scheme, host, and port");
    }
    let host = match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.into(),
        _ => bail!("AWS endpoint has no host"),
    };
    let mut params = params.to_vec();
    params.push(("Version", version));
    let body = aws::query_string(&params);
    let hash = hex::encode(Sha256::digest(body.as_bytes()));
    let headers = [(
        "content-type",
        "application/x-www-form-urlencoded; charset=utf-8",
    )];
    let signed = aws::sign(
        &aws::Request {
            method: "POST",
            host: &host,
            path: "/",
            query: &[],
            headers: &headers,
            payload_sha256: &hash,
        },
        credentials,
        region,
        service,
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    );
    let mut request = http.post(format!("{base}/")).timeout(limit).body(body);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    for (name, value) in signed {
        request = request.header(name, value);
    }
    let response = request.send().map_err(reqwest::Error::without_url)?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    response.take(RESPONSE_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        bail!("AWS response exceeds 4 MiB");
    }
    Ok((status, clean(credentials, &String::from_utf8(bytes)?)))
}
