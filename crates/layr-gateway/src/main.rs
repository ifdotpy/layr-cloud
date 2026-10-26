//! Public IPv6 object gateway. It runs independently of the local Layr service and store.

mod auth;
mod cdn;
mod aws;
mod config;
mod decoy;
mod epochs;
mod listing;
#[cfg(test)]
mod minio;
mod origin;
mod people;
mod person_support;
mod public;
mod requests;
mod server;
mod session;
mod site;
mod tls;
mod transport;
#[cfg(test)]
mod whois;

use anyhow::{anyhow, bail, Context, Result};
use std::path::PathBuf;

struct Options {
    config: PathBuf,
    tls: String,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    tls_dir: PathBuf,
    test_static: bool,
    ssm_parameter: Option<String>,
    tailscale_socket: PathBuf,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = command(&args) {
        eprintln!("layr-gateway: {error:#}");
        std::process::exit(1);
    }
}

fn command(args: &[String]) -> Result<()> {
    if args.is_empty() || matches!(args[0].as_str(), "--help" | "-h") {
        println!("usage: layr-gateway serve [options]\n       layr-gateway token <project> --write|--gc [options]");
        return Ok(());
    }
    if args[0] == "invite" {
        return invite_command(&args[1..]);
    }
    if !matches!(args[0].as_str(), "serve" | "token") {
        bail!("expected serve or token");
    }
    let token_command = args[0] == "token";
    let args = if token_command { args } else { &args[1..] };
    let mut options = Options {
        config: "/etc/layr-gateway/config.json".into(),
        tls: "files".into(),
        cert: None,
        key: None,
        tls_dir: "/etc/layr-gateway/tls".into(),
        test_static: false,
        ssm_parameter: None,
        tailscale_socket: "/var/run/tailscale/tailscaled.sock".into(),
    };
    let mut positional = Vec::new();
    let mut write = false;
    let mut gc = false;
    let mut until = "24h".to_string();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--write" => write = true,
            "--gc" => gc = true,
            "--test-static-credentials" if cfg!(debug_assertions) => options.test_static = true,
            "--config" | "--tls" | "--cert" | "--key" | "--tls-dir" | "--until"
            | "--tailscale-socket" | "--ssm-parameter" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("{arg} needs a value"))?;
                match arg.as_str() {
                    "--config" => options.config = value.into(),
                    "--tls" => options.tls = value.clone(),
                    "--cert" => options.cert = Some(value.into()),
                    "--key" => options.key = Some(value.into()),
                    "--tls-dir" => options.tls_dir = value.into(),
                    "--tailscale-socket" => options.tailscale_socket = value.into(),
                    "--until" => until = value.clone(),
                    "--ssm-parameter" => options.ssm_parameter = Some(value.clone()),
                    _ => unreachable!(),
                }
            }
            "--help" | "-h" => {
                println!("usage: layr-gateway serve --config <file> [--tls files|tailscale|none] [--cert <pem> --key <pem>]\n       layr-gateway token <project> --write|--gc [--until <duration>] [--config <file>]");
                return Ok(());
            }
            value if value.starts_with('-') => bail!("unknown gateway option {value}"),
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    if token_command {
        if positional.len() != 2 || write == gc || options.test_static {
            bail!("usage: layr-gateway token <project> --write|--gc [--until <duration>] [--config <file>]");
        }
        let project = &positional[1];
        if !config::project_name(project) {
            bail!("invalid gateway project name");
        }
        if let Some(parameter) = &options.ssm_parameter {
            anyhow::ensure!(
                rustix::process::geteuid().is_root(),
                "store service tokens as the gateway administrator"
            );
            anyhow::ensure!(
                service_parameter(project, parameter, gc),
                "unexpected service token parameter"
            );
        }
        let token = auth::issue(gc, auth::duration(&until)?, auth::now())?;
        config::add_hash(&options.config, project, auth::hash(&token), gc)?;
        if let Some(parameter) = options.ssm_parameter {
            let config = config::ConfigFile::new(options.config.clone()).get()?;
            let region = &config
                .projects
                .get(project)
                .context("unknown project")?
                .region;
            aws::Aws::new(false)?.put_secure_parameter(region, &parameter, &token, true)?;
            println!("Service write token stored in SSM parameter {parameter}");
        } else {
            println!("{token}");
        }
        return Ok(());
    }
    if !positional.is_empty() || write || gc || until != "24h" || options.ssm_parameter.is_some() {
        bail!("invalid gateway server arguments");
    }
    if rustix::process::geteuid().is_root() {
        bail!("layr-gateway refuses to run as root; use a dedicated service user");
    }
    run(options)
}

