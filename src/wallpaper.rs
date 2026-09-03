//! Desktop-neutral wallpaper discovery and Wine registry synchronization.
//!
//! UU reads the standard Windows `Control Panel\\Desktop\\Wallpaper` value.
//! Providers below discover the active Linux wallpaper without making the
//! session supervisor depend on a particular desktop environment.

use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wallpaper {
    pub path: PathBuf,
    pub provider: &'static str,
}

pub fn detect() -> Option<Wallpaper> {
    if let Some(value) = std::env::var_os("UUR_WALLPAPER") {
        if let Some(path) = resolve_candidate(PathBuf::from(value)) {
            return Some(Wallpaper {
                path,
                provider: "environment",
            });
        }
    }

    if let Some(path) = detect_runtime_provider() {
        return Some(Wallpaper {
            path,
            provider: "wallpaper daemon",
        });
    }

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut providers: Vec<fn() -> Option<Wallpaper>> = Vec::new();
    if desktop.contains("kde") || desktop.contains("plasma") {
        providers.push(detect_kde);
    }
    if desktop.contains("gnome")
        || desktop.contains("unity")
        || desktop.contains("budgie")
        || desktop.contains("pantheon")
    {
        providers.push(detect_gnome_family);
    }
    if desktop.contains("cinnamon") {
        providers.push(detect_cinnamon);
    }
    if desktop.contains("mate") {
        providers.push(detect_mate);
    }
    if desktop.contains("xfce") {
        providers.push(detect_xfce);
    }
    if desktop.contains("lxqt") || desktop.contains("lxde") {
        providers.push(detect_pcmanfm);
    }

    for fallback in [
        detect_kde as fn() -> Option<Wallpaper>,
        detect_gnome_family,
        detect_cinnamon,
        detect_mate,
        detect_xfce,
        detect_pcmanfm,
        detect_nitrogen,
        detect_feh,
    ] {
        if !providers
            .iter()
            .any(|provider| std::ptr::fn_addr_eq(*provider, fallback))
        {
            providers.push(fallback);
        }
    }
    providers.into_iter().find_map(|provider| provider())
}

