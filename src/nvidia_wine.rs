//! Optional Windows-client compatibility path, separate from native video APIs.
//! The user supplies a bridge bundle; normal sessions never load these libraries.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::symlink,
    path::{Component, Path, PathBuf},
};

const STATE: &str = "uur-nvdec-session";
const JOURNAL: &str = "journal.json";
const CACHE: &str = "config/streamer/decoder_codec_capability_cache.json";

#[derive(Serialize, Deserialize)]
struct Entry {
    /// Relative to the managed prefix; backup names are numeric, not user input.
    path: PathBuf,
    original: bool,
    /// None denotes the official client's capability cache, invalidated on exit.
    replacement: Option<PathBuf>,
    original_link: Option<PathBuf>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    entries: Vec<Entry>,
}

pub struct BridgeSession {
    prefix: PathBuf,
    active: bool,
}

pub fn dll_overrides(enabled: bool) -> &'static str {
    if enabled {
        "wevtapi=n,wtsapi32=n;nvcuda,nvcuvid=n"
    } else {
        "wevtapi=n,wtsapi32=n"
    }
}

// Probe the driver without assuming that a library in ldconfig means a working
// GPU. The initial bridge supports a single CUDA device; multi-GPU stays off.
fn supported_host() -> bool {
    unsafe {
        let cuda = libc::dlopen(c"libcuda.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if cuda.is_null() {
            return false;
        }
        let video = libc::dlopen(
            c"libnvcuvid.so.1".as_ptr(),
            libc::RTLD_NOW | libc::RTLD_LOCAL,
        );
        let init = libc::dlsym(cuda, c"cuInit".as_ptr());
        let count = libc::dlsym(cuda, c"cuDeviceGetCount".as_ptr());
        let mut devices = 0;
        let ok = !video.is_null() && !init.is_null() && !count.is_null() && {
            let init: unsafe extern "C" fn(u32) -> i32 = std::mem::transmute(init);
            let count: unsafe extern "C" fn(*mut i32) -> i32 = std::mem::transmute(count);
            init(0) == 0 && count(&mut devices) == 0 && devices == 1
        };
        if !video.is_null() {
            libc::dlclose(video);
        }
        libc::dlclose(cuda);
        ok
    }
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
fn relative(prefix: &Path, path: &Path) -> Result<PathBuf> {
    let rel = path
        .strip_prefix(prefix)
        .context("bridge path outside managed prefix")?;
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("invalid bridge path");
    }
    Ok(rel.to_owned())
}
fn is_link_to(path: &Path, target: &Path) -> bool {
    fs::read_link(path).is_ok_and(|value| value == target)
}
fn validate_entries(journal: &Journal) -> Result<()> {
    if journal.entries.len() != 3 {
        bail!("invalid NVIDIA bridge journal");
    }
    for (i, e) in journal.entries.iter().enumerate() {
        if e.path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            bail!("invalid NVIDIA bridge journal path");
        }
        if i < 2 {
            let expected = if i == 0 {
                "drive_c/windows/system32/nvcuda.dll"
            } else {
                "drive_c/windows/system32/nvcuvid.dll"
            };
            if e.path != Path::new(expected)
                || !e.replacement.as_ref().is_some_and(|p| p.is_absolute())
            {
                bail!("invalid NVIDIA bridge DLL entry");
            }
        } else if e.replacement.is_some()
            || !e.path.ends_with(CACHE)
            || !e.path.starts_with("drive_c")
        {
            bail!("invalid NVIDIA bridge cache entry");
        }
    }
    Ok(())
}

