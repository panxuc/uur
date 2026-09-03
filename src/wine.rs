use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::Config;

const OFFICIAL_DOWNLOAD_URL: &str = "https://api.nrd.nie.163.com/api/v1/release/dl/1?channel=gwqd";
const EULA_URL: &str = "https://uuyc.163.com/contact/20240402/40294_1146065.html";

/// One-time provisioning.  Deliberately quiet: progress on stdout, nothing
/// else.  NetEase binaries are never redistributed — they are fetched from
/// the official feed into a per-user prefix.
pub fn setup(accept_eula: bool) -> Result<()> {
    let mut config = Config::load()?;

    if !config.eula_accepted {
        if !accept_eula {
            println!("{}", t!("setup.eula_prompt", url = EULA_URL));
            anyhow::bail!("{}", t!("setup.eula_required"));
        }
        config.eula_accepted = true;
        config.store()?;
        println!("{}", t!("setup.eula_recorded"));
    }

    let data = crate::config::data_dir()?;
    let prefix = data.join("wine");
    std::fs::create_dir_all(&prefix).context("creating wine prefix directory")?;

    let cache = data.join("cache");
    std::fs::create_dir_all(&cache).context("creating cache directory")?;

    if find_client_dir(&prefix).is_some() {
        apply_registry_compat(&prefix)?;
        let uur_bin = std::env::current_exe().unwrap_or_default();
        let _ = adopt_shortcut(&prefix, &uur_bin);
        println!("{}", t!("setup.already_provisioned"));
        stop_wineserver(&prefix);
        return Ok(());
    }

    println!("{}", t!("setup.downloading", url = OFFICIAL_DOWNLOAD_URL));
    let installer = cache.join("uu-remote-setup.exe");
    let status = Command::new("curl")
        .args([
            "-fL",
            "--retry",
            "3",
            "--progress-bar",
            "-o",
            installer.to_string_lossy().as_ref(),
            OFFICIAL_DOWNLOAD_URL,
        ])
        .status()
        .context("running curl")?;
    if !status.success() {
        anyhow::bail!("{}", t!("setup.download_failed"));
    }

    println!("{}", t!("setup.initializing_prefix"));
    let _ = Command::new("wineboot")
        .env("WINEPREFIX", &prefix)
        .env("WINEDEBUG", wine_debug())
        .arg("--init")
        .status()
        .context("running wineboot")?;

    // The preload chain only works if Wine resolves wevtapi to the native
    // shim in the application directory instead of its builtin.
    let _ = Command::new("wine")
        .env("WINEPREFIX", &prefix)
        .env("WINEDEBUG", wine_debug())
        .args([
            "reg",
            "add",
            r"HKCU\Software\Wine\DllOverrides",
            "/v",
            "wevtapi",
            "/t",
            "REG_SZ",
            "/d",
            "native",
            "/f",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("registering wevtapi dll override")?;

    println!("{}", t!("setup.installing_client"));
    let status = Command::new("wine")
        .env("WINEPREFIX", &prefix)
        .env("WINEDEBUG", wine_debug())
        .arg(installer.to_string_lossy().as_ref())
        .arg("/S")
        .status()
        .context("launching installer")?;
    if !status.success() {
        println!("{}", t!("setup.silent_install_unavailable"));
    }

    // winemenubuilder generated its entries during the install: adopt the
    // start-menu one for the full session, then stop it from producing raw
    // shortcuts ever again, and sweep desktop copies.
    let _ = Command::new("wine")
        .env("WINEPREFIX", &prefix)
        .env("WINEDEBUG", wine_debug())
        .args([
            "reg",
            "add",
            r"HKCU\Software\Wine\DllOverrides",
            "/v",
            "winemenubuilder",
            "/t",
            "REG_SZ",
            "/d",
            "",
            "/f",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("disabling winemenubuilder")?;
    apply_registry_compat(&prefix)?;
    let uur_bin = std::env::current_exe().unwrap_or_default();
    let _ = adopt_shortcut(&prefix, &uur_bin);
    purge_wine_shortcuts();

    println!("{}", t!("setup.done", path = prefix.display().to_string()));
    stop_wineserver(&prefix);
    Ok(())
}

/// Registry compatibility profile, ported from the field-tested reference
/// implementation:
///
/// - msedgewebview2.exe runs under Wine only with a win8 app-compat
///   override — without it the WebView2 process never spawns;
/// - GameViewerServer.exe sees Windows 10 because current UU terminal builds
///   gate their ConPTY transport on the OS version. Native input and display
///   adapters handle the hardware capabilities separately;
/// - the service must auto-start;
/// - X11 focus/XIM tuning and CJK font substitution keep the controller
///   usable.
type RegistryEntry = (String, Vec<(&'static str, &'static str, &'static str)>);

pub fn apply_registry_compat(prefix: &Path) -> Result<()> {
    let entries: Vec<RegistryEntry> = vec![
        (
            r#"Software\Wine\AppDefaults\msedgewebview2.exe"#.to_string(),
            vec![("Version", "REG_SZ", "win8")],
        ),
        (
            r#"Software\Wine\AppDefaults\GameViewerServer.exe"#.to_string(),
            vec![("Version", "REG_SZ", "win10")],
        ),
        (
            r"System\CurrentControlSet\Services\GameViewerService".to_string(),
            vec![("Start", "REG_DWORD", "2")],
        ),
        (
            r"Software\Wine\X11 Driver".to_string(),
            vec![("UseTakeFocus", "REG_SZ", "N")],
        ),
        (
            r"Software\Wine\AppDefaults\gameviewer.exe\X11 Driver".to_string(),
            vec![("UseXIM", "REG_SZ", "N"), ("UseTakeFocus", "REG_SZ", "Y")],
        ),
        (
            r"Software\Microsoft\Windows NT\CurrentVersion\FontSubstitutes".to_string(),
            vec![
                ("MS Shell Dlg", "REG_SZ", "Noto Sans CJK SC"),
                ("MS Shell Dlg 2", "REG_SZ", "Noto Sans CJK SC"),
            ],
        ),
    ];

    for (key, values) in &entries {
        let full = if key.starts_with("System") {
            format!("HKLM\\{key}")
        } else {
            format!("HKCU\\{key}")
        };
        for (value, reg_type, data) in values {
            let _ = Command::new("wine")
                .env("WINEPREFIX", prefix)
                .env("WINEDEBUG", wine_debug())
                .args([
                    "reg", "add", &full, "/v", value, "/t", reg_type, "/d", data, "/f",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .with_context(|| format!("reg add {full} /v {value}"))?;
        }
    }
    Ok(())
}

pub fn set_dll_override(prefix: &Path, name: &str, mode: &str) -> Result<()> {
    let status = Command::new("wine")
        .env("WINEPREFIX", prefix)
        .env("WINEDEBUG", wine_debug())
        .args([
            "reg",
            "add",
            r"HKCU\Software\Wine\DllOverrides",
            "/v",
            name,
            "/t",
            "REG_SZ",
            "/d",
            mode,
            "/f",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("registering {name} dll override"))?;
    if !status.success() {
        anyhow::bail!("Wine rejected the {name} DLL override");
    }
    Ok(())
}

fn wine_debug() -> String {
    std::env::var("WINEDEBUG").unwrap_or_else(|_| "-all".to_string())
}

fn stop_wineserver(prefix: &Path) {
    let _ = Command::new("wineserver")
        .env("WINEPREFIX", prefix)
        .arg("-k")
        .status();
}

/// Locate the installed client directory regardless of how NetEase spells
/// the vendor folder ("Netease", "NetEase", ...).  Case matters on Linux.
pub fn find_client_dir(prefix: &Path) -> Option<PathBuf> {
    let program_files = prefix.join("drive_c").join("Program Files");
    let entries = std::fs::read_dir(&program_files).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().eq_ignore_ascii_case("netease") {
            let vendor = entry.path();
            if let Some(found) = find_dir_recursive(&vendor, "GameViewerServer.exe", 4) {
                let bin = found.parent()?;
                if bin
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("bin"))
                {
                    return bin.parent().map(Path::to_path_buf);
                }
                return Some(bin.to_path_buf());
            }
            return Some(vendor);
        }
    }
    None
}

fn find_dir_recursive(start: &Path, needle: &str, depth: u8) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(start).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().map(|n| n == needle).unwrap_or(false) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_dir_recursive(&path, needle, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// Preserve an existing Wine-generated launcher as a hidden compatibility
/// alias. Previously pinned desktop icons keep working, but the packaged
/// `uur.desktop` remains the one visible menu entry.
pub fn adopt_shortcut(prefix: &Path, uur_bin: &Path) -> Result<()> {
    use std::fs;

    let mut name = None;
    let users = prefix.join("drive_c/users");
    if let Ok(entries) = fs::read_dir(&users) {
        'outer: for user in entries.flatten() {
            let programs = user
                .path()
                .join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs");
            let Ok(items) = fs::read_dir(&programs) else {
                continue;
            };
            for item in items.flatten() {
                let file_name = item.file_name().to_string_lossy().into_owned();
                if file_name.ends_with(".lnk") && shortcut_targets_gameviewer(&item.path()) {
                    name = Some(file_name.trim_end_matches(".lnk").to_string());
                    break 'outer;
                }
            }
        }
    }
    let Some(name) = name else {
        return Ok(());
    };

    let mut icon_name = String::new();
    let home = std::env::var("HOME").unwrap_or_default();
    let icons = PathBuf::from(home).join(".local/share/icons");
    let mut best = 0u32;
    if let Ok(sizes) = fs::read_dir(icons.join("hicolor")) {
        for size in sizes.flatten() {
            let size_name = size.file_name().to_string_lossy().into_owned();
            let Ok(width) = size_name.split('x').next().unwrap_or("").parse::<u32>() else {
                continue;
            };
            let Ok(apps) = fs::read_dir(size.path().join("apps")) else {
                continue;
            };
            for app in apps.flatten() {
                let file_name = app.file_name().to_string_lossy().into_owned();
                if file_name.contains("GameViewer") && width > best {
                    best = width;
                    icon_name = file_name
                        .trim_end_matches(".png")
                        .trim_end_matches(".0")
                        .to_string();
                }
            }
        }
    }

    let applications = std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".local/share/applications"))
        .context("no home")?;
    std::fs::create_dir_all(&applications)?;
    let entry = applications.join(format!("{name}.desktop"));
    let exec = uur_bin.display();
    let icon_line = if icon_name.is_empty() {
        String::new()
    } else {
        format!("Icon={icon_name}\n")
    };
    std::fs::write(
        &entry,
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name={name}\n\
             Comment=UU Remote for Linux (uur)\n\
             Exec={exec} run\n\
             TryExec={exec}\n\
             {icon_line}Terminal=false\n\
             NoDisplay=true\n\
             X-UUR-Managed=true\n\
             Categories=Network;RemoteAccess;\n"
        ),
    )
    .with_context(|| format!("writing {}", entry.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&entry, fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

fn shortcut_targets_gameviewer(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let searchable: String = bytes
        .into_iter()
        .filter(|byte| *byte != 0)
        .map(|byte| byte as char)
        .collect::<String>()
        .to_ascii_lowercase();
    searchable.contains("gameviewer")
}

/// Remove Wine-generated desktop copies of the client.  The start-menu
/// entry is ours and correct; only Desktop copies are swept.
fn purge_wine_shortcuts() {
    let home = match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home),
        Err(_) => return,
    };
    let matches = |name: &str| {
        let lower = name.to_lowercase();
        lower.contains("uuremote") || lower.contains("gameviewer") || lower.contains("uu remote")
    };
    let dir = home.join("Desktop");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches(&name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
