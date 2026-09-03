//! Host Wake-on-LAN configuration without a desktop- or distro-specific policy.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct State {
    pub interface: String,
    pub mac: String,
    pub supported: Option<bool>,
    pub enabled: Option<bool>,
    pub connection: Option<String>,
}

pub fn status(requested: Option<&str>) -> Result<()> {
    let state = inspect(requested)?;
    println!("{}", t!("wol.interface", interface = state.interface));
    println!("{}", t!("wol.mac", mac = state.mac));
    println!(
        "{}",
        t!("wol.magic_support", state = tri_state(state.supported))
    );
    println!(
        "{}",
        t!("wol.magic_enabled", state = tri_state(state.enabled))
    );
    println!(
        "{}",
        t!(
            "wol.connection",
            connection = state.connection.as_deref().unwrap_or("-")
        )
    );
    Ok(())
}

pub fn configure(requested: Option<&str>, enabled: bool) -> Result<()> {
    let before = inspect(requested)?;
    if enabled && before.supported == Some(false) {
        anyhow::bail!("{}", t!("wol.unsupported", interface = before.interface));
    }

    if let Some(connection) = &before.connection {
        let value = if enabled { "magic" } else { "0" };
        checked(
            Command::new("nmcli")
                .args([
                    "connection",
                    "modify",
                    connection,
                    "802-3-ethernet.wake-on-lan",
                    value,
                ])
                .stdin(Stdio::null()),
            "updating the NetworkManager Wake-on-LAN profile",
        )?;
    }

    let mode = if enabled { "g" } else { "d" };
    checked(
        Command::new("ethtool")
            .args(["-s", &before.interface, "wol", mode])
            .stdin(Stdio::null()),
        &format!(
            "setting Wake-on-LAN on {} (run this command as root or through an authorized Polkit session)",
            before.interface
        ),
    )?;

    let after = inspect(Some(&before.interface))?;
    if after.enabled != Some(enabled) {
        anyhow::bail!("{}", t!("wol.verify_failed", interface = before.interface));
    }
    println!(
        "{}",
        if enabled {
            t!("wol.enabled", interface = before.interface)
        } else {
            t!("wol.disabled", interface = before.interface)
        }
    );
    Ok(())
}

pub fn inspect(requested: Option<&str>) -> Result<State> {
    let interface = select_interface(requested)?;
    let base = Path::new("/sys/class/net").join(&interface);
    let mac = std::fs::read_to_string(base.join("address"))
        .unwrap_or_else(|_| "unknown".into())
        .trim()
        .to_string();
    let properties = Command::new("ethtool")
        .arg(&interface)
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    let supported = properties
        .as_deref()
        .and_then(|text| property(text, "Supports Wake-on:").map(|value| value.contains('g')));
    let enabled = properties
        .as_deref()
        .and_then(|text| property(text, "Wake-on:").map(|value| value.contains('g')));
    Ok(State {
        interface: interface.clone(),
        mac,
        supported,
        enabled,
        connection: active_connection(&interface),
    })
}

fn select_interface(requested: Option<&str>) -> Result<String> {
    if let Some(interface) = requested {
        validate_interface(interface)?;
        return Ok(interface.to_string());
    }
    if let Some(interface) = default_route_interface().filter(|name| is_wired_physical(name)) {
        return Ok(interface);
    }
    let mut candidates = std::fs::read_dir("/sys/class/net")
        .context("enumerating network interfaces")?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_wired_physical(name))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .context("no physical wired network interface found")
}

fn validate_interface(interface: &str) -> Result<()> {
    if interface.is_empty()
        || !interface
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
        || !is_wired_physical(interface)
    {
        anyhow::bail!("invalid or non-wired physical interface: {interface}");
    }
    Ok(())
}

fn is_wired_physical(interface: &str) -> bool {
    let base = PathBuf::from("/sys/class/net").join(interface);
    base.join("device").exists() && !base.join("wireless").exists()
}

fn default_route_interface() -> Option<String> {
    let output = Command::new("ip")
        .args(["-o", "route", "show", "default"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let fields = text.split_whitespace().collect::<Vec<_>>();
    let index = fields.iter().position(|field| *field == "dev")?;
    fields.get(index + 1).map(|value| value.to_string())
}

fn active_connection(interface: &str) -> Option<String> {
    let output = Command::new("nmcli")
        .args(["-g", "GENERAL.CONNECTION", "device", "show", interface])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty() && value != "--").then_some(value)
}

fn property<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(name).map(str::trim))
}

fn checked(command: &mut Command, context: &str) -> Result<()> {
    let output = command.output().with_context(|| context.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    anyhow::bail!("{context}: {detail}")
}

fn tri_state(value: Option<bool>) -> String {
    match value {
        Some(true) => t!("wol.yes").to_string(),
        Some(false) => t!("wol.no").to_string(),
        None => t!("wol.unknown_ethtool").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::property;

    #[test]
    fn parses_ethtool_wake_properties_without_confusing_them() {
        let sample = "Supports Wake-on: pumbg\nWake-on: g\n";
        assert_eq!(property(sample, "Supports Wake-on:"), Some("pumbg"));
        assert_eq!(property(sample, "Wake-on:"), Some("g"));
    }
}
