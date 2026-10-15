//! The S3 client of the writer: a put of a new object (`If-None-Match: *`) and ListObjectsV2,
//! which is all that its write-only key may do. desk_core::aws signs and finds the dual-stack
//! endpoint, as for the gateway (src/aws.rs); this client differs in its credentials (the key of
//! the vault configuration, no role) and in its time limits, which grow with the size of a part.
//! reqwest takes its roots from webpki-roots, so a static binary needs no certificate store.

use anyhow::{anyhow, bail, Context, Result};
use bytes::Bytes;
use desk_core::aws::{self, Credentials, EMPTY_SHA256};
use reqwest::Method;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The time limit of a call without a body, and the base of one with a body.
const LIMIT: Duration = Duration::from_secs(120);
/// The slowest upload that a put waits for, in bytes a second.
const SLOWEST: u64 = 100_000;
/// Switch to S3 multipart before the single-request limit of 5 GB.
const MULTIPART_THRESHOLD: usize = 256 << 20;
/// S3 permits at most 10,000 upload parts, each at most 5 GiB.
const S3_MAX_PARTS: usize = 10_000;
const S3_MAX_PART_BYTES: usize = 5 * 1024 * 1024 * 1024;
const S3_PART_BYTES: usize = 64 << 20;

struct Reply {
    status: u16,
    headers: reqwest::header::HeaderMap,
    body: String,
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub struct S3 {
    http: reqwest::Client,
    credentials: Credentials,
    bucket: String,
    region: String,
    endpoint: Option<String>,
}

impl S3 {
    /// `endpoint` (`http://127.0.0.1:9000`) replaces AWS with a path-style test store.
    pub fn new(
        bucket: &str,
        region: &str,
        credentials: Credentials,
        endpoint: Option<&str>,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("layr-desk")
            .connect_timeout(Duration::from_secs(30))
            .tcp_keepalive(Duration::from_secs(60))
            .build()?;
        Ok(S3 {
            http,
            credentials,
            bucket: bucket.to_string(),
            region: region.to_string(),
            endpoint: endpoint.map(str::to_string),
        })
    }

    /// The status and body of one signed call.
    async fn call(
        &self,
        method: Method,
        key: &str,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        body: Bytes,
    ) -> Result<Reply> {
        let (base, path) =
            aws::s3_location(&self.bucket, &self.region, self.endpoint.as_deref(), key);
        let url = reqwest::Url::parse(&base)?;
        // The Host header that reqwest sends: the port only when it is not the default one.
        let host = match (url.host_str(), url.port()) {
            (Some(h), Some(p)) => format!("{h}:{p}"),
            (Some(h), None) => h.to_string(),
            _ => bail!("no host in {base}"),
        };
        let payload = if body.is_empty() {
            EMPTY_SHA256.to_string()
        } else {
            hex::encode(Sha256::digest(&body))
        };
        let unix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let signed = aws::sign(
            &aws::Request {
                method: method.as_str(),
                host: &host,
                path: &path,
                query,
                headers,
                payload_sha256: &payload,
            },
            &self.credentials,
            &self.region,
            "s3",
            unix,
        );
        let mut target = format!("{base}{}", aws::uri_encode(&path, true));
        if !query.is_empty() {
            target = format!("{target}?{}", aws::query_string(query));
        }
        // A time limit for the whole call: reqwest's read timeout would end an upload that
        // takes longer than the limit, however steadily it goes.
        let limit = LIMIT + Duration::from_secs(body.len() as u64 / SLOWEST);
        let has_body = method == Method::PUT || method == Method::POST;
        let mut request = self.http.request(method, target).timeout(limit);
        for (name, value) in headers.iter().copied() {
            request = request.header(name, value);
        }
        for (name, value) in &signed {
            request = request.header(name, value);
        }
        if has_body {
            request = request.body(body);
        }
        let response = request.send().await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        Ok(Reply {
            status,
            headers,
            body: response.text().await?,
        })
    }

