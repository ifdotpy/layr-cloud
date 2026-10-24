mod account;
mod credentials;
mod deploy;
mod query;
#[cfg(test)]
mod tests;

use anyhow::{bail, Context, Result};
use std::{collections::BTreeMap, path::PathBuf};

const USAGE: &str = "layr-cloud deploy --region <r> --stack-prefix <p> --release <dir> [--profile <name>] [--dry-run]";

pub struct Args {
    region: String,
    prefix: String,
    release: PathBuf,
    profile: Option<String>,
    dry_run: bool,
}

fn arguments(args: impl IntoIterator<Item = String>) -> Result<Option<Args>> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("--help" | "-h") => return Ok(None),
        Some("deploy") => {}
        _ => bail!("Usage: {USAGE}"),
    }
    let mut flags = BTreeMap::new();
    let mut dry_run = false;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--help" | "-h" => return Ok(None),
            "--dry-run" if !dry_run => dry_run = true,
            "--region" | "--stack-prefix" | "--release" | "--profile" => {
                let value = args
                    .next()
                    .filter(|v| !v.is_empty() && !v.starts_with("--"))
                    .with_context(|| format!("{flag} needs a value"))?;
                if flags.insert(flag.clone(), value).is_some() {
                    bail!("duplicate option {flag}");
                }
            }
            _ => bail!("unknown option {flag}\nUsage: {USAGE}"),
        }
    }
    let mut required = |key: &str| {
        flags
            .remove(key)
            .with_context(|| format!("missing {key}\nUsage: {USAGE}"))
    };
    let region = required("--region")?;
    let prefix = required("--stack-prefix")?;
    let release = required("--release")?.into();
    // The prefix is also the store prefix and the host name prefix.
    if prefix.is_empty()
        || prefix.len() > 44
        || !prefix.as_bytes()[0].is_ascii_lowercase()
        || !prefix
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || prefix.ends_with('-')
    {
        bail!("stack prefix must start with a lowercase letter, end with a letter or digit, and contain at most 44 lowercase letters, digits, or hyphens");
    }
    if region.is_empty()
        || !region
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("invalid AWS region");
    }
    Ok(Some(Args {
        region,
        prefix,
        release,
        profile: flags.remove("--profile"),
        dry_run,
    }))
}

fn run() -> Result<()> {
    let Some(args) = arguments(std::env::args().skip(1))? else {
        println!("Usage: {USAGE}");
        return Ok(());
    };
    deploy::run(args)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("layr-cloud: {error:#}");
        std::process::exit(
            if error
                .downcast_ref::<layr_store::aws::profile::SignInNeeded>()
                .is_some()
            {
                2
            } else {
                1
            },
        );
    }
}
