//! The network disk end to end: a fake gateway serves the fixture vault that the Python writer
//! made, the disk serves it over WebDAV, and every file of the latest state reads back as the
//! writer wrote it. A vault without a key on this machine shows how to unlock it.

use crate::{
    disk::Disk,
    keystore::{KeyStore, Memory},
};
use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, StatusCode};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../js/vault/test/fixture");
const TOKEN: &str = "token-of-the-test";

async fn gateway(
    request: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let states = PathBuf::from(FIXTURE).join("objects/proj/states");
    let path = request.uri().path().to_string();
    let authorized = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        == Some(&format!("Bearer {TOKEN}"));
    let json = |v: serde_json::Value| Response::new(Full::new(Bytes::from(v.to_string())));
    let status = |s: StatusCode| {
        let mut r = Response::new(Full::new(Bytes::new()));
        *r.status_mut() = s;
        r
    };
    Ok(match path.as_str() {
        "/api/projects" => json(serde_json::json!({"projects": ["locked", "proj"]})),
        "/api/p/proj/challenge" => {
            let recipient = desk_core::keys::recipient_of(
                &std::fs::read_to_string(PathBuf::from(FIXTURE).join("identity.txt")).unwrap(),
            )
            .unwrap();
            let sealed = desk_core::keys::encrypt(&recipient, TOKEN.as_bytes()).unwrap();
            json(
                serde_json::json!({"challenge": base64::engine::general_purpose::STANDARD.encode(sealed)}),
            )
        }
        _ if !authorized => status(StatusCode::UNAUTHORIZED),
        "/api/p/proj/states" => {
            let names: Vec<String> = std::fs::read_dir(&states)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            json(serde_json::json!({"states": names}))
        }
        p => match p
            .strip_prefix("/api/p/proj/states/")
            .and_then(|n| std::fs::read(states.join(n)).ok())
        {
            Some(data) => Response::new(Full::new(Bytes::from(data))),
            None => status(StatusCode::NOT_FOUND),
        },
    })
}

async fn listen<F, Fut>(handler: F) -> String
where
    F: Fn(Request<hyper::body::Incoming>) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Response<Full<Bytes>>, std::convert::Infallible>>
        + Send
        + 'static,
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let handler = handler.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(handler);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    url
}

#[tokio::test]
async fn the_disk_shows_the_latest_state_of_each_vault() {
    let site = listen(gateway).await;
    let keys = Memory::default();
    let identity = std::fs::read_to_string(PathBuf::from(FIXTURE).join("identity.txt")).unwrap();
    keys.set(
        &crate::keystore::account(&site, "proj").unwrap(),
        identity.trim(),
    )
    .unwrap();
    let cache = std::env::temp_dir().join(format!("layr-desk-test-{}", std::process::id()));
    let disk = Arc::new(
        Disk::new(
            &site,
            Box::new(keys),
            cache.clone(),
            std::time::Duration::from_secs(60),
        )
        .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!(
        "http://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    tokio::spawn(crate::run(listener, disk));
    let http = reqwest::Client::new();
    let propfind = reqwest::Method::from_bytes(b"PROPFIND").unwrap();

    let root = http
        .request(propfind.clone(), &base)
        .header("Depth", "1")
        .send()
        .await
        .unwrap();
    assert_eq!(root.status(), StatusCode::MULTI_STATUS);
    let root = root.text().await.unwrap();
    assert!(
        root.contains("<D:href>/proj/</D:href>") && root.contains("<D:href>/locked/</D:href>"),
        "{root}"
    );

    let locked = http
        .get(format!("{base}locked/LOCKED.txt"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        locked.contains("layr-desk unlock --site") && locked.contains("--project locked"),
        "{locked}"
    );

    let expected: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(PathBuf::from(FIXTURE).join("expected.json")).unwrap(),
    )
    .unwrap();
    let last = expected["states"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_object()
        .unwrap();
    for (path, want) in last {
        let response = http.get(format!("{base}proj/{path}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let data = response.bytes().await.unwrap();
        if want[1] != "120000" {
            assert_eq!(hex::encode(Sha256::digest(&data)), want[0], "{path}");
        }
    }
    let first = last.keys().find(|p| last[*p][1] == "100644").unwrap();
    let part = http
        .get(format!("{base}proj/{first}"))
        .header("Range", "bytes=0-0")
        .send()
        .await
        .unwrap();
    assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
    let put = http
        .put(format!("{base}proj/new.txt"))
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        http.get(format!("{base}proj/missing"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let _ = std::fs::remove_dir_all(cache);
}
