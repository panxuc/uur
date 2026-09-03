use anyhow::{Context, Result};
use std::process::Command;

const OFFICIAL_DOWNLOAD_URL: &str = "https://api.nrd.nie.163.com/api/v1/release/dl/1?channel=gwqd";

/// The latest official release filename, e.g. `uuyc_4.38.3.exe`, resolved
/// from the same feed the official client uses.  `uur` never mirrors the
/// binary; it only learns what upstream shipped.
fn latest_filename() -> Result<String> {
    // Follow redirects and stop at the headers: the feed answers with a
    // Location carrying the signed CDN path whose final component names the
    // installer version.
    let output = Command::new("curl")
        .args([
            "-fsIL",
            "--max-time",
            "20",
            "-o",
            "/dev/null",
            "-w",
            "%{url_effective}",
            OFFICIAL_DOWNLOAD_URL,
        ])
        .output()
        .context("running curl")?;
    if !output.status.success() {
        anyhow::bail!("{}", t!("upstream.unreachable"));
    }
    let url = String::from_utf8_lossy(&output.stdout).into_owned();
    let filename = url
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();
    if filename.is_empty() {
        anyhow::bail!("{}", t!("upstream.unparsable", url = url));
    }
    Ok(filename)
}

fn extract_version(filename: &str) -> Option<String> {
    let stem = filename.trim_end_matches(".exe");
    let bytes = stem.as_bytes();
    let mut start = None;
    let mut end = 0usize;
    let mut in_run = false;
    let mut run_start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        let is_part = byte.is_ascii_digit() || *byte == b'.';
        if is_part && !in_run {
            in_run = true;
            run_start = index;
        } else if !is_part && in_run {
            in_run = false;
            if stem[run_start..index].contains('.') {
                start = Some(run_start);
                end = index;
                break;
            }
        }
    }
    if in_run && start.is_none() && stem[run_start..].contains('.') {
        return Some(stem[run_start..].to_string());
    }
    start.map(|s| stem[s..end].to_string())
}

fn installed_version() -> Option<String> {
    let prefix = crate::config::data_dir().ok()?.join("wine");
    crate::wine::find_client_dir(&prefix)?;
    let output = Command::new("wine")
        .env("WINEPREFIX", &prefix)
        .env("WINEDEBUG", "-all")
        .args([
            "reg",
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\GameViewer",
            "/v",
            "DisplayVersion",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| line.contains("DisplayVersion") && line.contains("REG_SZ"))
        .and_then(|line| line.split("REG_SZ").nth(1))
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(ToOwned::to_owned)
}

pub fn check() -> Result<()> {
    let filename = latest_filename()?;
    // Filenames observed upstream: `uuyc_4.33.0.exe`,
    // `UURemote_Setup_4.39.1.1375_<build>_gwqd.exe`.  Take the first
    // dotted-numeric run as the version.
    let upstream =
        extract_version(&filename).with_context(|| format!("no version in {}", filename))?;

    println!(
        "{}",
        t!("upstream.latest", version = upstream, filename = filename)
    );
    match installed_version() {
        Some(local) if local == upstream => println!("{}", t!("upstream.up_to_date")),
        Some(local) => println!(
            "{}",
            t!(
                "upstream.newer_available",
                local = local,
                upstream = upstream
            )
        ),
        None => println!("{}", t!("upstream.not_provisioned")),
    }
    Ok(())
}