/// Restore an interrupted opt-in session even when the option is now disabled.
/// Caller owns the normal uur session lock. Recovery errors stop startup, so a
/// damaged/externally edited prefix cannot silently masquerade as default mode.
pub fn recover(prefix: &Path) -> Result<()> {
    let state = prefix.join(STATE);
    if !exists(&state) {
        return Ok(());
    }
    // The journal is published before any mutation. An unpublished staging file
    // can be left by interruption during creation; no prefix files changed yet.
    if !exists(&state.join(JOURNAL)) {
        if exists(&state.join("journal.tmp")) {
            fs::remove_file(state.join("journal.tmp"))?;
        }
        fs::remove_dir(&state).context("unexpected files in NVIDIA bridge state")?;
        return Ok(());
    }
    stop_prefix(prefix)?;
    restore_files(prefix)
}
fn restore_files(prefix: &Path) -> Result<()> {
    let state = prefix.join(STATE);
    let journal: Journal = serde_json::from_slice(&fs::read(state.join(JOURNAL))?)?;
    validate_entries(&journal)?;
    // Check all destinations before changing any of them. Do not overwrite a
    // user's DLL edit made while a session was running.
    for (i, e) in journal.entries.iter().enumerate() {
        let dest = prefix.join(&e.path);
        if let Some(target) = &e.replacement {
            if exists(&dest) && !is_link_to(&dest, target) && exists(&state.join(i.to_string())) {
                bail!(
                    "NVIDIA bridge destination changed externally: {}",
                    dest.display()
                );
            }
            if !e.original && exists(&dest) && !is_link_to(&dest, target) {
                bail!(
                    "NVIDIA bridge destination changed externally: {}",
                    dest.display()
                );
            }
        } else if exists(&dest) && !fs::symlink_metadata(&dest)?.file_type().is_file() {
            bail!("capability cache is no longer a regular file");
        }
    }
    for (i, e) in journal.entries.iter().enumerate().rev() {
        let dest = prefix.join(&e.path);
        let backup = state.join(i.to_string());
        if exists(&backup) {
            if exists(&dest) {
                fs::remove_file(&dest)?;
            }
            fs::rename(&backup, &dest)?;
        } else if !e.original && exists(&dest) {
            fs::remove_file(&dest)?;
        } else if e.replacement.as_ref().is_some_and(|p| is_link_to(&dest, p))
            && e.original_link != e.replacement
        {
            bail!(
                "original NVIDIA bridge file backup missing: {}",
                dest.display()
            );
        }
    }
    fs::remove_file(state.join(JOURNAL))?;
    fs::remove_dir(&state)?;
    Ok(())
}

