//! dimi-exec env module — `hostEnvironmentService.ts` + `environmentProbe.ts`
//! mirror (M2, slice 3).
//!
//! Probes the host OS / shell / path-style facts (`IHostEnvironment`). The
//! login-shell PATH enrichment is deliberately NOT migrated: it mutates
//! `process.env.PATH` on the Node side (a Node-specific side effect), so the
//! TS `applyLoginShellPathFromNode` stays the owner.
//!
//! Windows: the probe fields that std exposes (osKind/osArch/pathClass/homeDir)
//! work, but shell discovery (Git for Windows bash) and osVersion (kernel
//! release) are deferred to the Windows parity pass — see PLAN.md.

/// `HostEnvironmentInfo` mirror (hostEnvironment.ts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEnvironmentInfo {
    pub os_kind: String,
    pub os_arch: String,
    pub os_version: String,
    pub shell_name: String,
    pub shell_path: String,
    pub path_class: String,
    pub home_dir: String,
}

/// Node `process.arch` values (probe parity) — Rust `ARCH` differs
/// (`aarch64` vs `arm64`, `x86_64` vs `x64`, …).
pub fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        "powerpc64" => "ppc64",
        "loongarch64" => "loong64",
        other => other,
    }
}

/// `resolveOsKind(platform)` — darwin → macOS, linux → Linux, win32 →
/// Windows, anything else passes through.
pub fn os_kind() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    }
}

/// `pathClass` — win32 on Windows, posix elsewhere.
pub fn path_class() -> &'static str {
    if cfg!(windows) { "win32" } else { "posix" }
}

/// `os.release()` — the kernel release string (uname). Windows is deferred
/// (no std uname); returns a placeholder that the TS adapter tolerates until
/// the Windows parity pass.
#[cfg(unix)]
pub fn os_version() -> String {
    // SAFETY: uname is a plain libc struct-out call.
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: uts buffer is valid for the call.
    if unsafe { libc::uname(&mut uts) } != 0 {
        return String::new();
    }
    // `release` is a NUL-terminated char array; trim at the first NUL.
    let release = unsafe { std::ffi::CStr::from_ptr(uts.release.as_ptr()) };
    release.to_string_lossy().into_owned()
}

#[cfg(windows)]
pub fn os_version() -> String {
    // Deferred to the Windows parity pass (PLAN.md M2 slice notes).
    String::from("windows")
}

/// `os.homedir()` — `$HOME` on POSIX (Node: `$HOME` then passwd fallback).
pub fn home_dir() -> String {
    std::env::var("HOME").ok().unwrap_or_else(|| {
        std::env::home_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

/// The default shell path (bash when available, else `/bin/sh`) — the POSIX
/// branch of `environmentProbe.ts`. Used when a session does not specify a
/// shell (`SHELL` env + spawn target parity with the TS bash tool).
pub fn default_shell() -> String {
    probe_shell().1
}

/// `probeHostEnvironment` — the POSIX branch of environmentProbe.ts:
/// `/bin/bash` → `/usr/bin/bash` → `/usr/local/bin/bash`, else `sh`/`/bin/sh`.
/// Windows shell discovery (Git Bash) is deferred to the Windows pass.
#[cfg(unix)]
fn probe_shell() -> (String, String) {
    const CANDIDATES: [&str; 3] = ["/bin/bash", "/usr/bin/bash", "/usr/local/bin/bash"];
    for candidate in CANDIDATES {
        if std::path::Path::new(candidate).is_file() {
            return ("bash".to_owned(), candidate.to_owned());
        }
    }
    ("sh".to_owned(), "/bin/sh".to_owned())
}

#[cfg(windows)]
fn probe_shell() -> (String, String) {
    // Deferred to the Windows parity pass; /bin/sh placeholder keeps the
    // shape until git-bash discovery lands.
    ("sh".to_owned(), "/bin/sh".to_owned())
}

/// Full probe — the immutable `IHostEnvironment` snapshot.
pub fn probe() -> HostEnvironmentInfo {
    let (shell_name, shell_path) = probe_shell();
    HostEnvironmentInfo {
        os_kind: os_kind().to_owned(),
        os_arch: node_arch().to_owned(),
        os_version: os_version(),
        shell_name,
        shell_path,
        path_class: path_class().to_owned(),
        home_dir: home_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arch_maps_to_node_values() {
        // Node process.arch never returns 'aarch64'/'x86_64'.
        assert_ne!(node_arch(), "aarch64");
        assert_ne!(node_arch(), "x86_64");
        assert!(!node_arch().is_empty());
    }

    #[test]
    fn os_kind_matches_node_process_platform_mapping() {
        let kind = os_kind();
        match std::env::consts::OS {
            "macos" => assert_eq!(kind, "macOS"),
            "linux" => assert_eq!(kind, "Linux"),
            "windows" => assert_eq!(kind, "Windows"),
            _ => assert_eq!(kind, std::env::consts::OS),
        }
    }

    #[test]
    fn path_class_is_posix_on_unix() {
        #[cfg(unix)]
        assert_eq!(path_class(), "posix");
        #[cfg(windows)]
        assert_eq!(path_class(), "win32");
    }

    #[cfg(unix)]
    #[test]
    fn probe_finds_a_posix_shell() {
        let info = probe();
        assert_eq!(info.path_class, "posix");
        assert!(!info.home_dir.is_empty());
        assert!(!info.os_version.is_empty());
        assert!(info.shell_path.starts_with('/'));
        assert!(matches!(info.shell_name.as_str(), "bash" | "sh"));
        // macOS ships /bin/bash; if present it must be picked.
        if std::path::Path::new("/bin/bash").is_file() {
            assert_eq!(info.shell_name, "bash");
            assert_eq!(info.shell_path, "/bin/bash");
        }
    }

    #[cfg(unix)]
    #[test]
    fn default_shell_prefers_bash() {
        let shell = default_shell();
        assert!(shell.starts_with('/'), "{shell}");
        // The TS probe chain prefers bash; /bin/sh is only the fallback.
        if std::path::Path::new("/bin/bash").is_file() {
            assert_eq!(shell, "/bin/bash");
        } else {
            assert_eq!(shell, "/bin/sh");
        }
    }

    #[cfg(unix)]
    #[test]
    fn os_version_is_kernel_release() {
        let version = os_version();
        // Kernel releases look like '24.1.0' / '6.8.0-49-generic'.
        assert!(
            version.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "{version}"
        );
    }
}
