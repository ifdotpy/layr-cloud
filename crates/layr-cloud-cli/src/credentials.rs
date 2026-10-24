use anyhow::{bail, Context, Result};
use layr_store::aws::Credentials;
use std::{collections::BTreeMap, path::PathBuf};

type Values = BTreeMap<String, String>;

#[derive(Debug)]
pub struct SsoProfile;
impl std::fmt::Display for SsoProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SSO profiles need layr remote login first")
    }
}
impl std::error::Error for SsoProfile {}

fn section(text: &str, wanted: &str) -> Values {
    let mut selected = false;
    let mut values = Values::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            selected = name.trim() == wanted;
        } else if selected {
            if let Some((key, value)) = line.split_once('=') {
                values.insert(key.trim().to_ascii_lowercase(), value.trim().into());
            }
        }
    }
    values
}

fn read(path: PathBuf) -> Result<String> {
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

pub fn load(profile: Option<&str>) -> Result<Credentials> {
    let vars: Values = std::env::vars().collect();
    if vars.contains_key("AWS_ACCESS_KEY_ID") || vars.contains_key("AWS_SECRET_ACCESS_KEY") {
        return keys(
            &vars,
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
        );
    }
    let profile = profile
        .or_else(|| vars.get("AWS_PROFILE").map(String::as_str))
        .unwrap_or("default");
    let home = vars.get("HOME").map(PathBuf::from);
    let path = |variable: &str, filename: &str| -> Result<PathBuf> {
        vars.get(variable)
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|p| p.join(".aws").join(filename)))
            .context("HOME is not set")
    };
    let config = read(path("AWS_CONFIG_FILE", "config")?)?;
    let section_name = if profile == "default" {
        "default".into()
    } else {
        format!("profile {profile}")
    };
    let mut values = section(&config, &section_name);
    values.extend(section(
        &read(path("AWS_SHARED_CREDENTIALS_FILE", "credentials")?)?,
        profile,
    ));
    from_profile(values).with_context(|| format!("AWS profile {profile}"))
}

fn from_profile(values: Values) -> Result<Credentials> {
    if values.contains_key("sso_session") || values.contains_key("sso_start_url") {
        return Err(SsoProfile.into());
    }
    if values.contains_key("credential_process") {
        bail!("credential_process is not supported");
    }
    if values.contains_key("role_arn")
        || values.contains_key("source_profile")
        || values.contains_key("credential_source")
    {
        bail!("assume-role profiles are not supported; use environment credentials or a static-key profile");
    }
    keys(
        &values,
        "aws_access_key_id",
        "aws_secret_access_key",
        "aws_session_token",
    )
}

fn keys(values: &Values, key: &str, secret: &str, token: &str) -> Result<Credentials> {
    let value = |name: &str| values.get(name).filter(|s| !s.is_empty()).cloned();
    Ok(Credentials {
        key_id: value(key).context("AWS access key ID is missing")?,
        secret: value(secret).context("AWS secret access key is missing")?,
        token: value(token),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ini_sections_do_not_cross_profile_boundaries() {
        let values = section("# test\n[profile other]\nsso_session=x\n[profile p]\n region = r\naws_access_key_id = example\n[default]\nregion = wrong", "profile p");
        assert_eq!(values.len(), 2);
        assert_eq!(values["region"], "r");
        assert!(!values.contains_key("sso_session"));
    }
    #[test]
    fn static_profile_returns_keys_and_session_token() {
        let values = section("[p]\naws_access_key_id=fake-id\naws_secret_access_key=fake-secret\naws_session_token=fake-session", "p");
        let credentials = from_profile(values).unwrap();
        assert_eq!(credentials.key_id, "fake-id");
        assert_eq!(credentials.secret, "fake-secret");
        assert_eq!(credentials.token.as_deref(), Some("fake-session"));
    }
    #[test]
    fn both_sso_profile_formats_require_login() {
        for key in ["sso_session", "sso_start_url"] {
            let error = from_profile(Values::from([(key.into(), "fake".into())])).unwrap_err();
            assert!(error.downcast_ref::<SsoProfile>().is_some());
            assert_eq!(
                error.to_string(),
                "SSO profiles need layr remote login first"
            );
        }
    }
    #[test]
    fn credential_process_is_refused() {
        let error = from_profile(Values::from([(
            "credential_process".into(),
            "false".into(),
        )]))
        .unwrap_err();
        assert_eq!(error.to_string(), "credential_process is not supported");
    }
    #[test]
    fn partial_credentials_fail_without_printing_values() {
        let values = Values::from([("id".into(), "private-value".into())]);
        assert!(!keys(&values, "id", "secret", "token")
            .unwrap_err()
            .to_string()
            .contains("private-value"));
    }
}
