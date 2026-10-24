use anyhow::{Context, Result};
use layr_store::aws::{profile::Profile, Credentials};
use reqwest::blocking::Client;

pub enum Source {
    Environment(Credentials),
    Profile(Profile),
}

impl From<Credentials> for Source {
    fn from(credentials: Credentials) -> Self {
        Self::Environment(credentials)
    }
}

impl Source {
    pub fn credentials(&self, http: &Client) -> Result<Credentials> {
        match self {
            Self::Environment(credentials) => Ok(credentials.clone()),
            Self::Profile(profile) => profile.credentials(http),
        }
    }
}

pub fn load(profile: Option<&str>) -> Result<Source> {
    let vars: std::collections::HashMap<String, String> = std::env::vars().collect();
    if vars.contains_key("AWS_ACCESS_KEY_ID") || vars.contains_key("AWS_SECRET_ACCESS_KEY") {
        let value = |key: &str| vars.get(key).filter(|value| !value.is_empty()).cloned();
        return Ok(Source::Environment(Credentials {
            key_id: value("AWS_ACCESS_KEY_ID").context("AWS access key ID is missing")?,
            secret: value("AWS_SECRET_ACCESS_KEY").context("AWS secret access key is missing")?,
            token: value("AWS_SESSION_TOKEN"),
        }));
    }
    let name = profile
        .or_else(|| vars.get("AWS_PROFILE").map(String::as_str))
        .unwrap_or("default");
    Ok(Source::Profile(Profile::load_with_env(name, &vars)?))
}
