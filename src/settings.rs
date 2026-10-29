//! Service endpoints, kept out of the source so a public build contains no
//! internal hostnames. Resolution order:
//!
//! 1. environment variables
//! 2. `/etc/redhat-ai-configurator/endpoints.conf`
//! 3. the placeholder defaults below
//!
//! The defaults are deliberately examples, so a fresh install cannot silently
//! talk to the wrong service.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

pub const CONFIG_PATH: &str = "/etc/redhat-ai-configurator/endpoints.conf";

pub const KEYS: [&str; 8] = [
    "RHAI_MAAS_BASE_URL",
    "RHAI_MAAS_KEY_PAGE",
    "RHAI_MAAS_HELP_URL",
    "ENMAAS_BASE_URL",
    "ENMAAS_KEY_PAGE",
    "ENMAAS_HELP_URL",
    "ENMAAS_ELIGIBILITY_URL",
    "ENMAAS_SLACK",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub rhai_maas_base: String,
    pub rhai_maas_key_page: String,
    pub rhai_maas_help: String,
    pub enmaas_base: String,
    pub enmaas_key_page: String,
    pub enmaas_help: String,
    pub enmaas_eligibility: String,
    pub enmaas_slack: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            rhai_maas_base: "https://maas.example.com/v1".into(),
            rhai_maas_key_page: "https://maas.example.com/keys".into(),
            rhai_maas_help: "https://maas.example.com/docs".into(),
            enmaas_base: "https://enmaas.example.com".into(),
            enmaas_key_page: "https://enmaas.example.com/keys".into(),
            enmaas_help: "https://enmaas.example.com/docs".into(),
            enmaas_eligibility: String::new(),
            enmaas_slack: String::new(),
        }
    }
}

impl Endpoints {
    /// Build the effective endpoints. Environment wins over the config file.
    pub fn resolve(file: &BTreeMap<String, String>, env: &BTreeMap<String, String>) -> Self {
        let mut endpoints = Self::default();
        let value = |key: &str| {
            env.get(key)
                .or_else(|| file.get(key))
                .map(|raw| raw.trim().to_string())
                .filter(|raw| !raw.is_empty())
        };
        if let Some(v) = value("RHAI_MAAS_BASE_URL") {
            endpoints.rhai_maas_base = v;
        }
        if let Some(v) = value("RHAI_MAAS_KEY_PAGE") {
            endpoints.rhai_maas_key_page = v;
        }
        if let Some(v) = value("RHAI_MAAS_HELP_URL") {
            endpoints.rhai_maas_help = v;
        }
        if let Some(v) = value("ENMAAS_BASE_URL") {
            endpoints.enmaas_base = v;
        }
        if let Some(v) = value("ENMAAS_KEY_PAGE") {
            endpoints.enmaas_key_page = v;
        }
        if let Some(v) = value("ENMAAS_HELP_URL") {
            endpoints.enmaas_help = v;
        }
        if let Some(v) = value("ENMAAS_ELIGIBILITY_URL") {
            endpoints.enmaas_eligibility = v;
        }
        if let Some(v) = value("ENMAAS_SLACK") {
            endpoints.enmaas_slack = v;
        }
        endpoints
    }

    pub fn provider_base(&self, provider: &str) -> &str {
        match provider {
            "rhai-maas" => &self.rhai_maas_base,
            _ => &self.enmaas_base,
        }
    }

    pub fn key_page(&self, provider: &str) -> &str {
        match provider {
            "rhai-maas" => &self.rhai_maas_key_page,
            _ => &self.enmaas_key_page,
        }
    }

    pub fn help(&self, provider: &str) -> &str {
        match provider {
            "rhai-maas" => &self.rhai_maas_help,
            _ => &self.enmaas_help,
        }
    }

    pub fn eligibility(&self, provider: &str) -> Option<&str> {
        (provider == "enmaas" && !self.enmaas_eligibility.is_empty())
            .then_some(self.enmaas_eligibility.as_str())
    }

    pub fn slack(&self, provider: &str) -> Option<&str> {
        (provider == "enmaas" && !self.enmaas_slack.is_empty())
            .then_some(self.enmaas_slack.as_str())
    }

    pub fn uses_placeholders(&self) -> bool {
        self.rhai_maas_base.contains("example.com") || self.enmaas_base.contains("example.com")
    }
}

fn parse_config(text: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if KEYS.contains(&key) {
                values.insert(key.to_string(), value.trim().to_string());
            }
        }
    }
    values
}

fn read_config(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .map(|text| parse_config(&text))
        .unwrap_or_default()
}

fn env_values() -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for key in KEYS {
        if let Ok(value) = std::env::var(key) {
            values.insert(key.to_string(), value);
        }
    }
    values
}

static CACHE: OnceLock<Endpoints> = OnceLock::new();

pub fn endpoints() -> &'static Endpoints {
    CACHE.get_or_init(|| {
        Endpoints::resolve(&read_config(Path::new(CONFIG_PATH)), &env_values())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn defaults_are_placeholders() {
        let endpoints = Endpoints::resolve(&BTreeMap::new(), &BTreeMap::new());
        assert!(endpoints.uses_placeholders());
        assert_eq!(endpoints.provider_base("rhai-maas"), "https://maas.example.com/v1");
        assert_eq!(endpoints.provider_base("enmaas"), "https://enmaas.example.com");
        assert_eq!(endpoints.eligibility("enmaas"), None);
    }

    #[test]
    fn environment_overrides_the_file() {
        let file = map(&[("RHAI_MAAS_BASE_URL", "https://file.example/v1")]);
        let env = map(&[
            ("RHAI_MAAS_BASE_URL", "https://env.example/v1"),
            ("ENMAAS_SLACK", "#help"),
        ]);
        let endpoints = Endpoints::resolve(&file, &env);
        assert_eq!(endpoints.rhai_maas_base, "https://env.example/v1");
        assert_eq!(endpoints.slack("enmaas"), Some("#help"));
    }

    #[test]
    fn blank_values_are_ignored() {
        let env = map(&[("ENMAAS_BASE_URL", "   ")]);
        let endpoints = Endpoints::resolve(&BTreeMap::new(), &env);
        assert_eq!(endpoints.enmaas_base, "https://enmaas.example.com");
    }

    #[test]
    fn config_parser_ignores_comments_and_unknown_keys() {
        let values = parse_config(
            "# comment\nRHAI_MAAS_BASE_URL=https://example.test/v1\nOTHER=1\nbad line\n",
        );
        assert_eq!(values.len(), 1);
        assert_eq!(
            values.get("RHAI_MAAS_BASE_URL").map(String::as_str),
            Some("https://example.test/v1")
        );
    }

    #[test]
    fn placeholders_clear_once_a_real_base_is_configured() {
        let env = map(&[
            ("RHAI_MAAS_BASE_URL", "https://maas.corp.test/v1"),
            ("ENMAAS_BASE_URL", "https://enmaas.corp.test"),
        ]);
        assert!(!Endpoints::resolve(&BTreeMap::new(), &env).uses_placeholders());
    }
}