    async fn abort(&self, key: &str, id: &str) -> Result<()> {
        let reply = self
            .call(Method::DELETE, key, &[("uploadId", id)], &[], Bytes::new())
            .await?;
        if reply.status != 204 && reply.status != 404 {
            return Err(failure("S3 abort multipart", reply.status, &reply.body));
        }
        Ok(())
    }

    async fn multipart(&self, key: &str, body: Bytes, part_size: usize) -> Result<bool> {
        // Validate before creating an upload that would need manual cleanup.
        if part_size == 0
            || part_size > S3_MAX_PART_BYTES
            || body.len().div_ceil(part_size) > S3_MAX_PARTS
        {
            bail!("S3 multipart object exceeds the part limits");
        }
        let reply = self
            .call(Method::POST, key, &[("uploads", "")], &[], Bytes::new())
            .await?;
        if reply.status != 200 {
            return Err(failure("S3 create multipart", reply.status, &reply.body));
        }
        let id = aws::xml_text(&reply.body, "UploadId")
            .filter(|s| !s.is_empty())
            .context("S3 create multipart returned no upload ID")?;
        let outcome = self.upload_parts(key, &id, body, part_size).await;
        if !matches!(outcome, Ok(true)) {
            if let Err(cleanup) = self.abort(key, &id).await {
                bail!("S3 multipart {key}, upload {id}: {outcome:?}; cleanup failed: {cleanup:#}");
            }
        }
        outcome.with_context(|| format!("S3 multipart {key}, upload {id}"))
    }

    async fn upload_parts(
        &self,
        key: &str,
        id: &str,
        body: Bytes,
        part_size: usize,
    ) -> Result<bool> {
        let mut complete = String::from("<CompleteMultipartUpload>");
        for (index, start) in (0..body.len()).step_by(part_size).enumerate() {
            let number = (index + 1).to_string();
            let part = body.slice(start..(start + part_size).min(body.len()));
            let query = [("partNumber", number.as_str()), ("uploadId", id)];
            // UploadPart replaces only this part of this upload. Retrying the same bytes
            // after a lost response does not create another object or upload.
            let mut attempt = 0;
            let reply = loop {
                let result = self.call(Method::PUT, key, &query, &[], part.clone()).await;
                let retry = match &result {
                    Err(_) => true,
                    Ok(r) => r.status == 429 || r.status >= 500,
                };
                if !retry || attempt == 2 {
                    break result?;
                }
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(attempt)).await;
            };
            if reply.status != 200 {
                return Err(failure("S3 upload part", reply.status, &reply.body));
            }
            let etag = reply
                .headers
                .get("etag")
                .context("S3 upload part returned no ETag")?
                .to_str()?;
            if etag.is_empty() {
                bail!("S3 upload part returned an empty ETag");
            }
            complete.push_str(&format!(
                "<Part><PartNumber>{number}</PartNumber><ETag>{}</ETag></Part>",
                xml_escape(etag)
            ));
        }
        complete.push_str("</CompleteMultipartUpload>");
        let reply = self
            .call(
                Method::POST,
                key,
                &[("uploadId", id)],
                &[("if-none-match", "*"), ("content-type", "application/xml")],
                Bytes::from(complete),
            )
            .await?;
        if reply.status == 412
            || (reply.status == 200
                && reply.body.contains("<Error")
                && aws::xml_text(&reply.body, "Code").as_deref() == Some("PreconditionFailed"))
        {
            return Ok(false);
        }
        // S3 can send HTTP 200 first, then an Error XML body. Only a complete result
        // confirms publication. A lost response remains an error, never false success.
        if reply.status != 200
            || !reply.body.contains("<CompleteMultipartUploadResult")
            || reply.body.contains("<Error")
            || aws::xml_text(&reply.body, "ETag").is_none()
        {
            return Err(failure("S3 complete multipart", reply.status, &reply.body));
        }
        Ok(true)
    }
}

