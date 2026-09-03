//! Import standard Linux proxy environment into Wine's WinINet settings.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

pub fn sync(prefix: &Path) -> Result<Option<String>> {
    let http = environment_proxy("HTTP_PROXY", "http_proxy");
    let https = environment_proxy("HTTPS_PROXY", "https_proxy");
    if http.is_none() && https.is_none() {
        return Ok(None);
    }
    let mut entries = Vec::new();
    if let Some(endpoint) = http {
        entries.push(format!("http={endpoint}"));
    }
    if let Some(endpoint) = https {
        entries.push(format!("https={endpoint}"));
    }
    let server = entries.join(";");
    reg_add(prefix, "ProxyEnable", "REG_DWORD", "1")?;
    reg_add(prefix, "ProxyServer", "REG_SZ", &server)?;
    if let Some(no_proxy) = std::env::var("NO_PROXY")
        .ok()
        .or_else(|| std::env::var("no_proxy").ok())
    {
        let bypass = no_proxy
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>()
            .join(";");
        if !bypass.is_empty() {
            reg_add(prefix, "ProxyOverride", "REG_SZ", &bypass)?;
        }
    }
    Ok(Some(server))
}

fn environment_proxy(upper: &str, lower: &str) -> Option<String> {
    std::env::var(upper)
        .ok()
        .or_else(|| std::env::var(lower).ok())
        .and_then(|value| proxy_endpoint(&value))
}

fn proxy_endpoint(value: &str) -> Option<String> {
    let value = value.trim();
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(value)
        .split('/')
        .next()?;
    let endpoint = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    (!endpoint.is_empty()).then(|| endpoint.to_string())
}

fn reg_add(prefix: &Path, value: &str, kind: &str, data: &str) -> Result<()> {
    let status = Command::new("wine")
        .env("WINEPREFIX", prefix)
        .env("WINEDEBUG", "-all")
        .args([
            "reg",
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            value,
            "/t",
            kind,
            "/d",
            data,
            "/f",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("writing WinINet proxy value {value}"))?;
    if !status.success() {
        anyhow::bail!("Wine rejected WinINet proxy value {value}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::proxy_endpoint;

    #[test]
    fn proxy_url_is_reduced_without_leaking_credentials() {
        assert_eq!(
            proxy_endpoint("http://user:secret@127.0.0.1:7890/path"),
            Some("127.0.0.1:7890".into())
        );
        assert_eq!(
            proxy_endpoint("proxy.local:8080"),
            Some("proxy.local:8080".into())
        );
    }
}
