//! XDG desktop application bridge for UU's Windows quick-launch scanner.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

const MAGIC: u32 = 0x5555_4c41;
const TOKEN_BYTES: usize = 64;
const MAX_APPS: usize = 128;

#[derive(Clone)]
struct DesktopApp {
    id: String,
    name: String,
    path: PathBuf,
}

pub fn prepare(prefix: &Path, proxy: &Path) -> Result<usize> {
    let apps = catalog();
    let root = prefix.join("drive_c/uur-linux-apps");
    if root.exists() {
        std::fs::remove_dir_all(&root).context("replacing Linux quick-launch staging")?;
    }
    std::fs::create_dir_all(&root)?;

    let old_keys = std::fs::read_to_string(root.with_extension("keys")).unwrap_or_default();
    let mut registry = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    for key in old_keys.lines().filter(|key| !key.is_empty()) {
        registry.push_str(&format!("[-HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}]\r\n\r\n"));
    }

    let mut keys = Vec::new();
    for app in apps.values().take(MAX_APPS) {
        let hash = fnv1a(app.id.as_bytes());
        let key = format!("UURLinux_{hash:016x}");
        let directory = root.join(format!("{hash:016x}"));
        std::fs::create_dir_all(&directory)?;
        std::fs::copy(proxy, directory.join("launcher.exe"))?;
        std::fs::write(directory.join("app-id.txt"), format!("{}\n", app.id))?;
        let windows_directory = format!(r"C:\uur-linux-apps\{hash:016x}");
        let executable = format!(r"{windows_directory}\launcher.exe");
        registry.push_str(&format!(
            "[HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{key}]\r\n\
             \"DisplayName\"=\"{}\"\r\n\
             \"DisplayVersion\"=\"1\"\r\n\
             \"Publisher\"=\"Linux desktop\"\r\n\
             \"InstallLocation\"=\"{}\"\r\n\
             \"InstallSource\"=\"XDG Desktop Entry\"\r\n\
             \"DisplayIcon\"=\"{}\"\r\n\
             \"UninstallString\"=\"{}\"\r\n\
             \"NoModify\"=dword:00000001\r\n\
             \"NoRepair\"=dword:00000001\r\n\r\n",
            reg_escape(&app.name),
            reg_escape(&windows_directory),
            reg_escape(&executable),
            reg_escape(&executable),
        ));
        keys.push(key);
    }
    std::fs::write(root.with_extension("keys"), keys.join("\n") + "\n")?;
    let registry_path = prefix.join("drive_c/uur-linux-apps.reg");
    write_utf16_registry(&registry_path, &registry)?;
    let status = Command::new("wine")
        .env("WINEPREFIX", prefix)
        .env("WINEDEBUG", "-all")
        .args(["reg", "import"])
        .arg(&registry_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("importing Linux quick-launch applications")?;
    if !status.success() {
        anyhow::bail!("Wine rejected the Linux quick-launch registry");
    }
    Ok(keys.len())
}

pub fn serve() -> Result<()> {
    let prefix = crate::config::data_dir()?.join("wine");
    let config_path = runtime_config(&prefix);
    let apps = Arc::new(catalog());
    let token = random_token()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    write_config(&config_path, listener.local_addr()?.port(), &token)?;
    println!(
        "native quick-launch service ready with {} applications",
        apps.len()
    );
    for stream in listener.incoming().flatten() {
        let apps = apps.clone();
        let token = token.clone();
        std::thread::spawn(move || {
            if let Err(error) = handle(stream, &token, &apps) {
                eprintln!("quick-launch request failed: {error:#}");
            }
        });
    }
    Ok(())
}

pub fn runtime_config(prefix: &Path) -> PathBuf {
    prefix.join("drive_c/uur-launcher.runtime")
}

fn handle(
    mut stream: TcpStream,
    expected_token: &str,
    apps: &BTreeMap<String, DesktopApp>,
) -> Result<()> {
    let mut header = [0u8; 12];
    stream.read_exact(&mut header)?;
    let magic = u32::from_be_bytes(header[0..4].try_into().unwrap());
    let version = u16::from_be_bytes(header[4..6].try_into().unwrap());
    let token_length = u16::from_be_bytes(header[6..8].try_into().unwrap()) as usize;
    let id_length = u16::from_be_bytes(header[8..10].try_into().unwrap()) as usize;
    if magic != MAGIC
        || version != 1
        || token_length != TOKEN_BYTES
        || id_length == 0
        || id_length > 500
    {
        anyhow::bail!("invalid quick-launch handshake");
    }
    let mut token = [0u8; TOKEN_BYTES];
    stream.read_exact(&mut token)?;
    if !constant_time_equal(&token, expected_token.as_bytes()) {
        anyhow::bail!("quick-launch authentication failed");
    }
    let mut id = vec![0u8; id_length];
    stream.read_exact(&mut id)?;
    let id = std::str::from_utf8(&id)?;
    let accepted = apps.get(id).is_some_and(launch);
    stream.write_all(&[accepted as u8])?;
    Ok(())
}

fn launch(app: &DesktopApp) -> bool {
    let gio = Command::new("gio")
        .arg("launch")
        .arg(&app.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if gio.is_ok() {
        return true;
    }
    Command::new("gtk-launch")
        .arg(app.id.trim_end_matches(".desktop"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

fn catalog() -> BTreeMap<String, DesktopApp> {
    let mut apps = BTreeMap::new();
    for directory in data_directories() {
        collect_desktop_entries(&directory.join("applications"), &mut apps);
    }
    apps
}

fn data_directories() -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
        .split(':')
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .collect();
    if let Some(user) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
    {
        directories.push(user);
    }
    directories
}

fn collect_desktop_entries(directory: &Path, apps: &mut BTreeMap<String, DesktopApp>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_entries(&path, apps);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("desktop")
            || path
                .components()
                .any(|component| component.as_os_str() == "wine")
        {
            continue;
        }
        let Some(id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut in_desktop = false;
        let mut kind = "";
        let mut name = None;
        let mut hidden = false;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_desktop = line == "[Desktop Entry]";
                continue;
            }
            if !in_desktop {
                continue;
            }
            if let Some(value) = line.strip_prefix("Type=") {
                kind = value;
            } else if let Some(value) = line.strip_prefix("Name=") {
                name = Some(value.trim().to_string());
            } else if line == "Hidden=true" || line == "NoDisplay=true" {
                hidden = true;
            }
        }
        if kind == "Application" && !hidden {
            if let Some(name) = name.filter(|name| !name.is_empty()) {
                apps.insert(id.clone(), DesktopApp { id, name, path });
            }
        }
    }
}

fn write_config(path: &Path, port: u16, token: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    writeln!(file, "version=1")?;
    writeln!(file, "port={port}")?;
    writeln!(file, "token={token}")?;
    file.sync_all()?;
    Ok(())
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn reg_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_utf16_registry(path: &Path, content: &str) -> Result<()> {
    let mut bytes = vec![0xff, 0xfe];
    for unit in content.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{fnv1a, reg_escape};

    #[test]
    fn registry_values_are_escaped() {
        assert_eq!(reg_escape(r#"A\B"C"#), r#"A\\B\"C"#);
    }

    #[test]
    fn application_hash_is_stable() {
        let first = fnv1a(b"org.example.App.desktop");
        assert_eq!(first, fnv1a(b"org.example.App.desktop"));
        assert_ne!(first, fnv1a(b"org.example.Other.desktop"));
    }
}