fn service_parameter(project: &str, parameter: &str, gc: bool) -> bool {
    !gc && config::project_name(project)
        && (matches!(
            parameter,
            "/layr-store/executor/write-token" | "/layr-store/runner/write-token"
        ) || ["owner", "executor", "runner"]
            .iter()
            .any(|kind| parameter == format!("/layr-store/{kind}/projects/{project}/write-token")))
}

fn invite_command(args: &[String]) -> Result<()> {
    use server::Storage;
    let login = args
        .first()
        .context("usage: layr-cloud invite <login> --ssm-parameter <name> [--config <file>]")?;
    anyhow::ensure!(
        layr_store::person::login_name(login),
        "invalid person login"
    );
    anyhow::ensure!(
        rustix::process::geteuid().is_root(),
        "create invites as the gateway administrator through SSM"
    );
    let mut path = PathBuf::from("/etc/layr-gateway/config.json");
    let mut parameter = None;
    let mut flags = args[1..].chunks_exact(2);
    for pair in &mut flags {
        match pair[0].as_str() {
            "--config" => path = pair[1].clone().into(),
            "--ssm-parameter" if parameter.is_none() => parameter = Some(pair[1].clone()),
            _ => bail!("invalid invite option"),
        }
    }
    anyhow::ensure!(flags.remainder().is_empty(), "invite option needs a value");
    let parameter = parameter.context("give --ssm-parameter; the invite is never printed")?;
    anyhow::ensure!(
        parameter.starts_with("/layr-store/invites/")
            && parameter.len() < 512
            && parameter
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b)),
        "invalid invite parameter name"
    );
    let config = config::ConfigFile::new(path).get()?;
    let people = config
        .people
        .as_ref()
        .context("people storage is not configured")?;
    let storage = aws::Aws::new(false)?;
    let bucket = storage.people(people)?;
    let token = session::invite(bucket.as_ref(), &people.prefix, login, auth::now())?;
    storage.put_secure_parameter(&people.region, &parameter, &token, false)?;
    println!("Invite stored in SSM parameter {parameter}; expires in 7 days");
    Ok(())
}

fn run(options: Options) -> Result<()> {
    let _ = tls::provider().as_ref().clone().install_default();
    let storage = std::sync::Arc::new(aws::Aws::new(options.test_static)?);
    serve(options, storage)
}

fn tls_config(
    options: &Options,
    config: &config::Config,
) -> Result<Option<std::sync::Arc<rustls::ServerConfig>>> {
    let (cert, key) = match options.tls.as_str() {
        "none" => {
            if !config.listen.ip().is_loopback() {
                bail!("--tls none requires a loopback listen address");
            }
            return Ok(None);
        }
        "tailscale" => {
            if !config.host.ends_with(".ts.net") {
                bail!("--tls tailscale requires a ts.net host");
            }
            (
                options.tls_dir.join(format!("{}.crt", config.host)),
                options.tls_dir.join(format!("{}.key", config.host)),
            )
        }
        "files" => {
            let directory = std::env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from);
            let cert = options
                .cert
                .clone()
                .or_else(|| directory.as_ref().map(|p| p.join("tls.crt")));
            let key = options
                .key
                .clone()
                .or_else(|| directory.as_ref().map(|p| p.join("tls.key")));
            (
                cert.context("give --cert or systemd credential tls.crt")?,
                key.context("give --key or systemd credential tls.key")?,
            )
        }
        _ => bail!("--tls must be files, tailscale or none"),
    };
    Ok(Some(tls::server(std::sync::Arc::new(
        tls::Reloading::new(cert, key, tls::provider())?,
    ))?))
}

