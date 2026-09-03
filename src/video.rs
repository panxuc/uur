//! Native video acceleration capability inventory.
//!
//! This module reports host APIs independently from UU's Windows encoder.
//! It is the selection boundary for future native frame encoders and decoders.

use std::process::{Command, Stdio};

#[derive(Debug)]
pub struct Capabilities {
    pub render_node: bool,
    pub va_api: bool,
    pub vulkan: bool,
    pub vulkan_video: bool,
    pub nvenc: bool,
    pub nvdec: bool,
}

pub fn detect() -> Capabilities {
    let vulkan = command_output("vulkaninfo", &[]);
    Capabilities {
        render_node: render_node_present(),
        va_api: command_succeeds("vainfo", &["--display", "drm"]),
        vulkan: vulkan.is_some(),
        vulkan_video: vulkan
            .as_deref()
            .is_some_and(|text| text.contains("VK_KHR_video_queue")),
        nvenc: library_present("libnvidia-encode.so"),
        nvdec: library_present("libnvcuvid.so"),
    }
}

fn command_output(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(arguments)
        .env("DISPLAY", std::env::var("DISPLAY").unwrap_or_default())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn describe<'a>(value: bool, available: &'a str, unavailable: &'a str) -> &'a str {
    if value {
        available
    } else {
        unavailable
    }
}

fn render_node_present() -> bool {
    std::fs::read_dir("/dev/dri")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("renderD"))
}

fn command_succeeds(program: &str, arguments: &[&str]) -> bool {
    Command::new(program)
        .args(arguments)
        .env("DISPLAY", std::env::var("DISPLAY").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn library_present(needle: &str) -> bool {
    let cache = Command::new("ldconfig")
        .arg("-p")
        .stdin(Stdio::null())
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    if cache.contains(needle) {
        return true;
    }
    ["/usr/lib", "/usr/local/lib", "/lib"]
        .iter()
        .any(|directory| {
            std::fs::read_dir(directory)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with(needle))
        })
}

#[cfg(test)]
mod tests {
    use super::describe;

    #[test]
    fn capability_description_selects_the_correct_branch() {
        assert_eq!(describe(true, "available", "missing"), "available");
        assert_eq!(describe(false, "available", "missing"), "missing");
    }
}
