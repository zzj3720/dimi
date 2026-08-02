//! dimi-exec — OS process/exec layer (M2, slice 1: processes).
//!
//! Mirror of `hostProcessService.ts` (agent-core-v2
//! `os/backends/node-local/hostProcessService.ts`), the App-scope process
//! spawner every shell command, git call and rg probe goes through. Pure
//! std-only Rust; the napi bridge (`dimi-bridge`) owns the JS-facing
//! lifecycle (stream push via ThreadsafeFunction, async `wait`).
//!
//! Semantics mirrored from the TS implementation:
//! - `detached` defaults to `!isWindows`: on Unix the child is put in its
//!   own session/process group (`setsid`) so `kill(-pid)` terminates the
//!   whole tree; on Windows `taskkill /T /F` does the same.
//! - `mergeStderr` is a TS-adapter wiring concern: the child always gets two
//!   pipes here, and the adapter aliases the stderr view to stdout when the
//!   option is set.
//! - `timeout` is accepted in the options shape but NOT enforced — the TS
//!   implementation declares it in the interface and never consumes it.
//! - stdin/stdout/stderr are read continuously by background threads and
//!   handed out as channel receivers, so output stays readable after
//!   `wait()` returns (the TS `BufferedReadable` semantics).

mod process;

pub use process::{ExecProcess, ExecSpawnError, ShellSpec, SpawnOptions};

/// Spawn a child process. `command`/`args` follow Node `spawn` semantics;
/// `shell` is `None` (direct exec) or `Some(ShellSpec)`.
pub fn spawn(
    command: &str,
    args: &[String],
    options: &SpawnOptions,
) -> Result<ExecProcess, ExecSpawnError> {
    process::spawn(command, args, options)
}
