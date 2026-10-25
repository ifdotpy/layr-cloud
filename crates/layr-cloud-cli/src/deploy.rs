use crate::{
    account::{failure, Account, Parameters},
    credentials, Args,
};
use anyhow::{bail, Context, Result};
use layr_store::aws;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};

const BOOTSTRAP: &str = include_str!("../../../infra/aws/bootstrap.yaml");
const STORE: &str = include_str!("../../../infra/aws/template.yaml");
const DNS: &str = include_str!("../../../infra/aws/dns.yaml");
const GATEWAY: &str = include_str!("../../../infra/aws/gateway.yaml");
const EXECUTOR: &str = include_str!("../../../infra/aws/executor.yaml");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    image_id: String,
    gateway_host: String,
    domain: Option<String>,
    zone_id: Option<String>,
    gateway_auth_key_parameter: Option<String>,
    executor_auth_key_parameter: Option<String>,
}

impl Config {
    fn validate(&self) -> Result<()> {
        let image = self
            .image_id
            .strip_prefix("ami-")
            .context("imageId must be an AMI ID")?;
        if !matches!(image.len(), 8 | 17) || !image.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("imageId must be an AMI ID");
        }
        for host in std::iter::once(&self.gateway_host).chain(self.domain.iter()) {
            if host.len() > 253
                || !host.contains('.')
                || !host.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && label.as_bytes()[0].is_ascii_alphanumeric()
                        && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
            {
                bail!(
                    "gatewayHost and domain must be lowercase DNS names without a scheme or path"
                );
            }
        }
        if let Some(zone) = &self.zone_id {
            if self.domain.is_none() {
                bail!("zoneId requires domain");
            }
            if !zone.starts_with('Z')
                || !(2..=33).contains(&zone.len())
                || !zone
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                bail!("zoneId must be a Route 53 zone ID");
            }
        }
        for name in self
            .gateway_auth_key_parameter
            .iter()
            .chain(self.executor_auth_key_parameter.iter())
        {
            if !name.starts_with('/')
                || name.len() < 2
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
            {
                bail!("AuthKeyParameter must be an absolute SSM parameter name");
            }
        }
        Ok(())
    }
}

