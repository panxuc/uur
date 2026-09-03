//! Portal side of the capture path.
//!
//! Negotiates an xdg-desktop-portal ScreenCast session, obtains a private
//! PipeWire file descriptor and the video node id, then hands both to the
//! C capture helper (`uur-pw-capture`) which runs the PipeWire stream and
//! publishes frames into a private XDG runtime file (see
//! docs/capture-protocol.md).
//!
//! Consent: the first run shows the compositor's share dialog once.  The
//! returned restore token is stored in uur's config; every later run hands
//! it back with PersistMode::Persistent and the portal restores the choice
//! silently — no dialog, no local interaction, which is the whole point on
//! a headless remote host.

use anyhow::{anyhow, Context, Result};
use std::os::fd::AsRawFd;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::process::{Child, Command};

use crate::config::Config;

/// Run the capture path until interrupted.
pub fn run() -> Result<()> {
    let mut config = Config::load()?;
    let (pw_fd, node) = portal_session(&mut config)?;
    println!("{}", t!("capture.session_ready", node = node));

    let mut helper = spawn_helper(pw_fd, node)?;
    let status = helper.wait().context("waiting for capture helper")?;
    if !status.success() {
        anyhow::bail!("{}", t!("capture.helper_failed"));
    }
    Ok(())
}

/// Start the native PipeWire consumer with a portal-owned descriptor. The
/// descriptor remains private to the child and is never reopened by path.
pub(crate) fn spawn_helper(pw_fd: OwnedFd, node: u32) -> Result<Child> {
    // The helper inherits this descriptor: strip close-on-exec and keep the
    // OwnedFd alive across the spawn (closing it would invalidate the number
    // the child receives).
    let raw = pw_fd.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(raw, libc::F_GETFD);
        if flags < 0 {
            return Err(std::io::Error::last_os_error()).context("fcntl F_GETFD");
        }
        if libc::fcntl(raw, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
            return Err(std::io::Error::last_os_error()).context("fcntl F_SETFD");
        }
    }

    let helper = find_helper()?;
    let mut command = Command::new(&helper);
    command
        .env("UUR_PW_FD", raw.to_string())
        .env("UUR_PW_NODE", node.to_string())
        .env("UUR_FRAME_PATH", frame_path()?);
    if let Ok(debug) = std::env::var("UUR_PW_DEBUG") {
        command.env("PIPEWIRE_DEBUG", debug);
    }
    let child = command
        .spawn()
        .with_context(|| format!("running {}", helper.display()))?;
    Ok(child)
}

fn frame_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("UUR_FRAME_PATH") {
        return Ok(PathBuf::from(path));
    }
    Ok(crate::config::runtime_dir()?.join("frames.v1"))
}

/// Open the portal session and return (inherited fd number, node id).
/// First attempt reuses the stored restore token; if that fails (stale
/// token, monitor changed, compositor restart) fall back to one
/// interactive attempt and store the fresh token.
#[allow(clippy::type_complexity)]
fn portal_session(config: &mut Config) -> Result<(OwnedFd, u32)> {
    let saved_token = config.capture_restore_token.clone();
    let runtime = tokio::runtime::Runtime::new().context("creating tokio runtime")?;
    runtime.block_on(async move {
        let screencast = ashpd::desktop::screencast::Screencast::new()
            .await
            .context("connecting to the ScreenCast portal")?;

        let mut last_error = None;
        let source = source_type(&config.capture_source);
        for token in [saved_token.clone(), None] {
            match negotiate(&screencast, source, token.as_deref()).await {
                Ok((fd, node, fresh_token)) => {
                    if fresh_token != saved_token {
                        config.capture_restore_token = fresh_token;
                        config.store().context("storing the new restore token")?;
                    }
                    return Ok((fd, node));
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("portal session failed")))
    })
}

/// One full portal handshake.  Returns the PipeWire fd, the video node id
/// and the restore token the portal issued for this selection.
async fn negotiate(
    screencast: &ashpd::desktop::screencast::Screencast<'_>,
    source: ashpd::desktop::screencast::SourceType,
    restore_token: Option<&str>,
) -> Result<(OwnedFd, u32, Option<String>)> {
    use ashpd::desktop::screencast::CursorMode;
    use ashpd::desktop::PersistMode;

    let session = screencast.create_session().await?;

    screencast
        .select_sources(
            &session,
            CursorMode::Embedded,
            source.into(),
            false,
            restore_token,
            PersistMode::ExplicitlyRevoked,
        )
        .await?;

    let response = screencast.start(&session, None).await?;
    let streams_response = response.response()?;
    let streams = streams_response.streams();
    let stream = streams.first().context("no monitor selected")?;
    let node = stream.pipe_wire_node_id();
    let fresh_token = streams_response.restore_token().map(String::from);

    let fd = screencast
        .open_pipe_wire_remote(&session)
        .await
        .context("opening the PipeWire remote")?;
    Ok((fd, node, fresh_token))
}

pub(crate) fn source_type(value: &str) -> ashpd::desktop::screencast::SourceType {
    use ashpd::desktop::screencast::SourceType;
    match value {
        "window" => SourceType::Window,
        "virtual" => SourceType::Virtual,
        _ => SourceType::Monitor,
    }
}

fn find_helper() -> Result<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/usr/lib/uur/uur-pw-capture"),
        PathBuf::from("/usr/local/lib/uur/uur-pw-capture"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(home).join(".local/lib/uur/uur-pw-capture"));
    }
    if let Ok(exe) = std::env::current_exe() {
        // dev checkout: target/debug/uur -> capture/uur-pw-capture
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../lib/uur/uur-pw-capture"));
            candidates.push(dir.join("../../capture/uur-pw-capture"));
            candidates.push(dir.join("uur-pw-capture"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("capture/uur-pw-capture"));
    }
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        anyhow!("capture helper uur-pw-capture not found (build with capture/build.sh)")
    })
}

#[cfg(test)]
mod tests {
    use super::source_type;
    use ashpd::desktop::screencast::SourceType;

    #[test]
    fn configured_portal_source_is_mapped() {
        assert_eq!(source_type("monitor"), SourceType::Monitor);
        assert_eq!(source_type("window"), SourceType::Window);
        assert_eq!(source_type("virtual"), SourceType::Virtual);
    }
}
