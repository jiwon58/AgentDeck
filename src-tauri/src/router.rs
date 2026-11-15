use crate::model::{error, Result, Router};
use reqwest::{blocking::Client, Url};
use serde::Serialize;
use std::{
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
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(error)?;
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
