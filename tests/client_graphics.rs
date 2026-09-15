//! End-to-end checks for how a real client process writes Kitty graphics to
//! its terminal: direct cursor-positioned placements outside tmux, and
//! `DCS tmux;` passthrough with unicode-placeholder virtual placements when the
//! client starts inside a tmux pane.

#![cfg(unix)]

pub mod support;

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};
use support::{
    cleanup_test_base, register_runtime_dir, register_spawned_herdr_pid,
    unregister_spawned_herdr_pid, wait_for_socket,
};

/// UTF-8 encoding of `U+10EEEE`, the Kitty unicode placeholder character.
const PLACEHOLDER_UTF8: &[u8] = "\u{10EEEE}".as_bytes();
const TMUX_PASSTHROUGH_APC: &[u8] = b"\x1bPtmux;\x1b\x1b_G";

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!(
        "/tmp/herdr-client-graphics-test-{}-{nanos}",
        std::process::id()
    ))
}

fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct SpawnedHerdr {
    master: Option<Box<dyn MasterPty + Send>>,
    child: Box<dyn Child + Send + Sync>,
}

impl Drop for SpawnedHerdr {
    fn drop(&mut self) {
        let pid = self.child.process_id();
        let _ = self.child.kill();
        drop(self.master.take());
        if let Some(pid) = pid {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                let mut status = 0;
                let result =
                    unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
                if result == pid as libc::pid_t || result == -1 {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            unregister_spawned_herdr_pid(Some(pid));
        }
    }
}

fn base_command(config_home: &Path, runtime_dir: &Path, api_socket_path: &Path) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_herdr"));
    cmd.env("HERDR_DISABLE_SOUND", "1");
    cmd.env("XDG_STATE_HOME", runtime_dir.join("state"));
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd.env("XDG_RUNTIME_DIR", runtime_dir);
    cmd.env("HERDR_SOCKET_PATH", api_socket_path);
    // Debug builds read `herdr-dev/config.toml`; name the file explicitly so
    // both processes see the onboarding opt-out and the pane stays uncovered.
    cmd.env("HERDR_CONFIG_PATH", config_home.join("herdr/config.toml"));
    cmd.env_remove("HERDR_CLIENT_SOCKET_PATH");
    cmd.env_remove("HERDR_ENV");
    cmd.env_remove("HERDR_PANE_ID");
    cmd.env("SHELL", "/bin/sh");
    cmd
}

fn spawn_server(config_home: &Path, runtime_dir: &Path, api_socket_path: &Path) -> SpawnedHerdr {
    fs::create_dir_all(config_home.join("herdr")).unwrap();
    fs::create_dir_all(runtime_dir).unwrap();
    register_runtime_dir(runtime_dir);
    fs::write(
        config_home.join("herdr/config.toml"),
        "onboarding = false\n",
    )
    .unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = base_command(config_home, runtime_dir, api_socket_path);
    cmd.arg("server");
    let child = pair.slave.spawn_command(cmd).unwrap();
    register_spawned_herdr_pid(child.process_id());
    drop(pair.slave);
    SpawnedHerdr {
        master: Some(pair.master),
        child,
    }
}

/// The terminal a client believes it runs in: outside tmux with a plain
/// xterm identity, or inside a tmux pane.
enum HostTerminal {
    Plain,
    Tmux,
}

/// Spawn a client in a PTY that reports an 8x16 px cell grid through the
/// window-size ioctl, the way tmux and real terminals do, so the client
/// learns its cell size without answering escape queries.
fn spawn_client(
    config_home: &Path,
    runtime_dir: &Path,
    api_socket_path: &Path,
    host: HostTerminal,
) -> SpawnedHerdr {
    register_runtime_dir(runtime_dir);
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 640,
            pixel_height: 384,
        })
        .unwrap();
    let mut cmd = base_command(config_home, runtime_dir, api_socket_path);
    cmd.arg("client");
    cmd.env_remove("KITTY_WINDOW_ID");
    cmd.env_remove("STY");
    match host {
        HostTerminal::Plain => {
            cmd.env("TERM", "xterm-256color");
            cmd.env_remove("TERM_PROGRAM");
            cmd.env_remove("TMUX");
            cmd.env_remove("TMUX_PANE");
        }
        HostTerminal::Tmux => {
            cmd.env("TERM", "tmux-256color");
            cmd.env("TERM_PROGRAM", "tmux");
            cmd.env("TMUX", "/tmp/herdr-fake-tmux-socket,1,0");
            cmd.env("TMUX_PANE", "%0");
        }
    }
    let child = pair.slave.spawn_command(cmd).unwrap();
    register_spawned_herdr_pid(child.process_id());
    drop(pair.slave);
    SpawnedHerdr {
        master: Some(pair.master),
        child,
    }
}

/// Everything the client wrote to its terminal so far, collected off-thread so
/// a full PTY buffer never stalls the client.
fn capture_output(client: &SpawnedHerdr) -> Arc<Mutex<Vec<u8>>> {
    let mut reader = client
        .master
        .as_ref()
        .expect("client master")
        .try_clone_reader()
        .expect("clone client reader");
    let output = Arc::new(Mutex::new(Vec::new()));
    let sink = output.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut sink) = sink.lock() {
                        sink.extend_from_slice(&buf[..n]);
                    }
                }
            }
        }
    });
    output
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn wait_for_output(
    output: &Arc<Mutex<Vec<u8>>>,
    timeout: Duration,
    mut predicate: impl FnMut(&[u8]) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if output.lock().is_ok_and(|bytes| predicate(&bytes)) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn send_json_request(socket_path: &Path, id: &str, method: &str, params: Value) -> Value {
    let mut stream = UnixStream::connect(socket_path).expect("should connect to API socket");
    let request = json!({ "id": id, "method": method, "params": params });
    writeln!(stream, "{request}").unwrap();
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).unwrap();
    serde_json::from_str(&response).expect("response should be valid JSON")
}