pub fn begin(
    prefix: &Path,
    client_dir: &Path,
    bundle: Option<&Path>,
) -> Result<Option<BridgeSession>> {
    if bundle.is_none() {
        return Ok(None);
    }
    begin_with_support(prefix, client_dir, bundle, supported_host())
}
fn begin_with_support(
    prefix: &Path,
    client_dir: &Path,
    bundle: Option<&Path>,
    supported: bool,
) -> Result<Option<BridgeSession>> {
    let Some(bundle) = bundle else {
        return Ok(None);
    };
    if !supported {
        eprintln!("NVIDIA/Wine decode bridge skipped: requires a working single CUDA GPU and NVDEC driver");
        return Ok(None);
    }
    if !std::process::Command::new("wineserver")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!(
            "NVIDIA/Wine decode bridge skipped: matching wineserver must be available on PATH"
        );
        return Ok(None);
    }
    match install(prefix, client_dir, bundle) {
        Ok(session) => {
            println!("Experimental NVIDIA/Wine decode bridge enabled; confirm hard decode in UU's session statistics");
            Ok(Some(session))
        }
        Err(error) => {
            // A failed install must leave the prefix restored before fallback.
            recover(prefix).context("restoring failed NVIDIA bridge installation")?;
            eprintln!("NVIDIA/Wine decode bridge unavailable; using default path: {error:#}");
            Ok(None)
        }
    }
}
fn install(prefix: &Path, client_dir: &Path, bundle: &Path) -> Result<BridgeSession> {
    let cuda = fs::canonicalize(bundle.join("nvcuda.dll.so"))?;
    let cuvid = fs::canonicalize(bundle.join("nvcuvid.dll"))?;
    // Both relays in the pinned bundle are x86_64 ELF Wine modules (including
    // nvcuvid.dll despite its name). This does not prove runtime/codec support.
    use std::io::Read;
    for path in [&cuda, &cuvid] {
        if !fs::metadata(path)?.is_file() {
            bail!("bridge library is not a regular file");
        }
        let mut header = [0u8; 20];
        fs::File::open(path)?.read_exact(&mut header)?;
        if &header[..6] != b"\x7fELF\x02\x01" || header[18..20] != [0x3e, 0] {
            bail!("expected x86_64 ELF Wine module: {}", path.display());
        }
    }
    let cache = client_dir.join(CACHE);
    let mut entries = Vec::new();
    for (dest, replacement) in [
        (
            prefix.join("drive_c/windows/system32/nvcuda.dll"),
            Some(cuda),
        ),
        (
            prefix.join("drive_c/windows/system32/nvcuvid.dll"),
            Some(cuvid),
        ),
        (cache, None),
    ] {
        if exists(&dest) && fs::symlink_metadata(&dest)?.file_type().is_dir() {
            bail!("bridge destination is a directory");
        }
        if replacement.is_none()
            && exists(&dest)
            && !fs::symlink_metadata(&dest)?.file_type().is_file()
        {
            bail!("capability cache must be a regular file");
        }
        entries.push(Entry {
            path: relative(prefix, &dest)?,
            original: exists(&dest),
            replacement,
            original_link: fs::read_link(&dest).ok(),
        });
    }
    let journal = Journal { entries };
    validate_entries(&journal)?;
    let state = prefix.join(STATE);
    fs::create_dir(&state)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(state.join("journal.tmp"))?;
    file.write_all(&serde_json::to_vec(&journal)?)?;
    file.sync_all()?;
    fs::rename(state.join("journal.tmp"), state.join(JOURNAL))?;
    fs::File::open(&state)?.sync_all()?;
    let session = BridgeSession {
        prefix: prefix.to_owned(),
        active: true,
    };
    for (i, e) in journal.entries.iter().enumerate() {
        let dest = prefix.join(&e.path);
        if e.original {
            fs::rename(&dest, state.join(i.to_string()))?;
        }
        if let Some(target) = &e.replacement {
            symlink(target, &dest)?;
        }
    }
    Ok(session)
}
fn stop_prefix(prefix: &Path) -> Result<()> {
    for arg in ["-k", "-w"] {
        let status = std::process::Command::new("wineserver")
            .env("WINEPREFIX", prefix)
            .arg(arg)
            .status()
            .context("stopping managed Wine prefix before NVIDIA bridge restoration")?;
        if !status.success() && !(arg == "-k" && status.code() == Some(1)) {
            bail!("wineserver {arg} failed before NVIDIA bridge restoration");
        }
    }
    Ok(())
}
impl BridgeSession {
    pub fn restore(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        stop_prefix(&self.prefix)?;
        restore_files(&self.prefix)?;
        self.active = false;
        Ok(())
    }
}
impl Drop for BridgeSession {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("NVIDIA bridge restoration deferred to next run: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ID: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        root: PathBuf,
        prefix: PathBuf,
        bin: PathBuf,
        bundle: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "uur-nvdec-test-{}-{}",
                std::process::id(),
                ID.fetch_add(1, Ordering::Relaxed)
            ));
            let prefix = root.join("wine");
            let bin = prefix.join("drive_c/Program Files/NetEase/GameViewer");
            let bundle = root.join("bundle");
            fs::create_dir_all(prefix.join("drive_c/windows/system32")).unwrap();
            fs::create_dir_all(&bin).unwrap();
            fs::create_dir_all(bin.join("config/streamer")).unwrap();
            fs::create_dir_all(&bundle).unwrap();
            let mut header = [0u8; 20];
            header[..6].copy_from_slice(b"\x7fELF\x02\x01");
            header[18] = 0x3e;
            fs::write(bundle.join("nvcuda.dll.so"), header).unwrap();
            fs::write(bundle.join("nvcuvid.dll"), header).unwrap();
            Self {
                root,
                prefix,
                bin,
                bundle,
            }
        }
        fn dll(&self, n: &str) -> PathBuf {
            self.prefix.join("drive_c/windows/system32").join(n)
        }
        fn cache(&self) -> PathBuf {
            self.bin.join(CACHE)
        }
        fn interrupted(&self, mut session: BridgeSession) {
            session.active = false;
            drop(session);
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn default_and_non_nvidia_do_not_touch_prefix_or_bundle() {
        let f = Fixture::new();
        fs::write(f.dll("nvcuda.dll"), b"original").unwrap();
        assert!(begin(&f.prefix, &f.bin, None).unwrap().is_none());
        assert!(
            begin_with_support(&f.prefix, &f.bin, Some(&f.root.join("missing")), false)
                .unwrap()
                .is_none()
        );
        assert_eq!(fs::read(f.dll("nvcuda.dll")).unwrap(), b"original");
        assert!(!exists(&f.prefix.join(STATE)));
        assert_eq!(dll_overrides(false), "wevtapi=n,wtsapi32=n");
    }
    #[test]
    fn interruption_restores_file_dangling_link_and_original_cache() {
        let f = Fixture::new();
        fs::write(f.dll("nvcuda.dll"), b"original bytes").unwrap();
        symlink("/missing/original", f.dll("nvcuvid.dll")).unwrap();
        fs::write(f.cache(), b"original cache").unwrap();
        let session = install(&f.prefix, &f.bin, &f.bundle).unwrap();
        assert!(!exists(&f.cache()));
        fs::write(f.cache(), b"NVDEC cache").unwrap();
        f.interrupted(session);
        restore_files(&f.prefix).unwrap();
        assert_eq!(fs::read(f.dll("nvcuda.dll")).unwrap(), b"original bytes");
        assert_eq!(
            fs::read_link(f.dll("nvcuvid.dll")).unwrap(),
            Path::new("/missing/original")
        );
        assert_eq!(fs::read(f.cache()).unwrap(), b"original cache");
        assert!(!exists(&f.prefix.join(STATE)));
    }
    #[test]
    fn absent_originals_remove_bridge_and_generated_cache() {
        let f = Fixture::new();
        f.interrupted(install(&f.prefix, &f.bin, &f.bundle).unwrap());
        fs::write(f.cache(), b"NVDEC cache").unwrap();
        restore_files(&f.prefix).unwrap();
        assert!(!exists(&f.dll("nvcuda.dll")));
        assert!(!exists(&f.dll("nvcuvid.dll")));
        assert!(!exists(&f.cache()));
    }
    #[test]
    fn external_edit_is_preserved_until_recovery_can_retry() {
        let f = Fixture::new();
        fs::write(f.dll("nvcuda.dll"), b"original").unwrap();
        f.interrupted(install(&f.prefix, &f.bin, &f.bundle).unwrap());
        fs::remove_file(f.dll("nvcuda.dll")).unwrap();
        fs::write(f.dll("nvcuda.dll"), b"user edit").unwrap();
        assert!(restore_files(&f.prefix).is_err());
        assert_eq!(fs::read(f.dll("nvcuda.dll")).unwrap(), b"user edit");
        assert!(exists(&f.prefix.join(STATE).join("0")));
        fs::remove_file(f.dll("nvcuda.dll")).unwrap();
        symlink(
            fs::canonicalize(f.bundle.join("nvcuda.dll.so")).unwrap(),
            f.dll("nvcuda.dll"),
        )
        .unwrap();
        restore_files(&f.prefix).unwrap();
        assert_eq!(fs::read(f.dll("nvcuda.dll")).unwrap(), b"original");
    }
    #[test]
    fn bad_bundle_leaves_originals_untouched() {
        let f = Fixture::new();
        fs::write(f.cache(), b"original").unwrap();
        fs::write(f.bundle.join("nvcuda.dll.so"), b"bad").unwrap();
        assert!(install(&f.prefix, &f.bin, &f.bundle).is_err());
        assert!(!exists(&f.prefix.join(STATE)));
        assert_eq!(fs::read(f.cache()).unwrap(), b"original");
    }
    #[test]
    fn recovery_resumes_after_partial_restore() {
        let f = Fixture::new();
        fs::write(f.dll("nvcuda.dll"), b"original cuda").unwrap();
        fs::write(f.cache(), b"original cache").unwrap();
        f.interrupted(install(&f.prefix, &f.bin, &f.bundle).unwrap());
        fs::rename(f.prefix.join(STATE).join("2"), f.cache()).unwrap();
        restore_files(&f.prefix).unwrap();
        assert_eq!(fs::read(f.cache()).unwrap(), b"original cache");
        assert_eq!(fs::read(f.dll("nvcuda.dll")).unwrap(), b"original cuda");
    }
    #[test]
    fn invalid_journal_cannot_redirect_recovery() {
        let j = Journal {
            entries: vec![Entry {
                path: PathBuf::from("../outside"),
                original: false,
                replacement: None,
                original_link: None,
            }],
        };
        assert!(validate_entries(&j).is_err());
    }
}