/// What a failed call says: S3's code and message, which hold no secret.
fn failure(what: &str, status: u16, reply: &str) -> anyhow::Error {
    let code = aws::xml_text(reply, "Code").unwrap_or_default();
    let message = aws::xml_text(reply, "Message").unwrap_or_default();
    anyhow!("{what}: HTTP {status} {code} {message}")
}

impl desk_core::writer::Bucket for S3 {
    async fn put_new(&self, key: &str, body: Vec<u8>) -> Result<bool> {
        let body = Bytes::from(body);
        if body.len() > MULTIPART_THRESHOLD {
            let part_size = S3_PART_BYTES.max(body.len().div_ceil(S3_MAX_PARTS));
            return self.multipart(key, body, part_size).await;
        }
        let reply = self
            .call(Method::PUT, key, &[], &[("if-none-match", "*")], body)
            .await
            .with_context(|| format!("S3 put {key}"))?;
        match reply.status {
            200 => Ok(true),
            412 => Ok(false),
            _ => Err(failure(&format!("S3 put {key}"), reply.status, &reply.body)),
        }
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let (mut keys, mut token) = (Vec::new(), None::<String>);
        loop {
            let mut query = vec![("list-type", "2"), ("prefix", prefix)];
            if let Some(token) = &token {
                query.push(("continuation-token", token));
            }
            let reply = self
                .call(Method::GET, "", &query, &[], Bytes::new())
                .await
                .with_context(|| format!("S3 list {prefix}"))?;
            if reply.status != 200 {
                return Err(failure(
                    &format!("S3 list {prefix}"),
                    reply.status,
                    &reply.body,
                ));
            }
            if !reply.body.contains("<ListBucketResult") {
                bail!("S3 list {prefix}: the reply is not a listing");
            }
            keys.extend(aws::xml_texts(&reply.body, "Key"));
            token = aws::xml_text(&reply.body, "NextContinuationToken").filter(|t| !t.is_empty());
            if token.is_none() {
                return Ok(keys);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, Response};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy)]
    enum Outcome {
        Success,
        Exists,
        EmbeddedExists,
        EmbeddedError,
        Conflict,
        PartError,
        LostCompletion,
    }