struct Release {
    kind: &'static str,
    bytes: Vec<u8>,
    digest: String,
}
impl Release {
    fn read(args: &Args, kind: &'static str) -> Result<Self> {
        let path = args.release.join(format!("{kind}.tgz"));
        let metadata =
            std::fs::metadata(&path).with_context(|| format!("read {}", path.display()))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 5_000_000_000 {
            bail!(
                "{} must be a nonempty archive of at most 5 GB",
                path.display()
            );
        }
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let digest = hex::encode(Sha256::digest(&bytes));
        Ok(Self {
            kind,
            bytes,
            digest,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StackPlan {
    name: String,
    template: String,
    parameters: Parameters,
}

struct Deployment {
    account: Option<Account>,
    prefix: String,
    stacks: Vec<StackPlan>,
    uploads: Vec<Value>,
    results: BTreeMap<String, Parameters>,
}
impl Deployment {
    fn stack(
        &mut self,
        kind: &str,
        template: &str,
        body: &str,
        parameters: Parameters,
        output_names: &[&str],
    ) -> Result<Parameters> {
        let name = format!("{}-{kind}", self.prefix);
        let outputs = match &self.account {
            Some(account) => account.deploy_stack(&name, body, &parameters)?,
            None => output_names
                .iter()
                .map(|key| ((*key).into(), format!("<{name}.{key}>")))
                .collect(),
        };
        self.stacks.push(StackPlan {
            name: name.clone(),
            template: format!("infra/aws/{template}"),
            parameters,
        });
        self.results.insert(name, outputs.clone());
        Ok(outputs)
    }

    fn upload(
        &mut self,
        bucket: &str,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
        detail: Value,
    ) -> Result<()> {
        if let Some(account) = &self.account {
            account.put(bucket, key, bytes, content_type)?;
        }
        self.uploads.push(json!({"afterStack": format!("{}-bootstrap", self.prefix), "bucket": bucket, "key": key, "detail": detail}));
        Ok(())
    }
}

fn value(outputs: &Parameters, name: &str) -> Result<String> {
    outputs
        .get(name)
        .filter(|s| !s.is_empty())
        .cloned()
        .with_context(|| format!("stack has no output {name}"))
}
fn parameters<const N: usize>(items: [(&str, String); N]) -> Parameters {
    items.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

pub fn run(args: Args) -> Result<()> {
    let path = args.release.join("deploy.json");
    let config: Config = serde_json::from_slice(
        &std::fs::read(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .context("invalid deploy.json")?;
    config.validate()?;
    if let Some(domain) = config.domain.as_ref().filter(|_| config.zone_id.is_none()) {
        eprintln!("creates a new Route 53 hosted zone {domain}; delegate it from your registrar");
    }
    // Read both archives before the first AWS write.
    let releases = [
        Release::read(&args, "gateway")?,
        Release::read(&args, "executor")?,
    ];
    let account = if args.dry_run {
        None
    } else {
        Some(Account::new(
            credentials::load(args.profile.as_deref())?,
            &args.region,
        )?)
    };
    let (account_id, partition) = match &account {
        None => ("<STS.Account>".into(), "<STS.Partition>".into()),
        Some(account) => {
            let (status, reply) = account.query(
                "sts",
                "2011-06-15",
                &[("Action", "GetCallerIdentity")],
                Duration::from_secs(60),
            )?;
            if status != 200 {
                return Err(failure("GetCallerIdentity", status, &reply));
            }
            let id = aws::xml_text(&reply, "Account")
                .context("GetCallerIdentity returned no account")?;
            let arn = aws::xml_text(&reply, "Arn").context("GetCallerIdentity returned no ARN")?;
            let partition = arn
                .split(':')
                .nth(1)
                .filter(|s| !s.is_empty())
                .context("GetCallerIdentity returned an invalid ARN")?
                .to_string();
            if id.len() != 12 || !id.bytes().all(|b| b.is_ascii_digit()) {
                bail!("GetCallerIdentity returned an invalid account");
            }
            (id, partition)
        }
    };
    let mut deployment = Deployment {
        account,
        prefix: args.prefix.clone(),
        stacks: Vec::new(),
        uploads: Vec::new(),
        results: BTreeMap::new(),
    };
    let gateway_role = format!("{}-gateway", args.prefix);
    let executor_role = format!("{}-executor", args.prefix);
    let role_arn = |name: &str| format!("arn:{partition}:iam::{account_id}:role/{name}");
    let bootstrap = deployment.stack(
        "bootstrap",
        "bootstrap.yaml",
        BOOTSTRAP,
        Parameters::new(),
        &["ArtifactsBucket"],
    )?;
    let artifacts = value(&bootstrap, "ArtifactsBucket")?;
    for release in releases {
        let key = format!("{}/releases/{}.tgz", release.kind, release.digest);
        deployment.upload(
            &artifacts,
            &key,
            release.bytes,
            "application/gzip",
            json!({"source": format!("{}.tgz", release.kind), "sha256": release.digest}),
        )?;
        let descriptor = json!({"key": key, "sha256": release.digest});
        deployment.upload(
            &artifacts,
            &format!("{}/release.json", release.kind),
            serde_json::to_vec(&descriptor)?,
            "application/json",
            descriptor,
        )?;
    }
    let store = deployment.stack(
        "store",
        "template.yaml",
        STORE,
        parameters([
            ("Prefix", args.prefix.clone()),
            (
                "GatewayRoleArns",
                format!("{},{}", role_arn(&gateway_role), role_arn(&executor_role)),
            ),
        ]),
        &[
            "BucketName",
            "StoreUrl",
            "ReaderRoleArn",
            "WriterRoleArn",
            "GcRoleArn",
        ],
    )?;
    let reader = value(&store, "ReaderRoleArn")?;
    let writer = value(&store, "WriterRoleArn")?;
    let gc = value(&store, "GcRoleArn")?;
    let (host, zone) = match &config.domain {
        None => (String::new(), String::new()),
        Some(domain) => {
            let zone = match &config.zone_id {
                Some(zone) => zone.clone(),
                None => value(
                    &deployment.stack(
                        "dns",
                        "dns.yaml",
                        DNS,
                        parameters([("Domain", domain.clone())]),
                        &["ZoneId", "NameServers"],
                    )?,
                    "ZoneId",
                )?,
            };
            (domain.clone(), zone)
        }
    };
    let gateway = deployment.stack(
        "gateway",
        "gateway.yaml",
        GATEWAY,
        parameters([
            ("ArtifactsBucket", artifacts.clone()),
            ("ImageId", config.image_id.clone()),
            ("RoleName", gateway_role.clone()),
            ("TailnetHostname", gateway_role),
            ("StoreRoleArns", format!("{reader},{writer},{gc}")),
            ("Host", host),
            ("ZoneId", zone),
        ]),
        &["RoleArn", "InstanceId", "SubnetId", "VpcId", "RouteTableId"],
    )?;
    let subnet = value(&gateway, "SubnetId")?;
    let availability_zone = match &deployment.account {
        None => format!("<EC2.DescribeSubnets({subnet}).AvailabilityZone>"),
        Some(account) => {
            let (status, reply) = account.query(
                "ec2",
                "2016-11-15",
                &[("Action", "DescribeSubnets"), ("SubnetId.1", &subnet)],
                Duration::from_secs(60),
            )?;
            if status != 200 {
                return Err(failure("DescribeSubnets", status, &reply));
            }
            aws::xml_text(&reply, "availabilityZone")
                .filter(|s| !s.is_empty())
                .context("DescribeSubnets returned no availability zone")?
        }
    };
    deployment.stack(
        "executor",
        "executor.yaml",
        EXECUTOR,
        parameters([
            ("ArtifactsBucket", artifacts.clone()),
            ("ImageId", config.image_id),
            ("VpcId", value(&gateway, "VpcId")?),
            ("SubnetId", subnet),
            ("AvailabilityZone", availability_zone),
            ("RoleName", executor_role.clone()),
            ("TailnetHostname", executor_role),
            ("ReaderRoleArn", reader),
            ("WriterRoleArn", writer),
            ("InstanceType", "t4g.small".into()),
            ("DataVolumeSize", "100".into()),
        ]),
        &["InstanceId", "RoleArn", "DataVolumeId"],
    )?;
    let outputs = json!({"gatewayUrl": format!("https://{}", config.domain.unwrap_or(config.gateway_host)), "artifactsBucket": artifacts, "storeBucket": value(&store, "BucketName")?, "storeUrl": value(&store, "StoreUrl")?});
    let result = if args.dry_run {
        json!({"dryRun": true, "region": args.region, "profile": args.profile.or_else(|| std::env::var("AWS_PROFILE").ok()).unwrap_or_else(|| "default".into()), "lookups": ["STS.GetCallerIdentity", "EC2.DescribeSubnets"], "stacks": deployment.stacks, "uploads": deployment.uploads, "outputs": outputs})
    } else {
        json!({"region": args.region, "stacks": deployment.results, "outputs": outputs})
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
