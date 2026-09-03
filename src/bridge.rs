use anyhow::{Context, Result};
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::thread;

use crate::config::Config;
use crate::input;
use crate::protocol::{self, RECORD_BYTES, RECORD_HELLO};

/// Print the X11 pointer position — the before/after observable for hook
/// verification.
pub fn print_pointer() -> Result<()> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;
    let (conn, screen) = x11rb::connect(None).context("connecting to X display")?;
    let root = conn.setup().roots[screen].root;
    let reply = conn.query_pointer(root)?.reply()?;
    println!("{} {}", reply.root_x, reply.root_y);
    Ok(())
}

/// Serve the in-client hook: one authenticated loopback connection at a
/// time, records dispatched to the selected input backend.
pub fn serve() -> Result<()> {
    let config = Config::load()?;
    let preferred =
        std::env::var("UUR_INPUT_BACKEND").unwrap_or_else(|_| config.input_backend.clone());
    let selected = input::select(&preferred)?;
    if let Some(path) = std::env::var_os("UUR_BACKEND_STATUS") {
        std::fs::write(path, selected.name()).context("writing backend status")?;
    }
    let backend = std::sync::Arc::new(std::sync::Mutex::new(selected));
    let backend_handle = backend.clone();
    let addr = format!("127.0.0.1:{}", config.bridge_port);
    let listener = TcpListener::bind(&addr).with_context(|| format!("binding {addr}"))?;
    println!("{}", t!("bridge.listening", port = config.bridge_port));

    // The service process hooks itself too, so multiple authenticated
    // connections coexist; each gets its own thread.  Injection is
    // serialized through the single backend.
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        let config = config.clone();
        let backend_handle = backend_handle.clone();
        thread::spawn(move || {
            let mut stream = stream;
            match authenticate(&mut stream, &config) {
                Ok(true) => {}
                Ok(false) => return,
                Err(_) => return,
            }
            let mut guard = backend_handle.lock().unwrap();
            println!("{}", t!("bridge.connected", backend = guard.name()));
            match pump(&mut stream, guard.as_mut()) {
                Ok(count) => {
                    println!("{}", t!("bridge.session_summary", count = count));
                    println!("{}", t!("bridge.disconnected_clean"));
                }
                Err(error) => {
                    eprintln!(
                        "{}",
                        t!("bridge.disconnected_error", error = error.to_string())
                    );
                }
            }
            let _ = guard.release_all();
        });
    }
    Ok(())
}

fn authenticate(stream: &mut TcpStream, config: &Config) -> Result<bool> {
    let mut header = [0u8; protocol::HEADER_BYTES];
    stream.read_exact(&mut header).context("reading header")?;
    let header = protocol::read_header(&header);
    if header.magic != protocol::MAGIC
        || header.version != protocol::VERSION
        || header.kind != RECORD_HELLO
    {
        return Ok(false);
    }
    let expected: [u8; 32] = decode_token(&config.bridge_token);
    let mut token = [0u8; 32];
    stream.read_exact(&mut token)?;
    if tokens_differ(&token, &expected) {
        let shown: String = token[..4].iter().map(|b| format!("{b:02x}")).collect();
        eprintln!("{}", t!("bridge.bad_token_fingerprint", prefix = shown));
        return Ok(false);
    }
    Ok(true)
}

fn pump(stream: &mut TcpStream, backend: &mut dyn input::InputBackend) -> Result<u64> {
    let mut header = [0u8; protocol::HEADER_BYTES];
    let mut record = [0u8; RECORD_BYTES];
    let mut injected: u64 = 0;
    loop {
        // EOF is the hook exiting: a clean disconnect, not an error.
        match stream.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(injected);
            }
            Err(error) => return Err(error.into()),
        }
        let header = protocol::read_header(&header);
        if header.magic != protocol::MAGIC {
            anyhow::bail!("bad magic");
        }
        match header.kind {
            protocol::RECORD_MOUSE | protocol::RECORD_KEYBOARD => {
                stream.read_exact(&mut record)?;
                let record = protocol::read_record(&record);
                dispatch(backend, &record)?;
                injected += 1;
            }
            _ => {
                // Unknown record: skip its payload if any, stay in sync.
                for _ in 0..(header.length / RECORD_BYTES as u32) {
                    stream.read_exact(&mut record)?;
                }
            }
        }
    }
}

fn dispatch(backend: &mut dyn input::InputBackend, record: &protocol::Record) -> Result<()> {
    match record.kind {
        protocol::RECORD_KEYBOARD => backend.key(record.code, record.state == 1)?,
        protocol::RECORD_MOUSE => {
            // a/b carry motion deltas or absolute coordinates; the wheel
            // selector reuses `code`.
            const MOTION: u16 = 0xffff;
            const WHEEL: u16 = 0xfffe;
            match record.code {
                MOTION => backend.motion(record.state == 1, record.a, record.b)?,
                WHEEL => backend.wheel(record.a != 0, record.b)?,
                _ => backend.button(record.code, record.state == 1)?,
            }
        }
        _ => {}
    }
    Ok(())
}

fn decode_token(token: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (index, chunk) in token.as_bytes().chunks(2).take(32).enumerate() {
        if let Ok(value) = u8::from_str_radix(std::str::from_utf8(chunk).unwrap_or("0"), 16) {
            out[index] = value;
        }
    }
    out
}

/// Branch-free comparison; the token never leaves loopback but avoid
/// handing timing data to same-user processes anyway.
fn tokens_differ(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff != 0
}