    #[tokio::test]
    async fn multipart_preserves_bytes_and_conditions_and_cleans_failures() {
        for outcome in [
            Outcome::Success,
            Outcome::Exists,
            Outcome::EmbeddedExists,
            Outcome::EmbeddedError,
            Outcome::Conflict,
            Outcome::PartError,
            Outcome::LostCompletion,
        ] {
            let received = Arc::new(Mutex::new(Vec::<u8>::new()));
            let calls = Arc::new(Mutex::new(Vec::<String>::new()));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let saved = received.clone();
            let seen = calls.clone();
            let task = tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let received = saved.clone();
                    let calls = seen.clone();
                    tokio::spawn(async move {
                        let service = hyper::service::service_fn(
                            move |request: Request<hyper::body::Incoming>| {
                                let received = received.clone();
                                let calls = calls.clone();
                                async move {
                                    let (parts, body) = request.into_parts();
                                    let body = body.collect().await.unwrap().to_bytes();
                                    assert_eq!(
                                        parts.headers["x-amz-content-sha256"],
                                        hex::encode(Sha256::digest(&body))
                                    );
                                    assert!(parts.headers.contains_key("authorization"));
                                    let query = parts.uri.query().unwrap_or("");
                                    let label = format!("{} {}", parts.method, query);
                                    calls.lock().unwrap().push(label);
                                    let mut response = Response::new(Full::new(Bytes::new()));
                                    if query == "uploads=" {
                                        assert_eq!(parts.method, Method::POST);
                                        *response.body_mut() = Full::new(Bytes::from_static(b"<InitiateMultipartUploadResult><UploadId>a+/=&amp;b</UploadId></InitiateMultipartUploadResult>"));
                                    } else {
                                        assert!(query.contains("uploadId=a%2B%2F%3D%26b"));
                                        if parts.method == Method::DELETE {
                                            *response.status_mut() = hyper::StatusCode::NO_CONTENT;
                                        } else if parts.method == Method::PUT {
                                            assert!(!parts.headers.contains_key("if-none-match"));
                                            if matches!(outcome, Outcome::PartError) {
                                                *response.status_mut() =
                                                    hyper::StatusCode::FORBIDDEN;
                                            } else {
                                                received.lock().unwrap().extend_from_slice(&body);
                                                response.headers_mut().insert(
                                                    "etag",
                                                    "\"part&etag\"".parse().unwrap(),
                                                );
                                            }
                                        } else {
                                            assert_eq!(parts.headers["if-none-match"], "*");
                                            assert_eq!(
                                                parts.headers["content-type"],
                                                "application/xml"
                                            );
                                            let xml = String::from_utf8(body.to_vec()).unwrap();
                                            assert_eq!(xml.matches("<Part>").count(), 3);
                                            assert!(xml.contains("<PartNumber>3</PartNumber>"));
                                            assert!(xml.contains("&quot;part&amp;etag&quot;"));
                                            match outcome {
                                            Outcome::Success => *response.body_mut() = Full::new(Bytes::from_static(b"<CompleteMultipartUploadResult><ETag>complete</ETag></CompleteMultipartUploadResult>")),
                                            Outcome::Exists => *response.status_mut() = hyper::StatusCode::PRECONDITION_FAILED,
                                            Outcome::EmbeddedExists => *response.body_mut() = Full::new(Bytes::from_static(b" \n<Error><Code>PreconditionFailed</Code></Error>")),
                                            Outcome::Conflict => *response.status_mut() = hyper::StatusCode::CONFLICT,
                                            Outcome::EmbeddedError => *response.body_mut() = Full::new(Bytes::from_static(b" \n<Error><Code>InternalError</Code></Error>")),
                                            Outcome::LostCompletion => return Err(std::io::Error::other("completion response lost")),
                                            Outcome::PartError => panic!("must not complete failed parts"),
                                        }
                                        }
                                    }
                                    Ok::<_, std::io::Error>(response)
                                }
                            },
                        );
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                            .await;
                    });
                }
            });
            let client = S3::new(
                "bucket",
                "us-east-1",
                Credentials {
                    key_id: "key".into(),
                    secret: "secret".into(),
                    token: None,
                },
                Some(&endpoint),
            )
            .unwrap();
            let data = Bytes::from_static(b"0123456789");
            let result = client
                .multipart("project/states/object", data.clone(), 4)
                .await;
            match outcome {
                Outcome::Success => assert!(result.unwrap()),
                Outcome::Exists | Outcome::EmbeddedExists => assert!(!result.unwrap()),
                _ => assert!(result.is_err()),
            }
            let calls = calls.lock().unwrap();
            assert_eq!(
                calls.iter().filter(|s| s.starts_with("DELETE")).count(),
                usize::from(!matches!(outcome, Outcome::Success))
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|s| s.as_str() == "POST uploads=")
                    .count(),
                1
            );
            if !matches!(outcome, Outcome::PartError) {
                assert_eq!(*received.lock().unwrap(), data);
            }
            task.abort();
        }
    }
}

/// Explicit live check against an existing private vault. The caller supplies an isolated
/// object key, a presigned GET URL for that key, and an actual source file. No credentials
/// or file contents appear in the result. The caller removes the test object's versions.
#[cfg(test)]
mod live_tests {
    use super::*;
    use desk_core::{keys, zst};
    use serde::Deserialize;
    use std::path::PathBuf;

