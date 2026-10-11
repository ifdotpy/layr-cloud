//! The person key from a terminal (docs/design.md, "Person key"). `person keys` makes the person
//! identity and its paper key and registers them at a gateway; `person show` prints the public
//! recipient; `person vaults` checks the pinned vaults of the index. `grant` gives a person a
//! vault: a person slot in the vault bucket, and an invite in the person's inbox. The local files
//! are in ~/.config/studio-vault/.person: recipient.txt and paper.age (the dot keeps the folder
//! apart from the vault folders, whose names start with a letter or a digit).

use crate::gateway::Site;
use crate::profile::Profile;
use crate::s3::S3;
use crate::setup::{check_prefix, create_private, settings, show_paper};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use clap::{Args, Subcommand};
use desk_core::person::{self, Index, Pin};
use desk_core::writer::Bucket as _;
use desk_core::{keys, VaultInfo};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

fn folder() -> Result<PathBuf> {
    Ok(settings()?.join(".person"))
}

fn read_recipient(folder: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(folder.join("recipient.txt")) {
        Ok(text) => Ok(Some(text.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("recipient.txt of the person key"),
    }
}

/// The words from a file (`-`: standard input) or from a hidden prompt.
fn paper(file: Option<&Path>, prompt: &str) -> Result<String> {
    match file {
        Some(file) if file.as_os_str() == "-" => Ok(std::io::read_to_string(std::io::stdin())?),
        Some(file) => Ok(std::fs::read_to_string(file)?),
        None => Ok(rpassword::prompt_password(prompt)?),
    }
}

#[derive(Subcommand)]
pub enum Action {
    /// Make your person key and show its paper key once; run it in a terminal of your own
    Keys(Keys),
    /// Print the recipient of your person key, which vault owners need for `grant`
    Show(Show),
    /// Unlock your person key and check each vault of its index against its pin
    Vaults(Vaults),
}

pub async fn run(action: Action) -> Result<()> {
    match action {
        Action::Keys(args) => keys_command(args).await,
        Action::Show(args) => show(args).await,
        Action::Vaults(args) => vaults(args).await,
    }
}

#[derive(Args)]
pub struct Keys {
    /// Register the person key at this Desk site, for example https://desk.layr.ws
    #[arg(long)]
    site: Option<String>,
    /// After the words, wait for Enter, then clear the terminal and its scrollback
    #[arg(long)]
    clear: bool,
}

async fn keys_command(args: Keys) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let folder = folder()?;
    let recipient = match read_recipient(&folder)? {
        Some(recipient) if args.site.is_some() => recipient,
        Some(_) => bail!(
            "{} has a person key already; `layr-desk person show` prints its recipient, and \
             --site registers it at a gateway",
            folder.display()
        ),
        None => {
            let person = keys::generate_identity();
            let paper = keys::generate_identity();
            let slot = keys::seal_slot(&person, &keys::recipient_of(&paper)?)?;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&folder)?;
            // recipient.txt comes last: it marks a folder with a complete key.
            let _ = std::fs::remove_file(folder.join("paper.age"));
            create_private(&folder.join("paper.age"), &slot)?;
            let recipient = keys::recipient_of(&person)?;
            create_private(
                &folder.join("recipient.txt"),
                format!("{recipient}\n").as_bytes(),
            )?;
            show_paper(
                &paper,
                "Paper key of your person key: 24 words. Write them on paper twice, in this order, and keep\n\
                 the copies apart. They are shown once and stored nowhere. They open every vault that you\n\
                 are given, so never type them into an agent or a Studio terminal.",
                args.clear,
                "Your person key is ready.",
            )?;
            recipient
        }
    };
    println!("recipient: {recipient}");
    if let Some(site) = &args.site {
        let slot =
            std::fs::read(folder.join("paper.age")).context("paper.age of the person key")?;
        let (login, new) = Site::new(site)?
            .register(&recipient, &slot)
            .await
            .context("registering the person key")?;
        let what = if new {
            "registered"
        } else {
            "already registered"
        };
        println!("{what} at {site} as {login}");
    }
    Ok(())
}

#[derive(Args)]
pub struct Show {
    /// Also show your login at this Desk site and whether it has this person key
    #[arg(long)]
    site: Option<String>,
}

async fn show(args: Show) -> Result<()> {
    let recipient = read_recipient(&folder()?)?
        .ok_or_else(|| anyhow!("this Mac has no person key yet: layr-desk person keys"))?;
    println!("{recipient}");
    if let Some(site) = &args.site {
        let person = Site::new(site)?.person().await?;
        let state = match person.recipient.as_deref() {
            Some(r) if r == recipient => "has this person key",
            Some(_) => "has ANOTHER person key",
            None => "has no person key yet (layr-desk person keys --site)",
        };
        eprintln!("{site}: you are {}; the site {state}", person.login);
    }
    Ok(())
}

