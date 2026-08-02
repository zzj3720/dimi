//! Process spawn/kill/stdio — `hostProcessService.ts` mirror.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// `HostProcessOptions` (`hostProcess.ts` 19–29). `timeout` is accepted for
/// shape parity and deliberately unused (the TS implementation never
/// consumes it either).
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    pub cwd: Option<String>,
    /// Complete child env bag. `None` = inherit the parent env verbatim
    /// (Node: pass `undefined`); `Some` = the exact env (the caller is
    /// responsible for overlaying `process.env`, like the TS
    /// `SessionProcessRunner._buildExecEnv`).
    pub env: Option<HashMap<String, String>>,
    /// `shell: true | string` → run through a shell (`sh -c "…"`); `None` →
    /// exec `command` directly.
    pub shell: Option<ShellSpec>,
    /// `detached ?? !isWindows` — `None` means platform default: a new
    /// session/process group on Unix (so the whole tree dies with
    /// `kill(-pid)`), a plain spawn on Windows.
    pub detached: Option<bool>,
    pub windows_hide: bool,
}

/// `shell: boolean | string`.
#[derive(Debug, Clone)]
pub enum ShellSpec {
    /// `shell: true` — the platform default shell.
    Default,
    /// `shell: "/bin/bash"` — an explicit shell binary.
    Explicit(String),
}

/// One spawned process — mirror of the TS `IHostProcess` handle.
///
/// Background threads: one reader per pipe (blocking reads → channel) and
/// one waiter (`wait()` → exit code → channel). The bridge consumes the
/// receivers and forwards chunks to JS; because the readers run regardless
/// of JS consumption, output is buffered in memory until read — the
/// `BufferedReadable` "wait-then-read" semantics.
pub struct ExecProcess {
    pid: u32,
    /// `None` once the stdin handle has been closed/dropped.
    stdin: Mutex<Option<ChildStdin>>,
    stdout_rx: Mutex<Receiver<Vec<u8>>>,
    stderr_rx: Mutex<Receiver<Vec<u8>>>,
    exit_rx: Mutex<Receiver<i32>>,
    /// Set once the exit code is known (waiter thread or `wait()`). Kept
    /// separate from `exit_code` so `-1` stays a legitimate value: a process
    /// killed by a signal reports `Some(-1)` (`code ?? -1` in TS), and a
    /// concurrent `wait()` loser must never overwrite the winner's code.
    exited: Arc<AtomicBool>,
    exit_code: Arc<AtomicI32>,
    disposed: AtomicBool,
}

/// `HostProcessError` spawn failure (`os.process.spawn_failed`).
#[derive(Debug)]
pub struct ExecSpawnError {
    pub command: String,
    pub message: String,
    /// `errno`-style code when known (`ENOENT`, `EACCES`, …).
    pub errno: Option<String>,
}

impl std::fmt::Display for ExecSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to spawn \"{}\": {}", self.command, self.message)
    }
}

impl std::error::Error for ExecSpawnError {}

