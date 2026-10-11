//! layr-desk: read vaults through a gateway.
//!
//!   layr-desk ls     --site https://desk.example.com --project <p> [--paper-file <file>]
//!   layr-desk verify --site https://desk.example.com --project <p> --source <folder>
//!
//! The paper key comes from `--paper-file` (24 words or the age form) or from a hidden prompt.

mod gateway;

use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use desk_core::Reader;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "layr-desk", about = "End-to-end encrypted project copies")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Target {
    /// The Desk site, for example https://desk.layr.ws
    #[arg(long)]
    site: String,
    /// The vault (project prefix)
    #[arg(long)]
    project: String,
    /// A file with the paper key; without it the key is asked for
    #[arg(long)]
    paper_file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// The vaults that a gateway serves
    Projects {
        #[arg(long)]
        site: String,
    },
    /// The files of the latest state of a vault
    Ls(Target),
    /// Compare the latest state of a vault with a folder, file by file
    Verify {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        source: PathBuf,
    },
}

fn paper(target: &Target) -> Result<String> {
    match &target.paper_file {
        Some(file) => Ok(std::fs::read_to_string(file)?),
        None => Ok(rpassword::prompt_password(
            "Paper key of the vault (hidden): ",
        )?),
    }
}

async fn open(target: &Target) -> Result<Reader<gateway::Vault>> {
    let mut vault = gateway::Site::new(&target.site)?.vault(&target.project);
    vault.unlock_with_paper(&paper(target)?).await?;
    let names = vault.states().await?;
    let identity = vault.identity()?.to_string();
    Reader::new(vault, &identity, &names)
}

/// The bytes and mode of a file or link of the source folder, as the writer reads them.
fn content(path: &Path) -> Result<(Vec<u8>, &'static str)> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        use std::os::unix::ffi::OsStrExt;
        return Ok((
            std::fs::read_link(path)?.as_os_str().as_bytes().to_vec(),
            "120000",
        ));
    }
    use std::os::unix::fs::PermissionsExt;
    let mode = if meta.permissions().mode() & 0o111 != 0 {
        "100755"
    } else {
        "100644"
    };
    Ok((std::fs::read(path)?, mode))
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Projects { site } => {
            for name in gateway::Site::new(&site)?.projects().await? {
                println!("{name}");
            }
        }
        Command::Ls(target) => {
            let reader = open(&target).await?;
            for (path, entry) in reader.files(None).await? {
                println!("{} {:>10} {path}", entry.mode(), entry.size());
            }
        }
        Command::Verify { target, source } => {
            let reader = open(&target).await?;
            let (seq, _) = reader.latest()?;
            let files = reader.files(None).await?;
            let mut problems = 0;
            for (path, entry) in &files {
                let data = reader.read(entry).await?;
                let (want, mode) = content(&source.join(path))?;
                if Sha256::digest(&data) != Sha256::digest(&want) || mode != entry.mode() {
                    println!("differs: {path}");
                    problems += 1;
                }
            }
            println!("state {seq}: {} files, {problems} differ", files.len());
            if problems > 0 {
                bail!("the vault and the folder differ");
            }
        }
    }
    Ok(())
}