#[derive(Args)]
pub struct Vaults {
    #[arg(long)]
    site: String,
    /// A file with the paper key of your person key; without it the key is asked for
    #[arg(long)]
    paper_file: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

async fn vaults(args: Vaults) -> Result<()> {
    let site = Site::new(&args.site)?;
    let record = site.person().await?;
    let recipient = record
        .recipient
        .clone()
        .ok_or_else(|| anyhow!("{} has no person key for {}", args.site, record.login))?;
    if read_recipient(&folder()?)?.is_some_and(|local| local != recipient) {
        bail!("the gateway has another person key for you than this Mac");
    }
    let words = paper(
        args.paper_file.as_deref(),
        "Paper key of your person key (hidden): ",
    )?;
    let opener = keys::identity_from_paper(&words)?;
    let slot = record
        .slots
        .iter()
        .find(|s| s.name == "paper")
        .ok_or_else(|| anyhow!("your person key has no paper slot at the gateway"))?;
    let identity = keys::open_slot(&slot.bytes()?, &opener)
        .context("these words do not open your person key")?;
    if keys::recipient_of(&identity)? != recipient {
        bail!("the paper slot holds another key than your person key; it may be forged");
    }
    let index = match &record.index {
        Some(data) => person::open_index(&identity, &B64.decode(data)?)?,
        None => Index::default(),
    };
    let mut vaults = Vec::new();
    let mut differ = 0;
    for pin in &index.vaults {
        let checked = match site.vault_info(&pin.project).await {
            Ok(vault) => pin.check(&vault),
            Err(e) => Err(e),
        };
        if checked.is_err() {
            differ += 1;
        }
        vaults.push(json!({"project": pin.project, "recipient": pin.recipient,
            "error": checked.err().map(|e| format!("{e:#}"))}));
    }
    if args.json {
        println!(
            "{}",
            json!({"login": record.login, "vaults": vaults, "invites": record.inbox.len()})
        );
    } else {
        for vault in &vaults {
            match vault["error"].as_str() {
                Some(error) => println!("{} refused: {error}", vault["project"].as_str().unwrap()),
                None => println!("{} ok", vault["project"].as_str().unwrap()),
            }
        }
        if !record.inbox.is_empty() {
            let from: Vec<&str> = record.inbox.iter().map(|i| i.from.as_str()).collect();
            println!(
                "{} invites wait on the site (from {})",
                record.inbox.len(),
                from.join(", ")
            );
        }
    }
    if differ > 0 {
        bail!("{differ} of {} vaults differ from their pins", vaults.len());
    }
    Ok(())
}

#[derive(Args)]
pub struct Grant {
    /// The vault (project prefix)
    #[arg(long)]
    project: String,
    /// The tailnet login of the person, for example ann@example.com
    #[arg(long)]
    person: String,
    /// The person's recipient (`layr-desk person show`), from the person through a channel that
    /// you trust
    #[arg(long)]
    recipient: String,
    /// The Desk site whose inbox gets the invite; the site of `deploy` by default
    #[arg(long)]
    site: Option<String>,
    /// The AWS profile of the vault's account; the profile of `deploy` by default
    #[arg(long)]
    profile: Option<String>,
    /// A file with the paper key of the vault, or of your person key; without it the key is
    /// asked for
    #[arg(long)]
    paper_file: Option<PathBuf>,
    /// Give the first person slot of the vault to someone else than you. The vault then no longer
    /// shows to any login without a person slot: not to other tailnet members, not to tagged
    /// devices, and not to the layr-desk disk or lists of your own Mac
    #[arg(long)]
    hide_from_others: bool,
}

/// cloud.json of `deploy`.
#[derive(Deserialize)]
struct Cloud {
    profile: String,
    region: String,
    bucket: String,
    site: String,
}

/// Give a person a vault: the vault identity sealed to the person's recipient as a person slot,
/// and an invite with the vault's pin in the person's inbox.
pub async fn grant(args: Grant) -> Result<()> {
    check_prefix(&args.project)?;
    if !person::login_name(&args.person) {
        bail!(
            "{:?} is not a tailnet login such as ann@example.com",
            args.person
        );
    }
    let recipient = args.recipient.trim();
    if !keys::valid_recipient(recipient) {
        bail!("the recipient is not an age recipient (age1…)");
    }
    let vault_folder = settings()?.join(&args.project);
    let read = |name: &str| {
        std::fs::read(vault_folder.join(name)).with_context(|| {
            format!(
                "{}: set up the vault from this Mac first (layr-desk keys and deploy)",
                vault_folder.join(name).display()
            )
        })
    };
    let vault_recipient = String::from_utf8(read("recipient.txt")?)?
        .trim()
        .to_string();
    let cloud: Cloud = serde_json::from_slice(&read("cloud.json")?).context("cloud.json")?;
    let profile = Profile::load(args.profile.as_deref().unwrap_or(&cloud.profile))?;
    let credentials = profile.credentials(&reqwest::Client::new()).await?;
    let endpoint = std::env::var("AWS_ENDPOINT_URL_S3")
        .ok()
        .filter(|e| !e.is_empty());
    let bucket = S3::new(
        &cloud.bucket,
        &cloud.region,
        credentials,
        endpoint.as_deref(),
    )?;
    let slots = format!("{}/slots/person-", args.project);
    let granted = bucket.list(&slots).await?;
    let site = args.site.as_deref().unwrap_or(&cloud.site);

    // The first person slot hides the vault from every login without one, the owner's included.
    // So the owner grants their own login first, or says that the vault is to hide from others.
    if granted.is_empty() && !args.hide_from_others {
        let own = Site::new(site)?.person().await.ok().map(|p| p.login);
        if own.as_deref() != Some(args.person.as_str()) {
            let you = match &own {
                Some(login) => format!("you are {login} at {site}"),
                None => format!("{site} cannot tell who you are"),
            };
            bail!(
                "{p} has no person slot yet, and {you}. The first person slot hides the vault from \
                 every login without one: from other tailnet members, from tagged devices, and \
                 from the layr-desk disk and lists of a Mac whose login has no slot. Grant \
                 yourself first: layr-desk grant --project {p} --person <your login> --recipient \
                 \"$(layr-desk person show)\". Or pass --hide-from-others. Nothing is written.",
                p = args.project
            );
        }
    }

    // The vault identity, from the vault's paper slot or from the owner's person slot.
    let words = paper(
        args.paper_file.as_deref(),
        "Paper key of the vault, or of your person key (hidden): ",
    )?;
    let opener = keys::identity_from_paper(&words)?;
    let identity = match keys::open_slot(&read("paper.age")?, &opener) {
        Ok(identity) => identity,
        Err(_) => {
            let own = std::fs::read(folder()?.join("paper.age")).map_err(|_| {
                anyhow!("these words do not open the vault, and this Mac has no person key")
            })?;
            let person = keys::open_slot(&own, &opener)
                .map_err(|_| anyhow!("these words open neither the vault nor your person key"))?;
            let mut found = None;
            for key in &granted {
                if let Some(slot) = bucket.get(key).await? {
                    if let Ok(identity) = keys::open_slot(&slot, &person) {
                        found = Some(identity);
                        break;
                    }
                }
            }
            found.ok_or_else(|| {
                anyhow!("your person key has no slot in the vault {}", args.project)
            })?
        }
    };
    if keys::recipient_of(&identity)? != vault_recipient {
        bail!("the slot holds another key than the vault's; it may be forged");
    }
    let record = format!("{}/vault.json", args.project);
    let vault: VaultInfo = serde_json::from_str(
        &bucket
            .get_text(&record)
            .await?
            .ok_or_else(|| anyhow!("the vault has no vault.json; run layr-desk deploy"))?,
    )
    .context("vault.json")?;
    if vault.recipient.trim() != vault_recipient {
        bail!("the vault.json in the bucket names another recipient than this Mac's keys");
    }

    // The slot first: an invite without it would name a vault that the person cannot open.
    let key = format!("{slots}{}.age", args.person);
    bucket
        .put(
            &key,
            keys::seal_slot(&identity, recipient)?,
            "application/octet-stream",
        )
        .await?;
    if granted.is_empty() {
        eprintln!(
            "layr-desk: {} now shows only to logins with a person slot; tagged devices and other \
             members no longer see it",
            args.project
        );
    }
    let invite = person::seal_invite(recipient, &Pin::of(&args.project, &vault))?;
    let id = Site::new(site)?
        .drop_invite(&args.person, &invite)
        .await
        .with_context(|| {
            format!(
                "the person slot is in place, but the invite did not reach {}; run grant again \
                 after the person runs layr-desk person keys --site {site}",
                args.person
            )
        })?;
    println!(
        "{}",
        json!({"project": args.project, "person": args.person, "slot": key, "invite": id,
               "site": site})
    );
    Ok(())
}