pub fn spawn(
    command: &str,
    args: &[String],
    options: &SpawnOptions,
) -> Result<ExecProcess, ExecSpawnError> {
    let (command, args) = match &options.shell {
        None => (command.to_owned(), args.to_vec()),
        Some(shell) => shell_command(shell, command, args)?,
    };

    let mut cmd = Command::new(&command);
    cmd.args(&args);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(cwd) = &options.cwd {
        cmd.current_dir(cwd);
    }
    if let Some(env) = &options.env {
        cmd.env_clear();
        for (key, value) in env {
            cmd.env(key, value);
        }
    }

    // Unix: new session/process group unless explicitly non-detached, so
    // `kill(-pid)` reaches the whole tree (hostProcessService.ts
    // `detached = options.detached ?? !isWindows`).
    // `detached ?? !isWindows` — platform default when unspecified.
    let detached = options.detached.unwrap_or(!cfg!(windows));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        if detached {
            // SAFETY: setsid is a plain libc call with no pointer args.
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        if detached {
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if options.windows_hide {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
    }

    let mut child = cmd.spawn().map_err(|error| ExecSpawnError {
        command: command.clone(),
        message: error.to_string(),
        errno: errno_code(&error),
    })?;

    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let (stdout_tx, stdout_rx) = channel::<Vec<u8>>();
    let (stderr_tx, stderr_rx) = channel::<Vec<u8>>();
    let (exit_tx, exit_rx) = channel::<i32>();
    let exited = Arc::new(AtomicBool::new(false));
    let exit_code = Arc::new(AtomicI32::new(0));

    let stdout_thread = reader_thread(stdout, stdout_tx, "stdout");
    let stderr_thread = reader_thread(stderr, stderr_tx, "stderr");
    let waiter_thread = {
        let exited = Arc::clone(&exited);
        let exit_code = Arc::clone(&exit_code);
        std::thread::spawn(move || {
            let code = child
                .wait()
                .map(|status| status.code().unwrap_or(-1))
                .unwrap_or(-1);
            // Store before sending: a `wait()` loser that re-reads the
            // atomic after a disconnected recv is guaranteed to see it.
            exit_code.store(code, Ordering::Release);
            exited.store(true, Ordering::Release);
            let _ = exit_tx.send(code);
        })
    };

    // Dropping the JoinHandles detaches the threads; the channels keep the
    // pumps alive until EOF/exit and `Send`/`Sync` stay satisfied.
    drop((stdout_thread, stderr_thread, waiter_thread));

    Ok(ExecProcess {
        pid,
        stdin: Mutex::new(stdin),
        stdout_rx: Mutex::new(stdout_rx),
        stderr_rx: Mutex::new(stderr_rx),
        exit_rx: Mutex::new(exit_rx),
        exited,
        exit_code,
        disposed: AtomicBool::new(false),
    })
}

fn reader_thread(
    mut stream: impl Read + Send + 'static,
    tx: Sender<Vec<u8>>,
    _name: &str,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    })
}

/// Node `spawn(..., { shell })` builds `<shell> -c "<command> <args…>"` and
/// lets the shell parse the whole string (documented Node behavior).
fn shell_command(
    shell: &ShellSpec,
    command: &str,
    args: &[String],
) -> Result<(String, Vec<String>), ExecSpawnError> {
    let (shell_path, dash_c) = match shell {
        ShellSpec::Default => {
            #[cfg(windows)]
            {
                (
                    std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_owned()),
                    "/c".to_owned(),
                )
            }
            #[cfg(unix)]
            {
                // Node: "Uses '/bin/sh' on UNIX" for `shell: true`. `$SHELL`
                // would pick zsh on macOS and change glob/builtin/`$0`
                // semantics vs the TS baseline.
                ("/bin/sh".to_owned(), "-c".to_owned())
            }
        }
        ShellSpec::Explicit(path) => {
            let dash_c = if path.to_ascii_lowercase().ends_with("cmd.exe") {
                "/c"
            } else {
                "-c"
            };
            (path.clone(), dash_c.to_owned())
        }
    };
    let mut joined = command.to_owned();
    for arg in args {
        joined.push(' ');
        joined.push_str(arg);
    }
    Ok((shell_path, vec![dash_c, joined]))
}

#[cfg(unix)]
fn errno_code(error: &std::io::Error) -> Option<String> {
    error.raw_os_error().map(|code| code.to_string())
}

#[cfg(not(unix))]
fn errno_code(_error: &std::io::Error) -> Option<String> {
    None
}

impl ExecProcess {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// `exitCode` — the exit code once the process exited, `null` before
    /// (TS `exitCode: number | null`). A signal-killed process reports
    /// `Some(-1)` (`code ?? -1`), so `-1` is a real value, not a sentinel.
    pub fn exit_code(&self) -> Option<i32> {
        self.exited
            .load(Ordering::Acquire)
            .then(|| self.exit_code.load(Ordering::Acquire))
    }

