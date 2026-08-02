//! dimi-exec pty module — `hostTerminalService.ts` mirror (M2, slice 5).
//!
//! `portable-pty`-backed interactive terminal spawn mirroring the node-pty
//! surface the TS layer consumes:
//! - `spawn({cwd, shell, cols, rows, env})` → `PtyProcess`
//! - `PtyProcess` streams output via a channel (pump thread over the pty
//!   master reader), reports exit via a second channel, and forwards
//!   `write` / `resize` / `kill`.
//!
//! The `kill`/`wait` split: the exit pump polls `try_wait` every 50ms so
//! `kill()` (which needs the same `Child` handle) never blocks on a running
//! process.

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};

/// Poll interval for the exit pump.
const EXIT_POLL: Duration = Duration::from_millis(50);

/// `TerminalSpawnOptions` mirror (terminal.ts).
pub struct PtySpawnOptions {
    pub cwd: String,
    pub shell: String,
    pub cols: u16,
    pub rows: u16,
    /// Full environment (the TS adapter passes `process.env`).
    pub env: Vec<(String, String)>,
}

/// `TerminalProcess` mirror (terminal.ts) — one pty session.
pub struct PtyProcess {
    data_rx: Mutex<Receiver<String>>,
    exit_rx: Mutex<Receiver<Option<i32>>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>,
}

impl PtyProcess {
    /// Blocking read of the next output chunk (`None` = pump ended).
    pub fn recv_data(&self) -> Option<String> {
        self.data_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .recv()
            .ok()
    }

    /// Non-blocking drain of buffered output.
    pub fn try_recv_data(&self) -> Option<String> {
        self.data_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_recv()
            .ok()
    }

    /// Blocking read of the exit code (`None` = killed by a signal or
    /// wait failed; `Some(code)` = normal exit).
    pub fn recv_exit(&self) -> Option<Option<i32>> {
        self.exit_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .recv()
            .ok()
    }

    /// Non-blocking exit-code peek.
    pub fn try_recv_exit(&self) -> Option<Option<i32>> {
        self.exit_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_recv()
            .ok()
    }

    /// Write input to the pty (node-pty `write`).
    pub fn write(&self, data: &str) -> Result<(), String> {
        let mut writer = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        writer
            .write_all(data.as_bytes())
            .and_then(|_| writer.flush())
            .map_err(|error| format!("pty write failed: {error}"))
    }

