//! Native PTY service for UU Remote's terminal feature.
//!
//! The Windows compatibility proxy keeps UU's existing ConPTY-facing stdio
//! contract. This service authenticates that proxy and attaches it to the
//! current Linux user's login shell through a real pseudoterminal.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MAGIC: u32 = 0x5555_5242;
const VERSION: u16 = 1;
const TOKEN_BYTES: usize = 64;
const MAX_FRAME: usize = 64 * 1024;
const MAX_SESSIONS: usize = 4;
const ACCEPTED: u8 = 0x06;
const FRAME_DATA: u8 = 1;
const FRAME_RESIZE: u8 = 2;
const FRAME_EOF: u8 = 3;

#[link(name = "util")]
unsafe extern "C" {
    fn openpty(
        master: *mut libc::c_int,
        slave: *mut libc::c_int,
        name: *mut libc::c_char,
        termios: *const libc::termios,
        winsize: *const libc::winsize,
    ) -> libc::c_int;
}

pub fn serve() -> Result<()> {
    let config_path = std::env::var_os("UUR_TERMINAL_CONFIG")
        .map(PathBuf::from)
        .context("UUR_TERMINAL_CONFIG is not set")?;
    let token = random_token()?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("binding terminal bridge")?;
    let port = listener.local_addr()?.port();
    write_runtime_config(&config_path, port, &token)?;

    let active = Arc::new(AtomicUsize::new(0));
    for connection in listener.incoming() {
        let Ok(stream) = connection else {
            continue;
        };
        if active.fetch_add(1, Ordering::AcqRel) >= MAX_SESSIONS {
            active.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        let token = token.clone();
        let active = active.clone();
        thread::spawn(move || {
            let _guard = SessionCount(active);
            if let Err(error) = handle_connection(stream, &token) {
                eprintln!("terminal adapter connection failed: {error:#}");
            }
        });
    }
    Ok(())
}

struct SessionCount(Arc<AtomicUsize>);

impl Drop for SessionCount {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn handle_connection(mut stream: TcpStream, expected_token: &str) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut hello = [0u8; 12];
    stream.read_exact(&mut hello)?;
    if u32::from_be_bytes(hello[0..4].try_into().unwrap()) != MAGIC
        || u16::from_be_bytes(hello[4..6].try_into().unwrap()) != VERSION
        || u16::from_be_bytes(hello[6..8].try_into().unwrap()) as usize != TOKEN_BYTES
    {
        anyhow::bail!("invalid terminal handshake");
    }
    let columns = u16::from_be_bytes(hello[8..10].try_into().unwrap()).clamp(20, 1000);
    let rows = u16::from_be_bytes(hello[10..12].try_into().unwrap()).clamp(5, 500);
    let mut supplied = [0u8; TOKEN_BYTES];
    stream.read_exact(&mut supplied)?;
    if !constant_time_equal(&supplied, expected_token.as_bytes()) {
        anyhow::bail!("terminal authentication failed");
    }
    stream.write_all(&[ACCEPTED])?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;

    let (mut master, mut child) = spawn_shell(columns, rows)?;
    let mut output = master.try_clone()?;
    let mut output_socket = stream.try_clone()?;
    let output_thread = thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        while let Ok(count) = output.read(&mut buffer) {
            if count == 0 || output_socket.write_all(&buffer[..count]).is_err() {
                break;
            }
        }
        let _ = output_socket.shutdown(Shutdown::Write);
    });

    loop {
        let mut header = [0u8; 8];
        match stream.read_exact(&mut header) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if child.try_wait()?.is_some() {
                    break;
                }
                continue;
            }
            Err(_) => break,
        }
        let frame_type = header[0];
        let length = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
        if length > MAX_FRAME {
            anyhow::bail!("terminal frame exceeds limit");
        }
        let mut payload = vec![0u8; length];
        stream.read_exact(&mut payload)?;
        match frame_type {
            FRAME_DATA => master.write_all(&payload)?,
            FRAME_RESIZE if payload.len() == 4 => {
                let columns = u16::from_be_bytes(payload[0..2].try_into().unwrap());
                let rows = u16::from_be_bytes(payload[2..4].try_into().unwrap());
                resize_pty(&master, columns, rows)?;
            }
            FRAME_EOF => break,
            _ => anyhow::bail!("invalid terminal frame"),
        }
    }

    unsafe {
        libc::kill(child.id() as i32, libc::SIGHUP);
    }
    let _ = child.wait();
    drop(master);
    let _ = output_thread.join();
    Ok(())
}

fn spawn_shell(columns: u16, rows: u16) -> Result<(File, std::process::Child)> {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let mut master_fd = -1;
    let mut slave_fd = -1;
    let result = unsafe {
        openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null(),
            &size,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("openpty");
    }

    let master = unsafe { File::from_raw_fd(master_fd) };
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|path| Path::new(path).is_absolute() && Path::new(path).is_file())
        .or_else(|| {
            option_env!("UUR_BUILD_SHELL")
                .filter(|path| Path::new(path).is_file())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "bash".to_string());
    let mut command = Command::new(shell);
    command
        .arg("-l")
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave.try_clone()?));
    if let Some(home) = std::env::var_os("HOME").filter(|path| Path::new(path).is_absolute()) {
        command.current_dir(home);
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().context("starting login shell")?;
    drop(slave);
    Ok((master, child))
}

fn resize_pty(master: &File, columns: u16, rows: u16) -> Result<()> {
    let size = libc::winsize {
        ws_row: rows.clamp(5, 500),
        ws_col: columns.clamp(20, 1000),
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) };
    if result < 0 {
        return Err(std::io::Error::last_os_error()).context("resizing PTY");
    }
    Ok(())
}

fn write_runtime_config(path: &Path, port: u16, token: &str) -> Result<()> {
    let parent = path.parent().context("terminal config has no parent")?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    writeln!(file, "version=1")?;
    writeln!(file, "port={port}")?;
    writeln!(file, "token={token}")?;
    file.sync_all()?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

fn random_token() -> Result<String> {
    let mut random = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_shape_and_comparison() {
        let token = random_token().unwrap();
        assert_eq!(token.len(), TOKEN_BYTES);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(constant_time_equal(token.as_bytes(), token.as_bytes()));
        assert!(!constant_time_equal(token.as_bytes(), &[b'0'; TOKEN_BYTES]));
    }

    #[test]
    fn pty_starts_a_real_shell() {
        let (mut pty, mut child) = spawn_shell(80, 24).unwrap();
        pty.write_all(b"printf UUR_PTY_OK\\n\nexit\n").unwrap();
        let mut output = Vec::new();
        pty.read_to_end(&mut output).ok();
        let _ = child.wait();
        assert!(String::from_utf8_lossy(&output).contains("UUR_PTY_OK"));
    }
}
