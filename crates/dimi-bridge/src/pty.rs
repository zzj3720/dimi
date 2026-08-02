//! M2 pty bridge — `RustTerminal` / `RustTerminalProcess` napi surface over
//! `dimi-exec::pty`, the swap-in socket for the App-scope
//! `IHostTerminalService` (`hostTerminalService.ts`).
//!
//! Event delivery mirrors `RustFsWatchHandle`: pump threads forward output
//! chunks and the exit event through ThreadsafeFunctions (`CalleeHandled =
//! false`); `kill()` mirrors node-pty's default signal.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dimi_exec::pty as pty_core;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;

/// `TerminalSpawnOptions` mirror (terminal.ts) — `env` is the full process
/// environment (the TS adapter passes `process.env`).
#[napi(object)]
pub struct RustTerminalSpawnOptions {
    pub cwd: String,
    pub shell: String,
    pub cols: u32,
    pub rows: u32,
    pub env: HashMap<String, String>,
}

/// `onProcessExit` payload mirror (terminal.ts).
#[napi(object)]
pub struct RustTerminalExit {
    /// `None` when the shell was killed by a signal.
    pub exit_code: Option<i32>,
}

/// `CalleeHandled = false` — the callback receives the value directly.
type DataCallback = ThreadsafeFunction<String, Unknown<'static>, String, Status, false>;
type ExitCallback =
    ThreadsafeFunction<RustTerminalExit, Unknown<'static>, RustTerminalExit, Status, false>;

/// `RustTerminalProcess` — one pty session (`TerminalProcess` mirror).
#[napi]
pub struct RustTerminalProcess {
    inner: Arc<pty_core::PtyProcess>,
    data_started: AtomicBool,
    exit_started: AtomicBool,
}

#[napi]
impl RustTerminalProcess {
    /// Wire the output pump. Call once: `onData(chunk)` fires per output
    /// chunk until the pty closes.
    #[napi]
    pub fn set_on_data(&self, on_data: DataCallback) -> napi::Result<()> {
        if self.data_started.swap(true, Ordering::AcqRel) {
            return Err(napi::Error::from_reason(
                "setOnData may only be called once per handle",
            ));
        }
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || {
            while let Some(data) = inner.recv_data() {
                let _ = on_data.call(data, ThreadsafeFunctionCallMode::NonBlocking);
            }
        });
        Ok(())
    }

    /// Wire the exit pump. Call once: `onExit({ exitCode })` fires when the
    /// shell exits.
    #[napi]
    pub fn set_on_exit(&self, on_exit: ExitCallback) -> napi::Result<()> {
        if self.exit_started.swap(true, Ordering::AcqRel) {
            return Err(napi::Error::from_reason(
                "setOnExit may only be called once per handle",
            ));
        }
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || {
            if let Some(code) = inner.recv_exit() {
                let _ = on_exit.call(
                    RustTerminalExit { exit_code: code },
                    ThreadsafeFunctionCallMode::NonBlocking,
                );
            }
        });
        Ok(())
    }

    /// Write input to the pty (node-pty `write`).
    #[napi]
    pub fn write(&self, data: String) -> napi::Result<()> {
        self.inner.write(&data).map_err(napi_error)
    }

    /// Resize the pty (node-pty `resize`).
    #[napi]
    pub fn resize(&self, cols: u32, rows: u32) -> napi::Result<()> {
        self.inner
            .resize(cols as u16, rows as u16)
            .map_err(napi_error)
    }

    /// Kill the shell (node-pty `kill`; SIGHUP on unix).
    #[napi]
    pub fn kill(&self) {
        self.inner.kill();
    }
}

/// `RustTerminal` — stateless facade over `dimi-exec::pty`.
#[napi]
pub struct RustTerminal;

#[napi]
impl RustTerminal {
    /// `spawn({cwd, shell, cols, rows, env})` — spawn a shell in a fresh pty.
    #[napi]
    pub fn spawn(options: RustTerminalSpawnOptions) -> napi::Result<RustTerminalProcess> {
        let inner = pty_core::spawn(pty_core::PtySpawnOptions {
            cwd: options.cwd,
            shell: options.shell,
            cols: options.cols as u16,
            rows: options.rows as u16,
            env: options.env.into_iter().collect(),
        })
        .map_err(napi_error)?;
        Ok(RustTerminalProcess {
            inner: Arc::new(inner),
            data_started: AtomicBool::new(false),
            exit_started: AtomicBool::new(false),
        })
    }
}

fn napi_error(error: String) -> napi::Error {
    napi::Error::from_reason(format!("terminal.spawn failed: {error}"))
}