    /// Resize the pty (node-pty `resize`).
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let master = self.master.lock().unwrap_or_else(|p| p.into_inner());
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("pty resize failed: {error}"))
    }

    /// Kill the shell. Mirrors node-pty `kill()`; the default signal is
    /// SIGHUP on unix (node-pty parity), SIGKILL on Windows.
    pub fn kill(&self) {
        #[cfg(unix)]
        {
            if let Some(pid) = self
                .child
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .and_then(|c| c.process_id())
            {
                // SIGHUP matches node-pty's default kill signal; the child
                // handle stays owned by the exit pump, which reaps it.
                unsafe {
                    libc::kill(pid as i32, libc::SIGHUP);
                }
            }
        }
        #[cfg(not(unix))]
        {
            let mut guard = self.child.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

/// `IHostTerminalService.spawn` mirror — spawn a login shell in a fresh pty.
pub fn spawn(options: PtySpawnOptions) -> Result<PtyProcess, String> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: options.rows,
            cols: options.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("pty open failed: {error}"))?;

    let mut cmd = CommandBuilder::new(&options.shell);
    cmd.cwd(&options.cwd);
    for (key, value) in &options.env {
        cmd.env(key, value);
    }
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|error| format!("pty spawn failed: {error}"))?;
    // The slave end is owned by the child; closing our copy is required for
    // the master reader to observe EOF on exit.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("pty reader failed: {error}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("pty writer failed: {error}"))?;

    let (data_tx, data_rx) = channel::<String>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                    if data_tx.send(chunk).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let child = Arc::new(Mutex::new(Some(child)));
    let (exit_tx, exit_rx) = channel::<Option<i32>>();
    {
        let child = Arc::clone(&child);
        std::thread::spawn(move || {
            let status = loop {
                let mut guard = child.lock().unwrap_or_else(|p| p.into_inner());
                match guard.as_mut().and_then(|c| c.try_wait().ok()).flatten() {
                    Some(status) => break status,
                    None => {
                        drop(guard);
                        std::thread::sleep(EXIT_POLL);
                    }
                }
            };
            // Signal-terminated processes report no exit code (`signal()`
            // is set, `code` is a placeholder) — mirror the process slice's
            // `-1` sentinel as `None`.
            let code = if status.signal().is_some() {
                None
            } else {
                Some(status.exit_code() as i32)
            };
            let _ = exit_tx.send(code);
        });
    }

    Ok(PtyProcess {
        data_rx: Mutex::new(data_rx),
        exit_rx: Mutex::new(exit_rx),
        writer: Mutex::new(writer),
        master: Mutex::new(pair.master),
        child,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dimi-pty-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn shell() -> String {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())
    }

    fn env() -> Vec<(String, String)> {
        std::env::vars().collect()
    }

    fn collect_data(pty: &PtyProcess, ms: u64) -> String {
        let mut out = String::new();
        let deadline = std::time::Instant::now() + Duration::from_millis(ms);
        while std::time::Instant::now() < deadline {
            if let Some(chunk) = pty.try_recv_data() {
                out.push_str(&chunk);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        out
    }

    #[test]
    fn spawn_writes_and_reads_output() {
        let dir = temp_dir("basic");
        let pty = spawn(PtySpawnOptions {
            cwd: dir.to_string_lossy().into_owned(),
            shell: shell(),
            cols: 80,
            rows: 24,
            env: env(),
        })
        .unwrap();
        pty.write("echo pty-hello\n").unwrap();
        let out = collect_data(&pty, 2000);
        assert!(out.contains("pty-hello"), "{out:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exit_code_is_reported() {
        let dir = temp_dir("exit");
        let pty = spawn(PtySpawnOptions {
            cwd: dir.to_string_lossy().into_owned(),
            shell: shell(),
            cols: 80,
            rows: 24,
            env: env(),
        })
        .unwrap();
        pty.write("exit 42\n").unwrap();
        let code = pty.recv_exit().expect("exit event").expect("exit code");
        assert_eq!(code, 42);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cwd_is_respected() {
        let dir = temp_dir("cwd");
        let pty = spawn(PtySpawnOptions {
            cwd: dir.to_string_lossy().into_owned(),
            shell: shell(),
            cols: 80,
            rows: 24,
            env: env(),
        })
        .unwrap();
        pty.write("pwd\n").unwrap();
        let out = collect_data(&pty, 2000);
        let pwd_line = out.lines().map(str::trim).find(|l| l.starts_with('/'));
        assert!(pwd_line.is_some(), "{out:?}");
        assert!(
            pwd_line
                .unwrap()
                .ends_with(format!("dimi-pty-cwd-{}", std::process::id()).as_str())
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resize_reaches_the_shell() {
        let dir = temp_dir("resize");
        let pty = spawn(PtySpawnOptions {
            cwd: dir.to_string_lossy().into_owned(),
            shell: shell(),
            cols: 80,
            rows: 24,
            env: env(),
        })
        .unwrap();
        pty.resize(100, 40).unwrap();
        pty.write("stty size\n").unwrap();
        let out = collect_data(&pty, 2000);
        assert!(out.contains("100 40") || out.contains("40 100"), "{out:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn kill_terminates_the_shell() {
        let dir = temp_dir("kill");
        let pty = spawn(PtySpawnOptions {
            cwd: dir.to_string_lossy().into_owned(),
            shell: shell(),
            cols: 80,
            rows: 24,
            env: env(),
        })
        .unwrap();
        pty.write("sleep 30\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        pty.kill();
        let exit = pty.recv_exit().expect("exit event after kill");
        assert!(
            exit.is_none(),
            "signal-killed shell reports no exit code: {exit:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn env_is_forwarded() {
        let dir = temp_dir("env");
        let mut options = PtySpawnOptions {
            cwd: dir.to_string_lossy().into_owned(),
            shell: shell(),
            cols: 80,
            rows: 24,
            env: env(),
        };
        options
            .env
            .push(("DIMI_PTY_PROBE".to_owned(), "rust-pty".to_owned()));
        let pty = spawn(options).unwrap();
        pty.write("echo $DIMI_PTY_PROBE\n").unwrap();
        let out = collect_data(&pty, 2000);
        assert!(out.contains("rust-pty"), "{out:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
