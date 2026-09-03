//! Capability-selected Wine audio backend.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    PipeWirePulse,
    PulseAudio,
    PipeWireNative,
    Alsa,
    None,
}

impl Backend {
    pub fn label(self) -> &'static str {
        match self {
            Self::PipeWirePulse => "PulseAudio protocol on PipeWire",
            Self::PulseAudio => "PulseAudio",
            Self::PipeWireNative => "PipeWire (no PulseAudio compatibility detected)",
            Self::Alsa => "ALSA",
            Self::None => "no audio server detected",
        }
    }
}

pub fn detect() -> Backend {
    if let Some(server) = pactl_server() {
        return if server.to_ascii_lowercase().contains("pipewire") {
            Backend::PipeWirePulse
        } else {
            Backend::PulseAudio
        };
    }
    if command_succeeds("wpctl", &["status"]) {
        return Backend::PipeWireNative;
    }
    if Path::new("/proc/asound/cards").is_file() {
        return Backend::Alsa;
    }
    Backend::None
}

/// Select a Wine driver only when a usable host backend is observable. Wine's
/// pulse driver works with both a real PulseAudio daemon and pipewire-pulse.
pub fn sync_wine(prefix: &Path) -> Result<Backend> {
    let backend = detect();
    let driver = match backend {
        Backend::PipeWirePulse | Backend::PulseAudio => Some("pulse"),
        Backend::PipeWireNative | Backend::Alsa => Some("alsa"),
        Backend::None => None,
    };
    if let Some(driver) = driver {
        let status = Command::new("wine")
            .env("WINEPREFIX", prefix)
            .env("WINEDEBUG", "-all")
            .args([
                "reg",
                "add",
                r"HKCU\Software\Wine\Drivers",
                "/v",
                "Audio",
                "/t",
                "REG_SZ",
                "/d",
                driver,
                "/f",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("selecting the Wine audio driver")?;
        if !status.success() {
            anyhow::bail!("Wine rejected the {driver} audio driver setting");
        }
    }
    Ok(backend)
}

fn pactl_server() -> Option<String> {
    let output = Command::new("pactl")
        .arg("info")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.trim() == "Server Name")
        })
        .map(|(_, value)| value.trim().to_string())
}

fn command_succeeds(program: &str, arguments: &[&str]) -> bool {
    Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::Backend;

    #[test]
    fn backend_labels_are_user_facing() {
        assert_eq!(Backend::PulseAudio.label(), "PulseAudio");
        assert!(Backend::PipeWirePulse.label().contains("PipeWire"));
    }
}
