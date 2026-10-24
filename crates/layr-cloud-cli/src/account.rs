//! CloudFormation Query API, ported from Desk setup.rs at 31610ce.
use anyhow::{anyhow, bail, Context, Result};
use layr_store::{
    aws::{self, Credentials},
    s3::S3,
};
use reqwest::{blocking::Client, Method};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub type Parameters = BTreeMap<String, String>;

pub struct Stack {
    pub status: String,
    pub reason: String,
    pub outputs: Parameters,
}

pub struct Account {
    http: Client,
    credentials: Credentials,
    region: String,
    pub wait: Duration,
    pub poll: Duration,
}

impl Account {
    pub fn new(credentials: Credentials, region: &str) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .user_agent("layr-cloud")
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(30))
                .build()?,
            credentials,
            region: region.into(),
            wait: Duration::from_secs(3600),
            poll: Duration::from_secs(5),
        })
    }

    pub fn query(
        &self,
        service: &str,
        version: &str,
        params: &[(&str, &str)],
        limit: Duration,
    ) -> Result<(u16, String)> {
        crate::query::send(
            &self.http,
            &self.credentials,
            &self.region,
            service,
            version,
            params,
            limit,
        )
    }

    fn cloudformation(&self, params: &[(&str, &str)], limit: Duration) -> Result<(u16, String)> {
        self.query("cloudformation", "2010-05-15", params, limit)
    }

    pub fn describe(&self, name: &str, limit: Duration) -> Result<Option<Stack>> {
        let (status, reply) =
            self.cloudformation(&[("Action", "DescribeStacks"), ("StackName", name)], limit)?;
        if status == 400
            && aws::xml_text(&reply, "Code").as_deref() == Some("ValidationError")
            && aws::xml_text(&reply, "Message").is_some_and(|s| s.contains("does not exist"))
        {
            return Ok(None);
        }
        if status != 200 {
            return Err(failure(&format!("DescribeStacks {name}"), status, &reply));
        }
        let mut outputs = Parameters::new();
        if let Some(section) = elements(&reply, "Outputs").next() {
            for member in elements(section, "member") {
                if let (Some(key), Some(value)) = (
                    aws::xml_text(member, "OutputKey"),
                    aws::xml_text(member, "OutputValue"),
                ) {
                    outputs.insert(key, value);
                }
            }
        }
        Ok(Some(Stack {
            status: aws::xml_text(&reply, "StackStatus")
                .context("DescribeStacks returned no status")?,
            reason: aws::xml_text(&reply, "StackStatusReason").unwrap_or_default(),
            outputs,
        }))
    }

    fn events(&self, name: &str) -> String {
        match self.cloudformation(
            &[("Action", "DescribeStackEvents"), ("StackName", name)],
            Duration::from_secs(30),
        ) {
            Ok((200, reply)) => {
                let mut events = Vec::new();
                // Preserve member boundaries. Some stack events have no reason.
                for member in elements(&reply, "member") {
                    let status = aws::xml_text(member, "ResourceStatus").unwrap_or_default();
                    if status.contains("FAILED") {
                        events.push(format!(
                            "{}: {status}: {}",
                            aws::xml_text(member, "LogicalResourceId").unwrap_or_default(),
                            aws::xml_text(member, "ResourceStatusReason").unwrap_or_default()
                        ));
                    }
                }
                if events.is_empty() {
                    format!("DescribeStackEvents {name}: no failed resource events returned")
                } else {
                    events.join("\n")
                }
            }
            Ok((status, reply)) => failure("DescribeStackEvents", status, &reply).to_string(),
            Err(error) => format!("DescribeStackEvents {name}: {error}"),
        }
    }

    fn settle(&self, name: &str, deadline: Instant, operation: &str) -> Result<Parameters> {
        let mut shown = String::new();
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| {
                    anyhow!(
                        "stack {name}: wait timeout after {} seconds",
                        self.wait.as_secs_f64()
                    )
                })?;
            let stack = match self.describe(name, remaining.min(Duration::from_secs(60)))? {
                Some(stack) => stack,
                None => bail!(
                    "stack {name} disappeared after {operation}\n{}",
                    self.events(name)
                ),
            };
            if shown != stack.status {
                eprintln!("layr-cloud: {name}: {}", stack.status);
                shown = stack.status.clone();
            }
            match stack.status.as_str() {
                "CREATE_COMPLETE" | "UPDATE_COMPLETE" => return Ok(stack.outputs),
                status if status.ends_with("_IN_PROGRESS") => {}
                status => bail!(
                    "stack {name}: {status}: {}\n{}",
                    stack.reason,
                    self.events(name)
                ),
            }
            std::thread::sleep(
                self.poll
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    pub fn deploy_stack(
        &self,
        name: &str,
        template: &str,
        parameters: &Parameters,
    ) -> Result<Parameters> {
        let deadline = Instant::now() + self.wait;
        let existing = self.describe(name, self.wait.min(Duration::from_secs(60)))?;
        if existing
            .as_ref()
            .is_some_and(|s| s.status.ends_with("_IN_PROGRESS"))
        {
            self.settle(name, deadline, "an earlier operation")?;
        }
        // Re-read after an earlier operation, which may have removed the stack.
        let existing = if existing
            .as_ref()
            .is_some_and(|s| s.status.ends_with("_IN_PROGRESS"))
        {
            self.describe(
                name,
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(60)),
            )?
        } else {
            existing
        };
        let action = if existing.is_some() {
            "UpdateStack"
        } else {
            "CreateStack"
        };
        let keys: Vec<(String, String)> = parameters
            .iter()
            .enumerate()
            .flat_map(|(i, (key, value))| {
                [
                    (
                        format!("Parameters.member.{}.ParameterKey", i + 1),
                        key.clone(),
                    ),
                    (
                        format!("Parameters.member.{}.ParameterValue", i + 1),
                        value.clone(),
                    ),
                ]
            })
            .collect();
        let mut params = vec![
            ("Action", action),
            ("StackName", name),
            ("TemplateBody", template),
            ("Capabilities.member.1", "CAPABILITY_NAMED_IAM"),
        ];
        params.extend(
            keys.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
        // Keep a failed create so that its resource events remain available.
        if existing.is_none() {
            params.push(("OnFailure", "ROLLBACK"));
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .context("stack wait timeout before mutation")?;
        let (status, reply) =
            self.cloudformation(&params, remaining.min(Duration::from_secs(60)))?;
        if action == "UpdateStack"
            && status == 400
            && aws::xml_text(&reply, "Code").as_deref() == Some("ValidationError")
            && aws::xml_text(&reply, "Message")
                .is_some_and(|s| s.contains("No updates are to be performed"))
        {
            return Ok(self
                .describe(name, remaining.min(Duration::from_secs(60)))?
                .context("stack disappeared after UpdateStack")?
                .outputs);
        }
        if status != 200 {
            return Err(failure(&format!("{action} {name}"), status, &reply));
        }
        // The stack ID remains usable for events if AWS deletes the stack later.
        let id = aws::xml_text(&reply, "StackId").unwrap_or_else(|| name.into());
        self.settle(&id, deadline, action)
    }

    pub fn put(&self, bucket: &str, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        let endpoint = std::env::var("AWS_ENDPOINT_URL_S3")
            .ok()
            .filter(|s| !s.is_empty());
        let s3 = S3::new_with_client(
            bucket,
            &self.region,
            self.credentials.clone(),
            endpoint.as_deref(),
            self.http.clone(),
        )?;
        let limit = Duration::from_secs(120 + bytes.len() as u64 / 100_000);
        let response = s3.request(
            Method::PUT,
            key,
            &[],
            &[("content-type", content_type)],
            bytes,
            limit,
        )?;
        if !response.status().is_success() {
            bail!("S3 PUT {key}: HTTP {}", response.status());
        }
        Ok(())
    }
}

pub fn failure(what: &str, status: u16, reply: &str) -> anyhow::Error {
    anyhow!(
        "{what}: HTTP {status} {} {}",
        aws::xml_text(reply, "Code").unwrap_or_default(),
        aws::xml_text(reply, "Message").unwrap_or_default()
    )
}

// Leave containers encoded. Decode only leaf text through layr-store.
fn elements<'a>(xml: &'a str, tag: &str) -> impl Iterator<Item = &'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut rest = xml;
    std::iter::from_fn(move || {
        let start = rest.find(&open)? + open.len();
        let end = start + rest[start..].find(&close)?;
        let value = &rest[start..end];
        rest = &rest[end + close.len()..];
        Some(value)
    })
}
