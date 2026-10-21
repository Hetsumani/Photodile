//! Single-instance application coordination and localhost IPC for Photodile.
//!
//! When multiple images are selected in Windows File Explorer and opened with Photodile,
//! Explorer launches concurrent separate processes (one per file). This module ensures:
//! 1. Exactly one primary instance creates the application window.
//! 2. Secondary instances detect the primary, forward their file paths via localhost IPC,
//!    and exit cleanly with 0 windows.
//! 3. The primary instance batches forwarded paths and appends them to the filmstrip.
//! 4. If a previous instance crashed, stale locks are automatically recovered.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// Message received from secondary instances via localhost IPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcMessage {
    /// File paths forwarded to be opened in the filmstrip.
    Paths(Vec<PathBuf>),
    /// Request to bring the existing window to the foreground.
    Focus,
}

/// The role assigned to this process invocation.
pub enum InstanceRole {
    /// This process is the primary instance and should start the UI.
    Primary(SingleInstanceServer),
    /// Another instance is already running; file paths were forwarded to it.
    Secondary,
}

/// Server guard holding the IPC background listener and single-instance lock file.
/// When dropped, the lock file is cleanly removed.
pub struct SingleInstanceServer {
    lock_file: PathBuf,
    rx: Option<Receiver<IpcMessage>>,
    _listener_thread: Option<thread::JoinHandle<()>>,
}

impl SingleInstanceServer {
    /// Takes the IPC message receiver to forward incoming files to the Slint UI thread.
    pub fn take_receiver(&mut self) -> Option<Receiver<IpcMessage>> {
        self.rx.take()
    }
}

impl Drop for SingleInstanceServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_file);
    }
}

/// Normalizes a file path: converts relative paths to absolute and strips
/// Windows verbatim `\\?\` prefixes for consistent downstream codec/Slint handling.
pub fn normalize_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    let trimmed = s.trim_matches('"').trim();
    let p = Path::new(trimmed);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(p)
    } else {
        p.to_path_buf()
    };

    #[cfg(windows)]
    {
        let s = abs.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    abs
}

/// Computes the per-user lock file path in the OS temporary directory.
pub fn get_lock_file_path() -> PathBuf {
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "default".to_string());
    let safe_user: String = username
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    std::env::temp_dir().join(format!("photodile_ipc_{}.lock", safe_user))
}

/// Generates a pseudo-random 64-bit authentication token to verify localhost connections.
fn generate_token() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let mut val = (now as u64) ^ ((pid as u64) << 32);
    val ^= val >> 30;
    val = val.wrapping_mul(0xbf58476d1ce4e5b9);
    val ^= val >> 27;
    val = val.wrapping_mul(0x94d049bb133111eb);
    val ^= val >> 31;
    format!("{:016x}{:08x}", val, pid)
}

#[derive(Debug, PartialEq, Eq)]
enum ForwardResult {
    Delivered,
    StaleLock,
}

/// Attempts to forward the given paths to the existing primary instance.
fn try_forward_to_primary(lock_path: &Path, paths: &[PathBuf]) -> ForwardResult {
    let is_old = std::fs::metadata(lock_path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(3))
        .unwrap_or(false);

    let deadline = Instant::now() + Duration::from_millis(1500);

    while Instant::now() < deadline {
        if let Ok(content) = std::fs::read_to_string(lock_path) {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() >= 2 {
                let port_str = lines[0].trim();
                let token = lines[1].trim();

                if let Ok(port) = port_str.parse::<u16>() {
                    match TcpStream::connect_timeout(
                        &SocketAddr::from(([127, 0, 0, 1], port)),
                        Duration::from_millis(200),
                    ) {
                        Ok(mut stream) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

                            let mut payload = format!("PHOTODILE_IPC_V1 {}\n", token);
                            for path in paths {
                                let path_str = path.to_string_lossy();
                                let encoded = urlencoding::encode(&path_str);
                                payload.push_str(&format!("OPEN {}\n", encoded));
                            }
                            payload.push_str("DONE\n");

                            if stream.write_all(payload.as_bytes()).is_ok() {
                                let mut reader = BufReader::new(stream);
                                let mut ack = String::new();
                                if reader.read_line(&mut ack).is_ok() && ack.trim() == "OK" {
                                    return ForwardResult::Delivered;
                                }
                            }
                        }
                        Err(_) => {
                            // If lock file was created > 3 seconds ago and connection is refused,
                            // the primary instance definitively crashed.
                            if is_old {
                                return ForwardResult::StaleLock;
                            }
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(25));
    }

    ForwardResult::StaleLock
}

/// Processes a single secondary client connection.
fn handle_client(mut stream: TcpStream, expected_token: &str, tx: &Sender<IpcMessage>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let mut reader = BufReader::new(&stream);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).is_err() {
        return;
    }

    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() != 2 || parts[0] != "PHOTODILE_IPC_V1" || parts[1] != expected_token {
        let _ = stream.write_all(b"ERR_UNAUTHORIZED\n");
        return;
    }

    let mut received_paths = Vec::new();
    let mut line = String::new();
    let mut has_done = false;

    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed == "DONE" {
            has_done = true;
            break;
        } else if let Some(encoded_path) = trimmed.strip_prefix("OPEN ") {
            if let Ok(decoded) = urlencoding::decode(encoded_path) {
                let path = PathBuf::from(decoded.into_owned());
                if path.exists() {
                    received_paths.push(normalize_path(&path));
                }
            }
        }
        line.clear();
    }

    if has_done {
        let _ = stream.write_all(b"OK\n");
        let _ = stream.flush();

        if !received_paths.is_empty() {
            let _ = tx.send(IpcMessage::Paths(received_paths));
        } else {
            let _ = tx.send(IpcMessage::Focus);
        }
    }
}

/// Starts the loopback TCP listener to receive paths from secondary instances.
fn run_listener(listener: TcpListener, expected_token: String, tx: Sender<IpcMessage>) {
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let tx = tx.clone();
                let token = expected_token.clone();
                thread::spawn(move || {
                    handle_client(s, &token, &tx);
                });
            }
            Err(_) => {
                // Listener closed, terminate thread
                break;
            }
        }
    }
}

