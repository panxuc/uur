//! Capability-selected Wine audio backend.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

fn pactl(arguments: &[&str]) -> Result<String> {
    let output = Command::new("pactl")
        .args(arguments)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .context("querying PulseAudio; install pactl and enable PulseAudio or pipewire-pulse")?;
    anyhow::ensure!(
        output.status.success(),
        "pactl failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Resolve only a real, available source. Explicit loopback selection must
/// fail if unavailable rather than silently capturing a physical microphone.
pub fn capture_source(selection: Option<&str>) -> Result<Option<String>> {
    let Some(selection) = selection else {
        return Ok(None);
    };
    anyhow::ensure!(
        !selection.is_empty() && !selection.chars().any(char::is_control),
        "invalid audio source name"
    );
    let sources: Vec<serde_json::Value> =
        serde_json::from_str(&pactl(&["--format=json", "list", "sources"])?)?;
    let name = if selection == "system" {
        let default = pactl(&["get-default-sink"])?;
        let sinks: Vec<serde_json::Value> =
            serde_json::from_str(&pactl(&["--format=json", "list", "sinks"])?)?;
        sinks
            .iter()
            .find(|sink| sink["name"].as_str() == Some(&default))
            .and_then(|sink| sink["monitor_source"].as_str())
            .context("the default output has no system-audio monitor source")?
            .to_owned()
    } else {
        selection.to_owned()
    };
    anyhow::ensure!(sources.iter().any(|source| source["name"].as_str() == Some(&name)),
        "audio source {name:?} is unavailable; use `uur audio sources` to choose an available source");
    Ok(Some(name))
}

pub fn print_sources() -> Result<()> {
    let sources: Vec<serde_json::Value> =
        serde_json::from_str(&pactl(&["--format=json", "list", "sources"])?)?;
    println!("Default source: {}", pactl(&["get-default-source"])?);
    for source in sources {
        let name = source["name"]
            .as_str()
            .context("audio source has no name")?;
        let description = source["description"].as_str().unwrap_or(name);
        let monitor = source["monitor_source"].as_str().is_some()
            || source["monitor_of_sink"]
                .as_u64()
                .is_some_and(|index| index != u64::from(u32::MAX));
        println!(
            "{name}\t{}\t{description}",
            if monitor {
                "system monitor"
            } else {
                "capture input"
            }
        );
    }
    Ok(())
}

pub fn configure_source(selection: &str) -> Result<()> {
    let choice = (selection != "default").then_some(selection);
    let resolved = capture_source(choice)?;
    crate::config::Config::update(|stored| {
        stored.audio_capture_source = choice.map(ToOwned::to_owned)
    })?;
    println!(
        "Wine default audio capture: {}; applies after `uur stop` and `uur run`",
        resolved
            .as_deref()
            .unwrap_or("follow the existing audio environment")
    );
    if choice == Some("system") {
        println!("The fallback capture input will carry system sound. Separate simultaneous microphone and speaker channels still require the official audio driver.");
    }
    Ok(())
}

pub(crate) fn apply_capture_env(
    command: &mut Command,
    config: &crate::config::Config,
) -> Result<()> {
    if config.audio_capture_source.is_none() {
        return Ok(());
    }
    // Each process resolves once, before starting the managed wineserver/SCM.
    static SOURCE: std::sync::OnceLock<std::result::Result<Option<String>, String>> =
        std::sync::OnceLock::new();
    match SOURCE.get_or_init(|| {
        capture_source(config.audio_capture_source.as_deref()).map_err(|e| format!("{e:#}"))
    }) {
        Ok(Some(source)) => {
            command.env("PULSE_SOURCE", source);
        }
        Ok(None) => {}
        Err(error) => anyhow::bail!("Wine audio capture unavailable: {error}"),
    }
    Ok(())
}

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
        let status = crate::wine::command(prefix)?
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