    /// `wait()` — block until exit; returns the exit code (`-1` when the
    /// status carried no code, mirroring `code ?? -1`). Repeatable and
    /// concurrent-safe: once the process has exited, later calls return the
    /// cached code, and racing callers both observe the winner's value.
    pub fn wait(&self) -> i32 {
        if self.exited.load(Ordering::Acquire) {
            return self.exit_code.load(Ordering::Acquire);
        }
        let code = self
            .exit_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .recv()
            // Concurrent-wait loser: the channel is already drained and
            // disconnected. The waiter thread stores the code before
            // sending it, so the atomic is guaranteed to be set here.
            .unwrap_or_else(|_| self.exit_code.load(Ordering::Acquire));
        self.exit_code.store(code, Ordering::Release);
        self.exited.store(true, Ordering::Release);
        code
    }

    /// Non-blocking drain of whatever stdout is buffered so far.
    pub fn try_recv_stdout(&self) -> Option<Vec<u8>> {
        self.stdout_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_recv()
            .ok()
    }

    /// Non-blocking drain of whatever stderr is buffered so far.
    pub fn try_recv_stderr(&self) -> Option<Vec<u8>> {
        self.stderr_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_recv()
            .ok()
    }

    /// Blocking read of the next stdout chunk (`None` = EOF).
    pub fn recv_stdout(&self) -> Option<Vec<u8>> {
        self.stdout_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .recv()
            .ok()
    }

    /// Blocking read of the next stderr chunk (`None` = EOF).
    pub fn recv_stderr(&self) -> Option<Vec<u8>> {
        self.stderr_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .recv()
            .ok()
    }

    /// Write to the child's stdin; `None` when stdin is closed.
    pub fn write_stdin(&self, bytes: &[u8]) -> Result<(), String> {
        let mut guard = self
            .stdin
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match guard.as_mut() {
            Some(stdin) => stdin.write_all(bytes).map_err(|e| e.to_string()),
            None => Err("stdin closed".to_owned()),
        }
    }

    /// Close the child's stdin (EOF for the child).
    pub fn close_stdin(&self) {
        let mut guard = self
            .stdin
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.take();
    }

