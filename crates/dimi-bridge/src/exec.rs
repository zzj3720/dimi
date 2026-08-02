//! M2 exec bridge — `RustHostProcessService` / `RustHostProcess` napi
//! surface over `dimi-exec`, the swap-in socket for the App-scope
//! `IHostProcessService` (hostProcessService.ts).
//!
//! Stream wiring: dimi-exec's background reader threads fill std mpsc
//! channels; each `RustHostProcess` runs two pump threads that forward
//! chunks to JS through ThreadsafeFunctions (`onStdout`/`onStderr`) and a
//! final no-arg call when the pipe hits EOF, so the TS adapter can
//! `push(null)` its Readables. `wait()` is async (spawn_blocking on the
//! napi tokio runtime); `kill`/`dispose`/`writeStdin` are sync fast paths
//! (write goes through `spawn_blocking`-free direct write — the pipe write
//! is non-blocking enough for the interactive sizes; the TS `Writable`
//! adapter owns backpressure).

use std::collections::HashMap;
use std::sync::Arc;

use dimi_exec::{ExecProcess, ShellSpec, SpawnOptions};
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;

/// Chunk callback: receives the raw bytes with no error slot.
/// `CalleeHandled = false` — otherwise the default true prepends a `null`
/// error argument (Node async convention) to every JS callback invocation.
type ChunkCallback = ThreadsafeFunction<Vec<u8>, Unknown<'static>, Vec<u8>, Status, false>;
/// EOF callback: no arguments at all.
type EndCallback = ThreadsafeFunction<(), Unknown<'static>, (), Status, false>;

/// `HostProcessOptions` — `shell` is split into `shellDefault` (true) and
/// `shellPath` (explicit binary) because napi has no `boolean | string`.
/// `mergeStderr` is a TS-adapter wiring concern (the adapter aliases stderr
/// to the stdout view), so it is not part of the bridge surface.
#[napi(object)]
#[derive(Default)]
pub struct RustHostProcessOptions {
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub shell_default: Option<bool>,
    pub shell_path: Option<String>,
    pub detached: Option<bool>,
    pub windows_hide: Option<bool>,
}

#[napi]
pub struct RustHostProcess {
    inner: Arc<ExecProcess>,
    stream_threads: Vec<std::thread::JoinHandle<()>>,
    disposed: std::sync::atomic::AtomicBool,
}

#[napi]
impl RustHostProcess {
    /// `IHostProcessService.spawn(command, args, options)`.
    #[napi]
    pub fn spawn(
        command: String,
        args: Vec<String>,
        options: Option<RustHostProcessOptions>,
    ) -> napi::Result<Self> {
        let options = options.unwrap_or_default();
        let shell = match (options.shell_default, options.shell_path) {
            (Some(true), _) => Some(ShellSpec::Default),
            (_, Some(path)) => Some(ShellSpec::Explicit(path)),
            _ => None,
        };
        let spawn_options = SpawnOptions {
            cwd: options.cwd,
            env: options.env,
            shell,
            detached: options.detached,
            windows_hide: options.windows_hide.unwrap_or(true),
        };
        let inner = dimi_exec::spawn(&command, &args, &spawn_options).map_err(|error| {
            napi::Error::from_reason(format!(
                "Failed to spawn \"{}\": {}",
                error.command, error.message
            ))
        })?;
        Ok(Self {
            inner: Arc::new(inner),
            stream_threads: Vec::new(),
            disposed: std::sync::atomic::AtomicBool::new(false),
        })
    }

    #[napi(getter)]
    pub fn pid(&self) -> u32 {
        self.inner.pid()
    }

    #[napi(getter)]
    pub fn exit_code(&self) -> Option<i32> {
        self.inner.exit_code()
    }

    /// Wire the stream pump callbacks. Must be called exactly once, right
    /// after construction: `onStdout(chunk)` / `onStderr(chunk)` fire per
    /// pipe chunk, `onStdoutEnd()` / `onStderrEnd()` once at EOF.
    /// Positional args — napi-rs object structs need `ToNapiValue`, which
    /// `ThreadsafeFunction` does not implement.
    #[napi]
    pub fn set_stream_callbacks(
        &mut self,
        on_stdout: ChunkCallback,
        on_stderr: ChunkCallback,
        on_stdout_end: EndCallback,
        on_stderr_end: EndCallback,
    ) -> napi::Result<()> {
        if !self.stream_threads.is_empty() {
            return Err(napi::Error::from_reason(
                "setStreamCallbacks may only be called once",
            ));
        }
        let stdout_thread =
            self.pump_thread("stdout", Arc::clone(&self.inner), on_stdout, on_stdout_end);
        let stderr_thread =
            self.pump_thread("stderr", Arc::clone(&self.inner), on_stderr, on_stderr_end);
        self.stream_threads.push(stdout_thread);
        self.stream_threads.push(stderr_thread);
        Ok(())
    }

    fn pump_thread(
        &self,
        name: &'static str,
        inner: Arc<ExecProcess>,
        chunk_tsfn: ChunkCallback,
        end_tsfn: EndCallback,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            loop {
                let chunk = if name == "stdout" {
                    inner.recv_stdout()
                } else {
                    inner.recv_stderr()
                };
                match chunk {
                    Some(bytes) => {
                        let _ = chunk_tsfn.call(bytes, ThreadsafeFunctionCallMode::NonBlocking);
                    }
                    None => {
                        let _ = end_tsfn.call((), ThreadsafeFunctionCallMode::NonBlocking);
                        break;
                    }
                }
            }
        })
    }

    /// `wait()` — resolves with the exit code (`-1` when the process was
    /// killed by a signal, mirroring `code ?? -1`).
    #[napi]
    pub async fn wait(&self) -> napi::Result<i32> {
        let inner = Arc::clone(&self.inner);
        napi::tokio::task::spawn_blocking(move || inner.wait())
            .await
            .map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    /// `kill(signal?)` — terminates the process tree.
    #[napi]
    pub fn kill(&self, signal: Option<String>) -> napi::Result<()> {
        let signal = signal.as_deref().map(signal_to_number);
        self.inner.kill(signal).map_err(napi::Error::from_reason)
    }

    /// Write one chunk to the child's stdin (the TS `Writable` adapter calls
    /// this from `_write`).
    #[napi]
    pub fn write_stdin(&self, chunk: Buffer) -> napi::Result<()> {
        self.inner
            .write_stdin(&chunk)
            .map_err(napi::Error::from_reason)
    }

    /// Close the child's stdin (EOF).
    #[napi]
    pub fn close_stdin(&self) {
        self.inner.close_stdin();
    }

    #[napi]
    pub fn dispose(&self) {
        if self
            .disposed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        self.inner.dispose();
    }
}

/// `NodeJS.Signals` → numeric signal (defaults to SIGTERM).
fn signal_to_number(signal: &str) -> i32 {
    let name = signal
        .strip_prefix("SIG")
        .unwrap_or(signal)
        .to_ascii_uppercase();
    match name.as_str() {
        "TERM" => libc::SIGTERM,
        "KILL" => libc::SIGKILL,
        "INT" => libc::SIGINT,
        "HUP" => libc::SIGHUP,
        "QUIT" => libc::SIGQUIT,
        "ABRT" => libc::SIGABRT,
        "USR1" => libc::SIGUSR1,
        "USR2" => libc::SIGUSR2,
        "STOP" => libc::SIGSTOP,
        "CONT" => libc::SIGCONT,
        "PIPE" => libc::SIGPIPE,
        "ALRM" => libc::SIGALRM,
        _ => libc::SIGTERM,
    }
}