fn allowed_address(ip: std::net::IpAddr) -> bool {
    if ip.is_loopback() {
        return true;
    }
    match ip {
        std::net::IpAddr::V4(ip) => {
            let b = ip.octets();
            b[0] == 100 && (64..128).contains(&b[1])
        }
        std::net::IpAddr::V6(ip) => {
            (ip.segments()[0] & 0xe000) == 0x2000 || ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0]
        }
    }
}

fn serve(options: Options, storage: std::sync::Arc<aws::Aws>) -> Result<()> {
    use std::sync::Arc;
    let file = Arc::new(config::ConfigFile::new(options.config.clone()));
    let config = file.get()?;
    if !allowed_address(config.listen.ip()) {
        bail!("gateway listen must use IPv6 or a loopback address");
    }
    let tls = tls_config(&options, &config)?;
    if options.test_static && !config.listen.ip().is_loopback() {
        bail!("test credentials require loopback");
    }
    let secret = if let Some(region) = config
        .extra
        .get("sessionSecretRegion")
        .and_then(|v| v.as_str())
    {
        storage.session_secret(region)?
    } else {
        anyhow::ensure!(
            config.listen.ip().is_loopback(),
            "public gateway needs a persistent SSM session secret"
        );
        rand::random()
    };
    let origin = config
        .extra
        .get("originSecretRegion")
        .and_then(|v| v.as_str())
        .map(|region| storage.origin_secret(region).and_then(origin::Origin::new))
        .transpose()?;
    let cdn = config.extra.get("cdn").map(|value| -> Result<cdn::Signer> {
        let settings: cdn::Settings = serde_json::from_value(value.clone())?;
        let public_host = config.extra.get("publicHost").and_then(|v| v.as_str()).unwrap_or(&config.host);
        settings.validate(public_host)?;
        let pem = storage.cdn_key(&settings.region, &settings.key_parameter)?;
        cdn::Signer::new(settings, &pem, public_host)
    }).transpose()?;
    let gateway = Arc::new(server::Gateway {
        cdn,
        config: file,
        listen: config.listen,
        proof: auth::Proof::new(),
        storage,
        origin,
        person_secret: Arc::new(secret),
        person_writing: Arc::new(std::sync::Mutex::new(())),
        public_limit: public::Limiter::default(),
        sessions: session::Sessions::default(),
        bootstrap_limit: public::Limiter::default(),
    });
    transport::serve(config.listen, gateway, tls)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_token_parameters_bind_the_project_and_refuse_gc_or_other_secrets() {
        for project in ["layr", "layr-cloud", "layr-studio"] {
            for kind in ["owner", "executor", "runner"] {
                let parameter = format!("/layr-store/{kind}/projects/{project}/write-token");
                assert!(service_parameter(project, &parameter, false));
                assert!(!service_parameter("different-project", &parameter, false));
                assert!(!service_parameter(project, &parameter, true));
            }
        }
        for parameter in [
            "/layr-store/gateway/session-secret",
            "/layr-store/gateway/origin-secret",
            "/layr-store/owner/projects/layr/../layr-cloud/write-token",
            "/layr-store/owner/projects/layr/write-token/extra",
        ] {
            assert!(!service_parameter("layr", parameter, false));
        }
        assert!(service_parameter(
            "layr",
            "/layr-store/executor/write-token",
            false
        ));
    }
    #[test]
    fn wildcard_public_private_lan_and_plaintext_tailnet_binds_are_refused() {
        for address in [
            "0.0.0.0",
            "::",
            "192.168.1.1",
            "10.0.0.1",
            "8.8.8.8",
            "fd00::1",
        ] {
            assert!(!allowed_address(address.parse().unwrap()));
        }
        for address in [
            "127.0.0.1",
            "::1",
            "100.64.0.1",
            "100.127.255.254",
            "fd7a:115c:a1e0::123",
        ] {
            assert!(allowed_address(address.parse().unwrap()));
        }
        let mut config = config::sample();
        config.listen = "100.64.0.1:443".parse().unwrap();
        let options = Options {
            config: "unused".into(),
            tls: "none".into(),
            cert: None,
            key: None,
            tls_dir: "unused".into(),
            test_static: false,
            ssm_parameter: None,
            tailscale_socket: "/var/run/tailscale/tailscaled.sock".into(),
        };
        assert!(tls_config(&options, &config).is_err());
    }
}