    /// `kill(signal)` — terminate the process group. Signal is the numeric
    /// Unix signal; on Windows the signal is ignored and the tree is killed
    /// via `taskkill /T /F` (mirroring `hostProcessService.kill`).
    /// Deliberately NOT gated on `dispose()`: the TS baseline keeps `kill`
    /// functional after `dispose` (which only closes stdin).
    pub fn kill(&self, signal: Option<i32>) -> Result<(), String> {
        #[cfg(unix)]
        {
            let signal = signal.unwrap_or(libc::SIGTERM);
            // SAFETY: plain kill(2) call.
            let result = unsafe { libc::kill(-(self.pid as i32), signal) };
            if result == 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::ESRCH) => Ok(()), // already gone — silent, like TS
                Some(libc::EPERM) => {
                    // Fall back to killing just the child (TS falls back to
                    // `child.kill`).
                    // SAFETY: plain kill(2) call.
                    unsafe { libc::kill(self.pid as i32, signal) };
                    Ok(())
                }
                _ => Err(format!("Failed to kill process {}: {}", self.pid, error)),
            }
        }
        #[cfg(windows)]
        {
            self.kill_windows_tree()
        }
    }

    #[cfg(windows)]
    fn kill_windows_tree(&self) -> Result<(), String> {
        // Best-effort like TS: the killer's close/error always resolves.
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &self.pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Ok(())
    }

    /// `dispose()` — close stdin and stop accepting writes; the reader
    /// threads drain naturally once the child exits.
    pub fn dispose(&self) {
        if self.disposed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.close_stdin();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_echo(text: &str) -> ExecProcess {
        spawn("echo", &[text.to_owned()], &SpawnOptions::default()).unwrap()
    }

    #[test]
    fn spawn_wait_captures_stdout_and_exit_code() {
        let proc = spawn_echo("hello dimi");
        let mut out = Vec::new();
        while let Some(chunk) = proc.recv_stdout() {
            out.extend_from_slice(&chunk);
        }
        assert_eq!(proc.wait(), 0);
        assert_eq!(out, b"hello dimi\n");
        assert_eq!(proc.exit_code(), Some(0));
    }

    #[test]
    fn nonzero_exit_code_propagates() {
        let proc = spawn(
            "sh",
            &["-c".to_owned(), "exit 7".to_owned()],
            &SpawnOptions::default(),
        )
        .unwrap();
        assert_eq!(proc.wait(), 7);
        assert_eq!(proc.exit_code(), Some(7));
    }

    #[test]
    fn missing_command_reports_errno() {
        let error = match spawn(
            "definitely-not-a-command-xyz",
            &[],
            &SpawnOptions::default(),
        ) {
            Ok(_) => panic!("spawn should have failed"),
            Err(error) => error,
        };
        assert!(error.message.contains("No such file"), "{error}");
    }

    #[test]
    fn stderr_is_separate_channel() {
        let proc = spawn(
            "sh",
            &["-c".to_owned(), "echo out; echo err >&2".to_owned()],
            &SpawnOptions::default(),
        )
        .unwrap();
        let mut out = Vec::new();
        while let Some(chunk) = proc.recv_stdout() {
            out.extend_from_slice(&chunk);
        }
        let mut err = Vec::new();
        while let Some(chunk) = proc.recv_stderr() {
            err.extend_from_slice(&chunk);
        }
        proc.wait();
        assert_eq!(out, b"out\n");
        assert_eq!(err, b"err\n");
    }

    #[test]
    fn env_overrides_are_exact() {
        let mut env = HashMap::new();
        env.insert("DIMI_TEST_VAR".to_owned(), "42".to_owned());
        let proc = spawn(
            "sh",
            &["-c".to_owned(), "printf %s \"$DIMI_TEST_VAR\"".to_owned()],
            &SpawnOptions {
                env: Some(env),
                ..Default::default()
            },
        )
        .unwrap();
        let mut out = Vec::new();
        while let Some(chunk) = proc.recv_stdout() {
            out.extend_from_slice(&chunk);
        }
        proc.wait();
        assert_eq!(out, b"42");
    }

    #[test]
    fn cwd_is_respected() {
        // macOS `/tmp` is a symlink to `/private/tmp`; `pwd` prints the
        // physical path, so assert on the suffix.
        let proc = spawn(
            "pwd",
            &[],
            &SpawnOptions {
                cwd: Some("/tmp".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        let mut out = Vec::new();
        while let Some(chunk) = proc.recv_stdout() {
            out.extend_from_slice(&chunk);
        }
        proc.wait();
        let text = String::from_utf8_lossy(&out);
        assert!(text.ends_with("/tmp\n"), "pwd output: {text:?}");
    }

    #[cfg(unix)]
    #[test]
    fn kill_terminates_detached_tree() {
        // `sleep 30` in its own session; kill(-pid) must end it promptly.
        let proc = spawn("sleep", &["30".to_owned()], &SpawnOptions::default()).unwrap();
        assert!(proc.exit_code().is_none());
        proc.kill(Some(libc::SIGTERM)).unwrap();
        // kill + wait: exit code is not 0 (terminated by signal → None code → -1).
        assert_eq!(proc.wait(), -1);
        // `-1` is a real exit code, not the "not set" sentinel.
        assert_eq!(proc.exit_code(), Some(-1));
    }

    #[test]
    fn concurrent_wait_calls_agree() {
        let proc = Arc::new(spawn_echo("hello dimi"));
        let a = {
            let proc = Arc::clone(&proc);
            std::thread::spawn(move || proc.wait())
        };
        let b = {
            let proc = Arc::clone(&proc);
            std::thread::spawn(move || proc.wait())
        };
        assert_eq!(a.join().unwrap(), 0);
        assert_eq!(b.join().unwrap(), 0);
        // The loser must not have clobbered the winner's cached code.
        assert_eq!(proc.exit_code(), Some(0));
        assert_eq!(proc.wait(), 0);
    }

    #[test]
    fn stdin_write_reaches_child() {
        let proc = spawn("cat", &[], &SpawnOptions::default()).unwrap();
        proc.write_stdin(b"ping\n").unwrap();
        proc.close_stdin();
        let mut out = Vec::new();
        while let Some(chunk) = proc.recv_stdout() {
            out.extend_from_slice(&chunk);
        }
        proc.wait();
        assert_eq!(out, b"ping\n");
    }
}
