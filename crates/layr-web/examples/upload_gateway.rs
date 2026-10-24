//! Upload disposable fixture objects through the existing S3 client.
use anyhow::{Context, Result};
use layr_store::{aws::Credentials, s3::S3};
use std::{env, fs, path::Path, time::Duration};
fn upload(s3: &S3, root: &Path, at: &Path, prefix: &str) -> Result<()> {
    for entry in fs::read_dir(at)? {
        let path = entry?.path();
        if path.is_dir() {
            upload(s3, root, &path, prefix)?;
        } else {
            let key = format!(
                "{prefix}{}",
                path.strip_prefix(root)?
                    .to_str()
                    .context("path is not UTF-8")?
            );
            anyhow::ensure!(
                s3.put_new(&key, fs::read(&path)?)?,
                "fixture object already exists"
            );
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let bucket = args.first().context("bucket")?;
    let credentials = Credentials {
        key_id: env::var("AWS_ACCESS_KEY_ID")?,
        secret: env::var("AWS_SECRET_ACCESS_KEY")?,
        token: None,
    };
    let endpoint = env::var("AWS_ENDPOINT_URL_S3")?;
    let http = reqwest::blocking::Client::builder().no_proxy().build()?;
    let s3 = S3::new_with_client(bucket, "us-east-1", credentials, Some(&endpoint), http)?;
    let response = s3.request(
        reqwest::Method::PUT,
        "",
        &[],
        &[],
        Vec::new(),
        Duration::from_secs(10),
    )?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text()?;
        let code = body
            .split("<Code>")
            .nth(1)
            .and_then(|value| value.split("</Code>").next())
            .unwrap_or("unknown");
        anyhow::bail!("create fixture bucket failed: HTTP {status}, {code}");
    }
    let store = Path::new(args.get(1).context("store path")?);
    let people = Path::new(args.get(2).context("people path")?);
    upload(&s3, store, store, "fixture/")?;
    upload(&s3, people, people, "people/")?;
    Ok(())
}
