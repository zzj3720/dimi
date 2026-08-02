//! dimi-exec — OS execution layer (M2): processes and filesystem.
//!
//! Mirror of the agent-core-v2 `os/` domain:
//! - processes (`hostProcessService.ts`) — the App-scope spawner every shell
//!   command, git call and rg probe goes through;
//! - filesystem (`hostFsService.ts`) — the real-disk `IHostFileSystem`
//!   primitives used by persistence, skill loading and the file tools.
//!
//! The napi bridge (`dimi-bridge`) owns the JS-facing lifecycle (stream
//! push via ThreadsafeFunction, async `wait`). std + libc + notify; no
//! tokio.
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
//!   handed out as bounded channel receivers (4 × 64KB per pipe), so a slow
//!   consumer back-pressures the child through the kernel pipe buffer
//!   instead of buffering unboundedly.
//! - `readText` without options keeps the BOM and decodes leniently
//!   (`readFile(path, 'utf8')`); with options it follows
//!   `decodeTextWithErrors` (TextDecoder semantics).

pub mod env;
pub mod fs;
mod process;
pub mod pty;
pub mod watch;

pub use env::HostEnvironmentInfo;
pub use fs::{
    DecodeErrors, DirEntry, Encoding, FileStat, FsError, ReadLines, ReadTextOptions, append_text,
    create_exclusive, decode_with_errors, errno_name, lstat, mkdir, read_bytes, read_text, readdir,
    realpath, remove, stat, write_bytes, write_text,
};
pub use process::{ExecProcess, ExecSpawnError, ShellSpec, SpawnOptions};
pub use pty::{PtyProcess, PtySpawnOptions, spawn as pty_spawn};
pub use watch::{FsChange, FsWatchHandle, watch};

/// Spawn a child process. `command`/`args` follow Node `spawn` semantics;
/// `shell` is `None` (direct exec) or `Some(ShellSpec)`.
pub fn spawn(
    command: &str,
    args: &[String],
    options: &SpawnOptions,
) -> Result<ExecProcess, ExecSpawnError> {
    process::spawn(command, args, options)
}