fn first_pane_id(socket_path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let response = send_json_request(socket_path, "pane_list", "pane.list", json!({}));
        if let Some(pane_id) = response
            .pointer("/result/panes/0/pane_id")
            .and_then(Value::as_str)
        {
            return pane_id.to_owned();
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("server never reported a pane");
}

/// Push a 2x1 RGBA image over the first pane's top-left 4x2 cells.
fn set_pane_graphics(socket_path: &Path, pane_id: &str) {
    let response = send_json_request(
        socket_path,
        "graphics_set",
        "pane.graphics.set",
        json!({
            "pane_id": pane_id,
            "format": "rgba",
            "image_width": 2,
            "image_height": 1,
            "data_base64": "AQIDBAUGBwg=",
            "placement": {
                "viewport_col": 0,
                "viewport_row": 0,
                "grid_cols": 4,
                "grid_rows": 2
            }
        }),
    );
    assert!(
        response.get("error").is_none(),
        "pane.graphics.set failed: {response}"
    );
}

fn run_client_graphics_scenario(host: HostTerminal) -> Vec<u8> {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("herdr.sock");
    let client_socket = runtime_dir.join("herdr-client.sock");

    let server = spawn_server(&config_home, &runtime_dir, &api_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let client = spawn_client(&config_home, &runtime_dir, &api_socket, host);
    let output = capture_output(&client);
    assert!(
        wait_for_output(&output, Duration::from_secs(10), |bytes| {
            contains(bytes, b"\x1b[?2026h")
        }),
        "client should draw a synchronized frame after attaching"
    );

    let pane_id = first_pane_id(&api_socket);
    set_pane_graphics(&api_socket, &pane_id);
    let painted = wait_for_output(&output, Duration::from_secs(10), |bytes| {
        contains(bytes, b"\x1b_G")
    });
    let bytes = output.lock().map(|bytes| bytes.clone()).unwrap_or_default();
    assert!(
        painted,
        "client should write Kitty graphics for the pane layer: {:?}",
        String::from_utf8_lossy(&bytes)
    );
    // Give the client a moment to finish the transaction (upload then place).
    let _ = wait_for_output(&output, Duration::from_secs(5), |bytes| {
        contains(bytes, b"a=p,") || contains(bytes, b"a=T,")
    });
    let bytes = output.lock().map(|bytes| bytes.clone()).unwrap_or_default();

    drop(client);
    drop(server);
    cleanup_test_base(&base);
    bytes
}

#[test]
fn client_outside_tmux_writes_direct_cursor_positioned_placements() {
    let bytes = run_client_graphics_scenario(HostTerminal::Plain);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        contains(&bytes, b"\x1b_Ga=") && contains(&bytes, b"C=1"),
        "direct placements carry the cursor-move flag: {text:?}"
    );
    assert!(
        !contains(&bytes, TMUX_PASSTHROUGH_APC),
        "no tmux passthrough outside tmux: {text:?}"
    );
    assert!(
        !contains(&bytes, b"U=1"),
        "no virtual placements outside tmux: {text:?}"
    );
    assert!(
        !contains(&bytes, PLACEHOLDER_UTF8),
        "no placeholder cells outside tmux: {text:?}"
    );
    let placement = bytes
        .windows(b"\x1b_Ga=".len())
        .position(|window| window == b"\x1b_Ga=")
        .expect("a Kitty command");
    let before = &bytes[placement.saturating_sub(16)..placement];
    assert!(
        contains(before, b"H") && contains(before, b"\x1b["),
        "a cursor move precedes the direct placement: {:?}",
        String::from_utf8_lossy(before)
    );
}

#[test]
fn client_inside_tmux_wraps_graphics_in_passthrough_with_unicode_placeholders() {
    let bytes = run_client_graphics_scenario(HostTerminal::Tmux);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        contains(&bytes, TMUX_PASSTHROUGH_APC),
        "Kitty commands are wrapped in DCS tmux; passthrough: {text:?}"
    );
    assert!(
        contains(&bytes, b"U=1"),
        "placements are virtual (U=1): {text:?}"
    );
    assert!(
        contains(&bytes, PLACEHOLDER_UTF8),
        "placeholder cells are drawn into the frame: {text:?}"
    );
    assert!(
        !contains(&bytes, b"C=1"),
        "no cursor-positioned placements inside tmux: {text:?}"
    );
    // Every Kitty command must be wrapped: an unwrapped APC start is one that
    // is not preceded by the doubled-escape passthrough prefix.
    let mut index = 0;
    while let Some(offset) = bytes[index..]
        .windows(3)
        .position(|window| window == b"\x1b_G")
    {
        let start = index + offset;
        let wrapped = start >= 8 && &bytes[start - 8..start + 3] == TMUX_PASSTHROUGH_APC;
        // Inside the passthrough the APC introducer is `ESC ESC _ G`; the
        // scan above lands on its inner `ESC _ G`.
        assert!(
            wrapped,
            "unwrapped Kitty command at {start}: {:?}",
            String::from_utf8_lossy(
                &bytes[start.saturating_sub(12)..(start + 24).min(bytes.len())]
            )
        );
        index = start + 3;
    }
}