pub fn sync(prefix: &Path) -> Result<Option<Wallpaper>> {
    let Some(wallpaper) = detect() else {
        return Ok(None);
    };
    let compatible = windows_compatible_path(&wallpaper.path)?;
    let windows_path = format!("Z:{}", compatible.display()).replace('/', "\\");
    let status = Command::new("wine")
        .env("WINEPREFIX", prefix)
        .env("WINEDEBUG", "-all")
        .args([
            "reg",
            "add",
            r"HKCU\Control Panel\Desktop",
            "/v",
            "Wallpaper",
            "/t",
            "REG_SZ",
            "/d",
            &windows_path,
            "/f",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("updating Wine wallpaper registry value")?;
    if !status.success() {
        anyhow::bail!("Wine rejected the wallpaper registry update");
    }
    Ok(Some(wallpaper))
}

pub fn serve() -> Result<()> {
    let prefix = crate::config::data_dir()?.join("wine");
    let stopping = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stopping.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, stopping.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGHUP, stopping.clone())?;

    let mut previous: Option<(PathBuf, Option<SystemTime>)> = None;
    while !stopping.load(AtomicOrdering::Relaxed) {
        if let Some(current) = detect() {
            let modified = current
                .path
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok());
            let fingerprint = (current.path.clone(), modified);
            if previous.as_ref() != Some(&fingerprint) {
                if let Err(error) = sync(&prefix) {
                    eprintln!("wallpaper synchronization failed: {error:#}");
                } else {
                    println!(
                        "wallpaper synchronized via {}: {}",
                        current.provider,
                        current.path.display()
                    );
                    previous = Some(fingerprint);
                }
            }
        }
        for _ in 0..20 {
            if stopping.load(AtomicOrdering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    Ok(())
}

fn detect_kde() -> Option<Wallpaper> {
    let content =
        std::fs::read_to_string(config_home().join("plasma-org.kde.plasma.desktop-appletsrc"))
            .ok()?;
    let value = content
        .lines()
        .find_map(|line| line.strip_prefix("Image="))?;
    wallpaper_from_value(value, "KDE Plasma")
}

fn detect_gnome_family() -> Option<Wallpaper> {
    let dark = command_value(
        "gsettings",
        &["get", "org.gnome.desktop.background", "picture-uri-dark"],
    );
    let regular = command_value(
        "gsettings",
        &["get", "org.gnome.desktop.background", "picture-uri"],
    );
    dark.or(regular)
        .and_then(|value| wallpaper_from_value(&value, "GNOME settings"))
}

fn detect_cinnamon() -> Option<Wallpaper> {
    command_value(
        "gsettings",
        &["get", "org.cinnamon.desktop.background", "picture-uri"],
    )
    .and_then(|value| wallpaper_from_value(&value, "Cinnamon settings"))
}

fn detect_mate() -> Option<Wallpaper> {
    command_value(
        "gsettings",
        &["get", "org.mate.background", "picture-filename"],
    )
    .and_then(|value| wallpaper_from_value(&value, "MATE settings"))
}

fn detect_xfce() -> Option<Wallpaper> {
    let output = command_value("xfconf-query", &["-c", "xfce4-desktop", "-l", "-v"])?;
    output
        .lines()
        .filter(|line| line.contains("last-image"))
        .filter_map(|line| line.split_whitespace().last())
        .find_map(|value| wallpaper_from_value(value, "Xfce settings"))
}

fn detect_pcmanfm() -> Option<Wallpaper> {
    for relative in [
        "pcmanfm-qt/lxqt/settings.conf",
        "pcmanfm/LXDE/desktop-items-0.conf",
        "pcmanfm/default/desktop-items-0.conf",
    ] {
        let Ok(content) = std::fs::read_to_string(config_home().join(relative)) else {
            continue;
        };
        if let Some(value) = content.lines().find_map(|line| {
            line.strip_prefix("Wallpaper=")
                .or_else(|| line.strip_prefix("wallpaper="))
        }) {
            if let Some(found) = wallpaper_from_value(value, "PCManFM settings") {
                return Some(found);
            }
        }
    }
    None
}

fn detect_nitrogen() -> Option<Wallpaper> {
    let content = std::fs::read_to_string(config_home().join("nitrogen/bg-saved.cfg")).ok()?;
    let value = content
        .lines()
        .find_map(|line| line.strip_prefix("file="))?;
    wallpaper_from_value(value, "Nitrogen settings")
}

fn detect_feh() -> Option<Wallpaper> {
    let content = std::fs::read_to_string(home_dir().join(".fehbg")).ok()?;
    content
        .split(['\'', '"'])
        .find_map(|part| wallpaper_from_value(part, "feh configuration"))
}

fn detect_runtime_provider() -> Option<PathBuf> {
    for (command, arguments) in [
        ("swww", &["query"][..]),
        ("hyprctl", &["hyprpaper", "listactive"][..]),
    ] {
        if let Some(output) = command_value(command, arguments) {
            if let Some(path) = output.lines().find_map(path_from_runtime_line) {
                return Some(path);
            }
        }
    }

    for entry in std::fs::read_dir("/proc").ok()? {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let arguments: Vec<_> = bytes
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        let executable = arguments
            .first()
            .and_then(|path| Path::new(path).file_name())
            .and_then(OsStr::to_str)
            .unwrap_or_default();
        let candidate = match executable {
            "swaybg" => argument_after(&arguments, "-i"),
            "xwallpaper" => arguments.last().cloned(),
            "mpvpaper" => arguments.last().cloned(),
            _ => None,
        };
        if let Some(path) = candidate.and_then(|value| resolve_candidate(PathBuf::from(value))) {
            return Some(path);
        }
    }
    None
}

fn path_from_runtime_line(line: &str) -> Option<PathBuf> {
    let value = line
        .split_once("image:")
        .map(|(_, value)| value)
        .or_else(|| line.rsplit_once(" = ").map(|(_, value)| value))?;
    resolve_candidate(PathBuf::from(value.trim()))
}

fn argument_after(arguments: &[String], option: &str) -> Option<String> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == option)
        .map(|pair| pair[1].clone())
}

fn wallpaper_from_value(value: &str, provider: &'static str) -> Option<Wallpaper> {
    let value = value.trim().trim_matches(['\'', '"']);
    if value.is_empty() {
        return None;
    }
    let decoded = value
        .strip_prefix("file://")
        .map(percent_decode)
        .unwrap_or_else(|| value.to_string());
    resolve_candidate(PathBuf::from(decoded)).map(|path| Wallpaper { path, provider })
}

fn resolve_candidate(path: PathBuf) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    if path.is_file() {
        return Some(path);
    }
    if !path.is_dir() {
        return None;
    }
    best_image_in_directory(&path, 4)
}

