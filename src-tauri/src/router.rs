use crate::model::{error, Result, Router};
use reqwest::{
    blocking::{Client, RequestBuilder, Response},
    Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    net::{IpAddr, TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

pub fn loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

pub fn base(profile: &Router) -> Result<Url> {
    if !matches!(profile.scheme.as_str(), "http" | "https")
        || profile.port == 0
        || profile.host.is_empty()
        || !profile
            .host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
    {
        return Err("Enter a valid HTTP(S) host and port (1–65535)".into());
    }
    if !loopback(&profile.host) && !profile.allow_remote {
        return Err("Confirm the advanced non-loopback host setting".into());
    }
    let host = if profile.host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{}]", profile.host)
    } else {
        profile.host.clone()
    };
    let url =
        Url::parse(&format!("{}://{}:{}/", profile.scheme, host, profile.port)).map_err(error)?;
    if url.host_str().is_none() || url.port_or_known_default() != Some(profile.port) {
        return Err("Invalid router address".into());
    }
    Ok(url)
}

pub fn endpoint(profile: &Router, path: &str) -> Result<String> {
    if (!path.is_empty() && !path.starts_with('/'))
        || path.starts_with("//")
        || path
            .bytes()
            .any(|b| b.is_ascii_control() || b"?#\\".contains(&b))
    {
        return Err("API base path must start with / and contain no query or fragment".into());
    }
    let mut url = base(profile)?;
    url.set_path(path);
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

pub fn validate(profile: &Router) -> Result<()> {
    base(profile)?;
    if profile.name.trim().is_empty()
        || !matches!(profile.authentication.as_str(), "none" | "bearer")
    {
        return Err("Enter a profile name and valid authentication mode".into());
    }
    if profile.openai_path.is_none() && profile.anthropic_path.is_none() {
        return Err("Enable at least one API protocol".into());
    }
    for path in [&profile.openai_path, &profile.anthropic_path]
        .into_iter()
        .flatten()
    {
        endpoint(profile, path)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProtocol {
    Openai,
    Anthropic,
}

impl ModelProtocol {
    fn resource(self, profile: &Router, resource: &str) -> Result<Url> {
        let path = match self {
            Self::Openai => profile.openai_path.as_deref(),
            Self::Anthropic => profile.anthropic_path.as_deref(),
        }
        .ok_or("Enable the selected API protocol first")?;
        let base = endpoint(profile, path)?;
        let mut url = Url::parse(&base).map_err(error)?;
        let prefix = url.path().trim_end_matches('/');
        let version = if matches!(self, Self::Anthropic) && !prefix.ends_with("/v1") {
            "/v1"
        } else {
            ""
        };
        url.set_path(&format!("{prefix}{version}/{resource}"));
        Ok(url)
    }

    fn request(self, request: RequestBuilder, profile: &Router, token: &str) -> RequestBuilder {
        let request = if profile.authentication == "bearer" && !token.is_empty() {
            request.bearer_auth(token)
        } else {
            request
        };
        match self {
            Self::Anthropic => request.header("anthropic-version", "2023-06-01"),
            Self::Openai => request,
        }
    }
}

fn http_client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(error)
}

fn response_json(response: Response) -> Option<Value> {
    const MAX_BODY: u64 = 512 * 1024;
    let mut bytes = Vec::new();
    response.take(MAX_BODY + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BODY {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn http_failure(status: u16) -> &'static str {
    match status {
        401 | 403 => "Authentication required or rejected",
        404 => "Model or API route not found",
        408 | 504 => "Router timed out",
        429 => "Router rate or quota limit reached",
        500..=599 => "Router or model backend failed",
        300..=399 => "Router requested a redirect",
        _ => "Router rejected the request",
    }
}

#[derive(Serialize)]
pub struct ModelCatalog {
    pub models: Vec<String>,
    pub limits: BTreeMap<String, ModelLimits>,
    pub complete: bool,
    pub http: Option<u16>,
    pub message: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ModelLimits {
    pub context: Option<u32>,
    pub output: Option<u32>,
}

pub fn list_models(profile: &Router, token: &str, protocol: ModelProtocol) -> Result<ModelCatalog> {
    const MAX_MODELS: usize = 1000;
    validate(profile)?;
    let url = protocol.resource(profile, "models")?;
    let client = http_client(Duration::from_secs(5))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut catalog = ModelCatalog {
        models: vec![],
        limits: BTreeMap::new(),
        complete: false,
        http: None,
        message: "Model catalog limit reached",
    };
    let mut ids = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            catalog.message = "Model list request timed out";
            break;
        }
        let mut page = url.clone();
        if let Some(cursor) = &cursor {
            page.query_pairs_mut().append_pair("after_id", cursor);
        }
        let request = client
            .get(page)
            .timeout(remaining.min(Duration::from_secs(5)));
        let response = match protocol.request(request, profile, token).send() {
            Ok(response) => response,
            Err(err) => {
                catalog.http = None;
                catalog.message = if err.is_timeout() {
                    "Model list request timed out"
                } else {
                    "Could not reach the model catalog"
                };
                break;
            }
        };
        let status = response.status();
        catalog.http = Some(status.as_u16());
        if !status.is_success() {
            catalog.message = http_failure(status.as_u16());
            break;
        }
        let Some(body) = response_json(response) else {
            catalog.message = "Unreadable or oversized model list response";
            break;
        };
        let Some(models) = body["data"].as_array() else {
            catalog.message = "Router did not return a model list";
            break;
        };
        for model in models {
            let Some(id) = model["id"].as_str().filter(|id| valid_model_id(id)) else {
                continue;
            };
            if ids.len() == MAX_MODELS && !ids.contains(id) {
                catalog.models = ids.into_iter().collect();
                return Ok(catalog);
            }
            let token_limit = |field: &str| {
                model[field]
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|&value| value > 0)
            };
            let limits = match protocol {
                ModelProtocol::Openai => ModelLimits {
                    context: token_limit("max_model_len"),
                    output: None,
                },
                ModelProtocol::Anthropic => ModelLimits {
                    context: token_limit("max_input_tokens"),
                    output: token_limit("max_tokens"),
                },
            };
            if ids.insert(id.to_owned()) {
                catalog.limits.insert(id.to_owned(), limits);
            }
        }
        if body["has_more"] != true {
            catalog.complete = true;
            catalog.message = "Model list refreshed";
            break;
        }
        let Some(next) = body["last_id"].as_str().filter(|id| valid_model_id(id)) else {
            catalog.message = "Router did not supply a pagination cursor";
            break;
        };
        if !matches!(protocol, ModelProtocol::Anthropic) || !cursors.insert(next.to_owned()) {
            catalog.message = "Router returned unsupported or repeated pagination";
            break;
        }
        cursor = Some(next.to_owned());
    }
    catalog.models = ids.into_iter().collect();
    Ok(catalog)
}

fn valid_model_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
}

pub const MODEL_TEST_PROMPT: &str = "Reply with OK.";

#[derive(Serialize)]
pub struct ModelTest {
    pub responded: bool,
    pub http: Option<u16>,
    pub elapsed_ms: u64,
    pub message: &'static str,
}

pub fn test_model(
    profile: &Router,
    token: &str,
    protocol: ModelProtocol,
    model: &str,
) -> Result<ModelTest> {
    validate(profile)?;
    if !valid_model_id(model) {
        return Err("Enter a valid model ID".into());
    }
    let (resource, token_field) = match protocol {
        ModelProtocol::Openai => ("chat/completions", "max_completion_tokens"),
        ModelProtocol::Anthropic => ("messages", "max_tokens"),
    };
    let url = protocol.resource(profile, resource)?;
    let client = http_client(Duration::from_secs(20))?;
    let mut body = json!({
        "model": model, "stream": false,
        "messages": [{"role": "user", "content": MODEL_TEST_PROMPT}],
    });
    body[token_field] = json!(64);
    let started = Instant::now();
    let request = protocol
        .request(client.post(url), profile, token)
        .json(&body);
    let (responded, http, message) = match request.send() {
        Ok(response) => {
            let code = response.status().as_u16();
            if response.status().is_success() {
                let text = |value: &Value| value.as_str().is_some_and(|s| !s.trim().is_empty());
                let has_reply = response_json(response).is_some_and(|body| match protocol {
                    ModelProtocol::Openai => text(&body["choices"][0]["message"]["content"]),
                    ModelProtocol::Anthropic => body["content"].as_array().is_some_and(|parts| {
                        parts
                            .iter()
                            .any(|part| part["type"] == "text" && text(&part["text"]))
                    }),
                });
                (
                    has_reply,
                    Some(code),
                    if has_reply {
                        "Model responded"
                    } else {
                        "Router returned no readable model text"
                    },
                )
            } else {
                (false, Some(code), http_failure(code))
            }
        }
        Err(err) => (
            false,
            None,
            if err.is_timeout() {
                "Model request timed out"
            } else {
                "Could not reach the model endpoint"
            },
        ),
    };
    crate::diagnostics::record(
        if responded { "INFO" } else { "WARN" },
        "router.model_test_result",
        Some(&profile.id),
        http.map(i64::from),
        Some(message),
    );
    Ok(ModelTest {
        responded,
        http,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message,
    })
}

#[derive(Debug, Serialize)]
pub struct Probe {
    tcp: &'static str,
    http: Option<u16>,
    result: &'static str,
    models: Vec<String>,
}

pub fn test(profile: &Router, token: &str) -> Result<Probe> {
    let probe = probe(profile, token)?;
    crate::diagnostics::record(
        if probe
            .http
            .is_some_and(|status| (200..300).contains(&status))
        {
            "INFO"
        } else {
            "WARN"
        },
        "router.probe_result",
        Some(&profile.id),
        probe.http.map(i64::from),
        Some(probe.result),
    );
    Ok(probe)
}

fn probe(profile: &Router, token: &str) -> Result<Probe> {
    validate(profile)?;
    let url = base(profile)?;
    let host = url
        .host_str()
        .ok_or("Host required")?
        .trim_matches(['[', ']'])
        .to_owned();
    let port = profile.port;
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = send.send(
            (host.as_str(), port)
                .to_socket_addrs()
                .map(|a| a.collect::<Vec<_>>()),
        );
    });
    let addresses = match receive.recv_timeout(Duration::from_secs(2)) {
        Ok(value) => value.map_err(error)?,
        Err(_) => {
            return Ok(Probe {
                tcp: "timeout",
                http: None,
                result: "DNS lookup timed out",
                models: vec![],
            })
        }
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut tcp = "unreachable";
    for addr in addresses {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            tcp = "timeout";
            break;
        }
        match TcpStream::connect_timeout(&addr, remaining) {
            Ok(_) => {
                tcp = "connected";
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => tcp = "timeout",
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => tcp = "refused",
            Err(_) => {}
        }
    }
    let mut probe = Probe {
        tcp,
        http: None,
        result: "No TCP connection",
        models: vec![],
    };
    if tcp != "connected" {
        crate::diagnostics::record(
            "WARN",
            "router.tcp_failed",
            Some(&profile.id),
            None,
            Some(tcp),
        );
        return Ok(probe);
    }
    let request_url = match &profile.openai_path {
        Some(path) => format!("{}/models", endpoint(profile, path)?),
        None => format!(
            "{}/",
            endpoint(profile, profile.anthropic_path.as_deref().unwrap_or(""))?
        ),
    };
    let client = http_client(Duration::from_secs(3))?;
    let mut request = client.get(request_url);
    if profile.authentication == "bearer" && !token.is_empty() {
        request = request.bearer_auth(token);
    }
    let response = match request.send() {
        Ok(response) => response,
        Err(e) => {
            probe.result = if e.is_timeout() {
                "HTTP timeout"
            } else {
                "HTTP request failed"
            };
            return Ok(probe);
        }
    };
    probe.http = Some(response.status().as_u16());
    probe.result = match probe.http {
        Some(401 | 403) => "Authentication required or rejected",
        Some(404) => "Path not found",
        _ => "HTTP server responded; confirm API support manually",
    };
    if response.status().is_success() && profile.openai_path.is_some() {
        let mut body = Vec::new();
        if response.take(512 * 1024).read_to_end(&mut body).is_ok() {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
                if let Some(models) = json["data"].as_array() {
                    probe.models = models
                        .iter()
                        .filter_map(|m| m["id"].as_str())
                        .take(100)
                        .map(str::to_owned)
                        .collect();
                    probe.result = "Model list available; inference protocol untested";
                }
            }
        }
    }
    Ok(probe)
}
