//! Key validation, model discovery and reachability. All network calls are
//! blocking and are expected to run on a worker thread in the TUI.

use std::time::Duration;

use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(15);
const REACH_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthStyle {
    Bearer,
    XApiKey,
}

impl AuthStyle {
    pub fn header(self, key: &str) -> (&'static str, String) {
        match self {
            AuthStyle::Bearer => ("Authorization", format!("Bearer {key}")),
            AuthStyle::XApiKey => ("x-api-key", key.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validation {
    pub ok: bool,
    pub status: Option<u16>,
    pub auth: Option<AuthStyle>,
    pub models: Vec<String>,
    pub message: String,
}

impl Validation {
    fn fail(status: Option<u16>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            status,
            auth: None,
            models: Vec::new(),
            message: message.into(),
        }
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

fn request_models(base: &str, key: &str, style: AuthStyle) -> Result<(u16, String), String> {
    let url = format!("{}/models", base.trim_end_matches('/'));
    let (header_name, header_value) = style.header(key);
    let mut request = agent(TIMEOUT)
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "redhat-ai-configurator")
        .header(header_name, header_value);
    if style == AuthStyle::XApiKey {
        request = request.header("anthropic-version", "2023-06-01");
    }

    match request.call() {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let body = response
                .body_mut()
                .read_to_string()
                .unwrap_or_default();
            Ok((status, body))
        }
        Err(error) => Err(error.to_string()),
    }
}

/// Validate a key against `{base}/models`, retrying with the other auth
/// header when the first is rejected.
pub fn validate(base: &str, raw_key: &str) -> Validation {
    let key: String = raw_key.chars().filter(|c| !c.is_whitespace()).collect();
    if key.is_empty() {
        return Validation::fail(None, "Paste your API key first.");
    }

    let mut last_status = None;
    for style in [AuthStyle::Bearer, AuthStyle::XApiKey] {
        match request_models(base, &key, style) {
            Ok((status, body)) if (200..300).contains(&status) => {
                let models = parse_models(&body);
                let count = models.len();
                return Validation {
                    ok: true,
                    status: Some(status),
                    auth: Some(style),
                    message: if count == 0 {
                        "Key accepted.".to_string()
                    } else {
                        format!(
                            "Key accepted. {count} model{} available.",
                            if count == 1 { "" } else { "s" }
                        )
                    },
                    models,
                };
            }
            Ok((status, _)) => {
                last_status = Some(status);
                if status != 401 {
                    break;
                }
            }
            Err(error) => {
                return Validation::fail(
                    None,
                    format!(
                        "Could not reach the service ({error}). Check your network and the VPN requirement, then try again."
                    ),
                );
            }
        }
    }

    let message = match last_status {
        Some(401) => "That key was rejected. Check that you copied the whole key, or create a new one.".to_string(),
        Some(403) => "This key has no access to this service. Check your entitlement or create a new key.".to_string(),
        Some(404) => "The service did not recognise this endpoint. Check the VPN requirement for this step.".to_string(),
        Some(status) => format!("The service answered with HTTP {status}. Try again, or create a new key."),
        None => "The key could not be validated.".to_string(),
    };
    Validation::fail(last_status, message)
}

fn parse_models(body: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for key in ["data", "models"] {
        if let Some(Value::Array(items)) = value.get(key) {
            for item in items {
                let name = item
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("name").and_then(Value::as_str));
                if let Some(name) = name {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Reachability probe: any non-5xx answer counts as reachable.
pub fn reachable(url: &str) -> bool {
    match agent(REACH_TIMEOUT)
        .get(url)
        .header("Accept", "application/json,text/html")
        .header("User-Agent", "redhat-ai-configurator")
        .call()
    {
        Ok(response) => response.status().as_u16() < 500,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_and_anthropic_lists() {
        assert_eq!(
            parse_models(r#"{"data":[{"id":"b"},{"id":"a"}]}"#),
            vec!["a", "b"]
        );
        assert_eq!(
            parse_models(r#"{"models":[{"name":"m2"},{"name":"m1"}]}"#),
            vec!["m1", "m2"]
        );
        assert!(parse_models("not json").is_empty());
    }

    #[test]
    fn empty_key_fails_without_network() {
        let result = validate("http://127.0.0.1:1/v1", "   ");
        assert!(!result.ok);
        assert!(result.status.is_none());
    }

    fn serve(responses: Vec<(bool, &'static str)>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for (ok, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_ascii_lowercase();
                let use_x_api_key = ok && !request.contains("authorization:");
                let status = if ok { "200 OK" } else { "401 Unauthorized" };
                let payload = if ok {
                    body
                } else {
                    r#"{"error":"unauthorized"}"#
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = use_x_api_key;
            }
        });
        (format!("http://{address}/v1"), handle)
    }

    #[test]
    fn accepts_a_bearer_key() {
        let (base, server) = serve(vec![(true, r#"{"data":[{"id":"m1"},{"id":"m2"}]}"#)]);
        let result = validate(&base, "secret");
        server.join().unwrap();
        assert!(result.ok);
        assert_eq!(result.auth, Some(AuthStyle::Bearer));
        assert_eq!(result.models, vec!["m1", "m2"]);
    }

    #[test]
    fn retries_with_x_api_key_after_401() {
        let (base, server) = serve(vec![
            (false, ""),
            (true, r#"{"models":[{"name":"claude"}]}"#),
        ]);
        let result = validate(&base, "secret");
        server.join().unwrap();
        assert!(result.ok);
        assert_eq!(result.auth, Some(AuthStyle::XApiKey));
        assert_eq!(result.models, vec!["claude"]);
    }

    #[test]
    fn surfaces_a_rejected_key() {
        let (base, server) = serve(vec![(false, ""), (false, "")]);
        let result = validate(&base, "secret");
        server.join().unwrap();
        assert!(!result.ok);
        assert_eq!(result.status, Some(401));
        assert!(result.message.contains("rejected"));
    }
}