    // A valid zstd frame with raw blocks makes the encrypted object exceed the
    // single-request limit even when the source file compresses well.
    fn raw_frame(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() + data.len().div_ceil(128 << 10) * 3 + 16);
        // Keep the decode window at 128 KiB, independent of the total file size.
        out.extend_from_slice(&[0x28, 0xb5, 0x2f, 0xfd, 0xc0, 0x38]);
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        let count = data.len().div_ceil(128 << 10);
        for (index, block) in data.chunks(128 << 10).enumerate() {
            let header = ((block.len() as u32) << 3) | u32::from(index + 1 == count);
            out.extend_from_slice(&header.to_le_bytes()[..3]);
            out.extend_from_slice(block);
        }
        if data.is_empty() {
            out.extend_from_slice(&[1, 0, 0]);
        }
        out
    }

    #[test]
    fn raw_test_frames_preserve_block_boundaries() {
        for size in [0, 1, 128 << 10, (128 << 10) + 1] {
            let data = vec![42u8; size];
            assert_eq!(zst::decompress(&raw_frame(&data), None).unwrap(), data);
        }
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Config {
        bucket: String,
        region: String,
        access_key_id: String,
        secret_access_key: String,
    }

    #[tokio::test]
    #[ignore = "requires an explicitly selected AWS vault, source file and cleanup"]
    async fn encrypted_multipart_round_trip_and_overwrite_protection() {
        let dir = PathBuf::from(std::env::var("LAYR_MULTIPART_CHECK_DIR").unwrap());
        let config: Config =
            serde_json::from_slice(&std::fs::read(dir.join("config.json")).unwrap()).unwrap();
        let key = std::fs::read_to_string(dir.join("object-key")).unwrap();
        let source = std::fs::read_to_string(dir.join("source-path")).unwrap();
        let original = std::fs::read(source.trim()).unwrap();
        let digest = hex::encode(Sha256::digest(&original));
        let identity = keys::generate_identity();
        let recipient = keys::recipient_of(&identity).unwrap();
        let source_bytes = original.len();
        let raw = std::env::var("LAYR_MULTIPART_RAW_FRAME").as_deref() == Ok("1");
        let frame = if raw {
            raw_frame(&original)
        } else {
            zst::compress(&original, None).unwrap()
        };
        let encrypted = Bytes::from(keys::encrypt(&recipient, &frame).unwrap());
        drop(frame);
        drop(original);
        let client = S3::new(
            &config.bucket,
            &config.region,
            Credentials {
                key_id: config.access_key_id,
                secret: config.secret_access_key,
                token: None,
            },
            None,
        )
        .unwrap();
        assert!(client
            .multipart(key.trim(), encrypted.clone(), 5 << 20)
            .await
            .unwrap());
        assert!(!client
            .multipart(key.trim(), encrypted.clone(), 5 << 20)
            .await
            .unwrap());
        let unconditioned = client
            .call(
                Method::PUT,
                key.trim(),
                &[],
                &[],
                Bytes::from_static(b"must be denied"),
            )
            .await
            .unwrap();
        assert_eq!(
            unconditioned.status, 403,
            "bucket must reject unconditional replacement"
        );
        let url = std::fs::read_to_string(dir.join("get-url")).unwrap();
        let downloaded = reqwest::get(url.trim())
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(downloaded, encrypted);
        let plain = keys::decrypt(&identity, &downloaded).unwrap();
        let restored = zst::decompress_with_limit(&plain, None, source_bytes).unwrap();
        assert_eq!(hex::encode(Sha256::digest(&restored)), digest);
        std::fs::write(dir.join("result.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "verified":true, "sourceBytes":source_bytes, "rawZstdFrame":raw, "encryptedBytes":encrypted.len(),
            "sha256":digest, "conditionalOverwriteRejected":true, "unconditionalOverwriteRejected":true,
            "key":key.trim()
        })).unwrap()).unwrap();
    }
}