/// Coordinates single-instance startup:
/// - If this is the first instance: acquires the lock file, starts loopback IPC listener,
///   and returns `InstanceRole::Primary`.
/// - If another instance is running: connects, forwards `paths`, and returns `InstanceRole::Secondary`.
/// - If a stale lock exists: safely removes it and elects this instance as primary.
pub fn init(paths: &[PathBuf]) -> Result<InstanceRole, Box<dyn std::error::Error>> {
    let lock_path = get_lock_file_path();

    for _attempt in 0..3 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(mut file) => {
                // Primary election won! Bind loopback listener.
                let listener = match TcpListener::bind("127.0.0.1:0") {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = std::fs::remove_file(&lock_path);
                        return Err(Box::new(e));
                    }
                };
                let port = listener.local_addr()?.port();
                let token = generate_token();
                let pid = std::process::id();

                let content = format!("{}\n{}\n{}\n", port, token, pid);
                if let Err(e) = file.write_all(content.as_bytes()) {
                    let _ = std::fs::remove_file(&lock_path);
                    return Err(Box::new(e));
                }
                let _ = file.sync_all();

                let (tx, rx) = mpsc::channel();
                let listener_token = token.clone();
                let thread_handle = thread::spawn(move || {
                    run_listener(listener, listener_token, tx);
                });

                return Ok(InstanceRole::Primary(SingleInstanceServer {
                    lock_file: lock_path,
                    rx: Some(rx),
                    _listener_thread: Some(thread_handle),
                }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // An existing lock file was found. Try forwarding paths.
                match try_forward_to_primary(&lock_path, paths) {
                    ForwardResult::Delivered => {
                        return Ok(InstanceRole::Secondary);
                    }
                    ForwardResult::StaleLock => {
                        // Stale lock detected, remove and re-attempt election.
                        let _ = std::fs::remove_file(&lock_path);
                        continue;
                    }
                }
            }
            Err(e) => {
                // If creating in temp dir fails for OS permissions or filesystem reasons,
                // fail-open and launch as primary without IPC so user is never blocked.
                eprintln!("Warning: failed to open IPC lock file: {}", e);
                let (_tx, rx) = mpsc::channel();
                return Ok(InstanceRole::Primary(SingleInstanceServer {
                    lock_file: lock_path,
                    rx: Some(rx),
                    _listener_thread: None,
                }));
            }
        }
    }

    // Fallback if all attempts exhausted: launch as primary
    let (_tx, rx) = mpsc::channel();
    Ok(InstanceRole::Primary(SingleInstanceServer {
        lock_file: lock_path,
        rx: Some(rx),
        _listener_thread: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        let p = Path::new("Cargo.toml");
        let norm = normalize_path(p);
        assert!(norm.is_absolute());
        assert!(norm.ends_with("Cargo.toml"));
    }

    #[test]
    fn test_token_generation() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert_ne!(t1, t2);
        assert_eq!(t1.len(), 24);
    }

    #[test]
    fn test_ipc_communication_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let port = listener.local_addr().unwrap().port();
        let token = "test_secret_token_123";

        let (tx, rx) = mpsc::channel();
        let token_clone = token.to_string();
        thread::spawn(move || {
            run_listener(listener, token_clone, tx);
        });

        // Secondary connects
        let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port)))
            .expect("connect to test listener");

        let test_file = std::env::current_dir().unwrap().join("Cargo.toml");
        let binding = test_file.to_string_lossy();
        let encoded = urlencoding::encode(&binding);
        let payload = format!("PHOTODILE_IPC_V1 {}\nOPEN {}\nDONE\n", token, encoded);
        stream.write_all(payload.as_bytes()).unwrap();

        let mut reader = BufReader::new(stream);
        let mut ack = String::new();
        reader.read_line(&mut ack).unwrap();
        assert_eq!(ack.trim(), "OK");

        let msg = rx.recv_timeout(Duration::from_secs(1)).expect("receive message");
        match msg {
            IpcMessage::Paths(paths) => {
                assert_eq!(paths.len(), 1);
                assert_eq!(paths[0], normalize_path(&test_file));
            }
            IpcMessage::Focus => panic!("expected Paths"),
        }
    }
}