fn best_image_in_directory(root: &Path, depth: usize) -> Option<PathBuf> {
    let mut files = Vec::new();
    collect_images(root, depth, &mut files);
    let target_ratio = display_ratio().unwrap_or(16.0 / 9.0);
    files.into_iter().max_by(|left, right| {
        image_score(left, target_ratio)
            .partial_cmp(&image_score(right, target_ratio))
            .unwrap_or(Ordering::Equal)
    })
}

fn collect_images(directory: &Path, depth: usize, output: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_images(&path, depth - 1, output);
        } else if supported_image(&path) {
            output.push(path);
        }
    }
}

fn supported_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "jpg" | "jpeg" | "png" | "bmp" | "webp" | "jxl"
    )
}

fn image_score(path: &Path, target_ratio: f64) -> f64 {
    let dimensions = path
        .file_stem()
        .and_then(OsStr::to_str)
        .and_then(parse_dimensions);
    if let Some((width, height)) = dimensions {
        let ratio_error = (width as f64 / height as f64 - target_ratio).abs();
        return 1_000_000_000.0 - ratio_error * 100_000_000.0
            + (width as f64 * height as f64).sqrt();
    }
    path.metadata()
        .map(|metadata| metadata.len() as f64)
        .unwrap_or(0.0)
}

fn parse_dimensions(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn display_ratio() -> Option<f64> {
    let output = command_value("xrandr", &["--current"])?;
    let current = output.split("current ").nth(1)?;
    let mut words = current.split_whitespace();
    let width: f64 = words.next()?.parse().ok()?;
    if words.next()? != "x" {
        return None;
    }
    let height: f64 = words.next()?.trim_end_matches(',').parse().ok()?;
    (height > 0.0).then_some(width / height)
}

fn command_value(program: &str, arguments: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait().ok()? {
            Some(status) => break status,
            None if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    let output = output.trim().to_string();
    (!output.is_empty()).then_some(output)
}

fn windows_compatible_path(source: &Path) -> Result<PathBuf> {
    let metadata = source
        .metadata()
        .with_context(|| format!("reading wallpaper metadata for {}", source.display()))?;
    let extension = source
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "bmp")
        && metadata.len() <= 5 * 1024 * 1024
    {
        return Ok(source.to_path_buf());
    }

    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let cache = crate::config::data_dir()?.join("wallpaper-cache");
    std::fs::create_dir_all(&cache)?;
    let stem = format!("wallpaper-{}-{modified}", metadata.len());
    let jpeg = cache.join(format!("{stem}.jpg"));
    if jpeg.is_file() {
        return Ok(jpeg);
    }

    let temporary_jpeg = cache.join(format!("{stem}.tmp.jpg"));
    let converted = Command::new("magick")
        .arg(source)
        .args([
            "-auto-orient",
            "-resize",
            "3840x2160>",
            "-strip",
            "-quality",
            "88",
        ])
        .arg(&temporary_jpeg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success() && temporary_jpeg.is_file());
    if converted {
        std::fs::rename(&temporary_jpeg, &jpeg)?;
        remove_old_cache_files(&cache, &jpeg);
        return Ok(jpeg);
    }
    let _ = std::fs::remove_file(&temporary_jpeg);

    let png = cache.join(format!("{stem}.png"));
    let temporary_png = cache.join(format!("{stem}.tmp.png"));
    let input_uri = format!("file://{}", source.display());
    let converted = Command::new("glycin-thumbnailer")
        .args(["--input", &input_uri, "--size", "3840", "--output"])
        .arg(&temporary_png)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success() && temporary_png.is_file());
    if converted {
        std::fs::rename(&temporary_png, &png)?;
        remove_old_cache_files(&cache, &png);
        return Ok(png);
    }
    let _ = std::fs::remove_file(&temporary_png);

    // UU may support the source codec through its own image stack. Returning
    // the original is more useful than suppressing wallpaper publication.
    Ok(source.to_path_buf())
}

fn remove_old_cache_files(directory: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != keep
            && path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with("wallpaper-"))
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"))
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::{parse_dimensions, percent_decode};

    #[test]
    fn file_uri_percent_encoding_is_decoded() {
        assert_eq!(
            percent_decode("/Pictures/My%20Wallpaper%23one.png"),
            "/Pictures/My Wallpaper#one.png"
        );
    }

    #[test]
    fn package_image_dimensions_are_recognized() {
        assert_eq!(parse_dimensions("5120x2880"), Some((5120, 2880)));
        assert_eq!(parse_dimensions("wallpaper"), None);
    }
}
