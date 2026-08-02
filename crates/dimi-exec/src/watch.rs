//! dimi-exec watch module — `hostFsWatchService.ts` mirror (M2, slice 4).
//!
//! Thin `notify`-backed watcher reporting raw create/modify/delete events
//! under an absolute path, normalizing notify's backend noise (FSEvents
//! duplicates, single-path renames) to the chokidar event surface the TS
//! layer emits:
//! - `Create(File|Folder)` → created, kind from the event
//! - `Modify(Data(_))` → modified / file (content changes only)
//! - `Remove(File|Folder)` → deleted, kind from the event
//! - `Modify(Name(Any))` → rename: stat-probe the single reported path —
//!   exists → created, missing → deleted; the kind for a missing path comes
//!   from an internal path-type cache (populated on create), default file.
//! - dedupe: identical (path, action, kind) within a short window is dropped
//!   (FSEvents re-reports the same change).
//! - `.git` path segments are filtered (the node-local DEFAULT_IGNORED).
//!
//! The `ignored` callback option stays on the TS adapter (JS function); the
//! default `.git` filter lives here.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use notify::{EventKind, RecursiveMode, Watcher};

/// Dedupe window — FSEvents re-reports the same change within ~100ms.
const DEDUPE_WINDOW: std::time::Duration = std::time::Duration::from_millis(200);

/// `HostFsChange` mirror (hostFsWatch.ts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsChange {
    pub path: String,
    pub action: &'static str,
    pub kind: &'static str,
}

/// `HostFsChangeAction` / `HostFsChangeKind` string unions.
pub const ACTION_CREATED: &str = "created";
pub const ACTION_MODIFIED: &str = "modified";
pub const ACTION_DELETED: &str = "deleted";
pub const KIND_FILE: &str = "file";
pub const KIND_DIRECTORY: &str = "directory";

/// Path type cache + dedupe state, shared between the notify callback
/// thread and `watch()`'s pump.
struct WatchState {
    /// path → last known kind ('file' | 'directory') for rename-From probes.
    kinds: HashMap<PathBuf, &'static str>,
    /// path → last emitted (action, kind) timestamp for dedupe.
    last_emit: HashMap<(PathBuf, &'static str, &'static str), Instant>,
    /// Paths that existed when the watch started — the chokidar
    /// `ignoreInitial` mirror. FSEvents re-reports pre-existing paths as
    /// `created` during the startup window (observed as late as ~1s after
    /// `watch()`), and chokidar never fires `add` for them.
    initial: HashSet<PathBuf>,
    /// Path → mtime at watch start (files only). FSEvents also re-reports the
    /// *content* change of pre-existing files (`Modify(Data)`) during the
    /// same startup window; a replay has the unchanged snapshot mtime and is
    /// suppressed, while a real modification (new mtime) still fires.
    initial_mtimes: HashMap<PathBuf, Option<std::time::SystemTime>>,
}

impl WatchState {
    fn new(
        initial: HashSet<PathBuf>,
        initial_mtimes: HashMap<PathBuf, Option<std::time::SystemTime>>,
    ) -> Self {
        Self {
            kinds: HashMap::new(),
            last_emit: HashMap::new(),
            initial,
            initial_mtimes,
        }
    }

    fn should_emit(&mut self, change: &FsChange) -> bool {
        let path = PathBuf::from(&change.path);
        let key = (path, change.action, change.kind);
        let now = Instant::now();
        if let Some(last) = self.last_emit.get(&key) {
            if now.duration_since(*last) < DEDUPE_WINDOW {
                return false;
            }
        }
        self.last_emit.insert(key, now);
        true
    }

    fn is_initial(&self, path: &str) -> bool {
        self.initial.contains(&PathBuf::from(path))
    }

    /// Whether `path` was snapshotted with exactly `mtime` at watch start.
    fn has_initial_mtime(&self, path: &str, mtime: Option<std::time::SystemTime>) -> bool {
        self.initial_mtimes
            .get(&PathBuf::from(path))
            .copied()
            .flatten()
            == mtime
    }

    fn forget_initial(&mut self, path: &str) {
        let path = PathBuf::from(path);
        self.initial.remove(&path);
        self.initial_mtimes.remove(&path);
    }

    fn remember_kind(&mut self, path: &str, kind: &'static str) {
        self.kinds.insert(PathBuf::from(path), kind);
    }

    fn forget_kind(&mut self, path: &str) {
        self.kinds.remove(&PathBuf::from(path));
    }

    fn known_kind(&self, path: &str) -> &'static str {
        self.kinds
            .get(&PathBuf::from(path))
            .copied()
            .unwrap_or(KIND_FILE)
    }
}

/// The chokidar `.git` ignore — any path segment named `.git`.
pub fn is_git_path(path: &str) -> bool {
    path.split(['/', '\\']).any(|segment| segment == ".git")
}

/// Map one notify event to zero, one or two chokidar-style changes.
fn map_event(kind: &EventKind, path: &std::path::Path, state: &mut WatchState) -> Vec<FsChange> {
    let path_str = path.to_string_lossy().into_owned();
    let mut changes = Vec::new();
    match kind {
        EventKind::Create(notify::event::CreateKind::File) => {
            if !state.is_initial(&path_str) {
                changes.push(FsChange {
                    path: path_str.clone(),
                    action: ACTION_CREATED,
                    kind: KIND_FILE,
                });
            }
            state.remember_kind(&path_str, KIND_FILE);
        }
        EventKind::Create(notify::event::CreateKind::Folder) => {
            if !state.is_initial(&path_str) {
                changes.push(FsChange {
                    path: path_str.clone(),
                    action: ACTION_CREATED,
                    kind: KIND_DIRECTORY,
                });
            }
            state.remember_kind(&path_str, KIND_DIRECTORY);
        }
        EventKind::Create(_) => {}
        EventKind::Modify(notify::event::ModifyKind::Data(_)) => {
            // FSEvents replays the content change of pre-existing files
            // during startup; the mtime is unchanged, so the replay is
            // suppressed. A real modification has a new mtime and fires.
            let replay = state.is_initial(&path_str)
                && state.has_initial_mtime(
                    &path_str,
                    path.metadata().ok().and_then(|m| m.modified().ok()),
                );
            if !replay {
                changes.push(FsChange {
                    path: path_str.clone(),
                    action: ACTION_MODIFIED,
                    kind: KIND_FILE,
                });
            }
        }
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
            // Rename: FSEvents reports one path per event — probe it.
            let exists = path.exists();
            if exists {
                let kind = if path.is_dir() {
                    KIND_DIRECTORY
                } else {
                    KIND_FILE
                };
                changes.push(FsChange {
                    path: path_str.clone(),
                    action: ACTION_CREATED,
                    kind,
                });
                state.remember_kind(&path_str, kind);
            } else {
                let kind = state.known_kind(&path_str);
                changes.push(FsChange {
                    path: path_str.clone(),
                    action: ACTION_DELETED,
                    kind,
                });
                state.forget_kind(&path_str);
                state.forget_initial(&path_str);
            }
        }
        EventKind::Modify(_) => {}
        EventKind::Remove(notify::event::RemoveKind::File) => {
            changes.push(FsChange {
                path: path_str.clone(),
                action: ACTION_DELETED,
                kind: KIND_FILE,
            });
            state.forget_kind(&path_str);
            state.forget_initial(&path_str);
        }
        EventKind::Remove(notify::event::RemoveKind::Folder) => {
            changes.push(FsChange {
                path: path_str.clone(),
                action: ACTION_DELETED,
                kind: KIND_DIRECTORY,
            });
            state.forget_kind(&path_str);
            state.forget_initial(&path_str);
        }
        EventKind::Remove(_) => {}
        EventKind::Any | EventKind::Other | EventKind::Access(_) => {}
    }
    changes
}

/// One watch session — mirror of `IHostFsWatchHandle`.
pub struct FsWatchHandle {
    rx: Mutex<Receiver<FsChange>>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

impl FsWatchHandle {
    /// Blocking read of the next normalized change (`None` = watcher died
    /// or `stop()` was called).
    pub fn recv(&self) -> Option<FsChange> {
        self.rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .recv()
            .ok()
    }

    /// Non-blocking drain of whatever is buffered so far.
    pub fn try_recv(&self) -> Option<FsChange> {
        self.rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_recv()
            .ok()
    }

    /// Stop the backend watcher. Dropping it releases the event callback
    /// closure (and its channel sender), so a blocked `recv()` returns
    /// `None` and the pump thread exits.
    pub fn stop(&self) {
        if let Ok(mut watcher) = self.watcher.lock() {
            watcher.take();
        }
    }
}

/// Map a notify-reported (physical) path back to the lexical watch view.
/// FSEvents reports `/private/var/...` for a watch rooted at `/var/...`;
/// chokidar resolves symlinks for its internal comparison but reports the
/// lexical path, so we strip the physical root prefix and re-apply the
/// lexical one. Falls back to the raw path when roots don't line up.
fn map_to_lexical(
    path: &std::path::Path,
    lexical_root: &std::path::Path,
    physical_root: &Option<PathBuf>,
) -> PathBuf {
    if let Some(physical) = physical_root {
        if let Ok(relative) = path.strip_prefix(physical) {
            return lexical_root.join(relative);
        }
    }
    path.to_path_buf()
}

/// Snapshot of everything that exists when the watch starts — the chokidar
/// `ignoreInitial` mirror. Includes the root itself (FSEvents re-reports it)
/// and skips `.git` segments (they are filtered from events anyway).
/// Directory symlinks are not followed (`followSymlinks: false` parity).
/// Returns the path set plus per-file mtimes used to suppress the startup
/// replay of content changes.
fn snapshot_paths(
    root: &std::path::Path,
    recursive: bool,
) -> (
    HashSet<PathBuf>,
    HashMap<PathBuf, Option<std::time::SystemTime>>,
) {
    let mut paths = HashSet::new();
    let mut mtimes = HashMap::new();
    paths.insert(root.to_path_buf());
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_git_path(&path.to_string_lossy()) {
                continue;
            }
            paths.insert(path.clone());
            mtimes.insert(
                path.clone(),
                entry.metadata().ok().and_then(|m| m.modified().ok()),
            );
            if recursive {
                let is_dir = path
                    .symlink_metadata()
                    .map(|meta| meta.is_dir())
                    .unwrap_or(false);
                if is_dir {
                    let (sub_paths, sub_mtimes) = snapshot_paths(&path, true);
                    paths.extend(sub_paths);
                    mtimes.extend(sub_mtimes);
                }
            }
        }
    }
    (paths, mtimes)
}

/// `watch(path, { recursive })` — chokidar `watch()` mirror. `recursive`
/// defaults to true (chokidar's default depth); `false` → depth 0.
pub fn watch(path: &str, recursive: bool) -> Result<FsWatchHandle, String> {
    // Lexical absolute root (chokidar's `path.resolve` view); physical root
    // has symlinks resolved so FSEvents' physical paths can be mapped back.
    let lexical_root = std::path::absolute(path).map_err(|error| error.to_string())?;
    let physical_root = std::fs::canonicalize(path).ok();
    // Snapshot BEFORE the watcher starts: FSEvents re-reports pre-existing
    // paths as `created` during the startup window, and the snapshot is what
    // suppresses them (`ignoreInitial` parity).
    let (initial, initial_mtimes) = snapshot_paths(&lexical_root, recursive);
    let (tx, rx) = channel::<FsChange>();
    let state = Arc::new(Mutex::new(WatchState::new(initial, initial_mtimes)));
    let mut watcher = notify::recommended_watcher({
        let state = Arc::clone(&state);
        let tx = tx.clone();
        let lexical_root = lexical_root.clone();
        let physical_root = physical_root.clone();
        move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
            let path = event
                .paths
                .first()
                .map(|p| map_to_lexical(p, &lexical_root, &physical_root))
                .unwrap_or_default();
            for change in map_event(&event.kind, &path, &mut state) {
                if is_git_path(&change.path) {
                    continue;
                }
                if state.should_emit(&change) {
                    let _ = tx.send(change);
                }
            }
        }
    })
    .map_err(|error| error.to_string())?;

    watcher
        .watch(
            std::path::Path::new(path),
            if recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            },
        )
        .map_err(|error| error.to_string())?;

    Ok(FsWatchHandle {
        rx: Mutex::new(rx),
        watcher: Mutex::new(Some(watcher)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dimi-watch-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn collect(handle: &FsWatchHandle, ms: u64) -> Vec<FsChange> {
        let mut changes = Vec::new();
        let deadline = Instant::now() + std::time::Duration::from_millis(ms);
        while Instant::now() < deadline {
            if let Some(change) = handle.try_recv() {
                changes.push(change);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        changes
    }

    #[test]
    fn reports_file_create_modify_delete() {
        let dir = temp_dir("basic");
        let handle = watch(&dir.to_string_lossy(), true).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300)); // settle

        let file = dir.join("a.txt");
        std::fs::write(&file, "hello").unwrap();
        std::fs::write(&file, "hello2").unwrap();
        std::fs::remove_file(&file).unwrap();

        let changes = collect(&handle, 1000);
        let actions: Vec<(&str, &str)> = changes.iter().map(|c| (c.action, c.kind)).collect();
        assert!(
            actions.contains(&(ACTION_CREATED, KIND_FILE)),
            "{actions:?}"
        );
        assert!(
            actions.contains(&(ACTION_MODIFIED, KIND_FILE)),
            "{actions:?}"
        );
        assert!(
            actions.contains(&(ACTION_DELETED, KIND_FILE)),
            "{actions:?}"
        );
        // The created path must be absolute.
        let created = changes.iter().find(|c| c.action == ACTION_CREATED).unwrap();
        assert!(created.path.ends_with("a.txt"), "{}", created.path);
        assert!(created.path.starts_with('/'), "{}", created.path);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn directory_create_and_delete_keep_kind() {
        let dir = temp_dir("dirs");
        let handle = watch(&dir.to_string_lossy(), true).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::remove_dir_all(&sub).unwrap();

        let changes = collect(&handle, 1000);
        assert!(
            changes
                .iter()
                .any(|c| c.action == ACTION_CREATED && c.kind == KIND_DIRECTORY),
            "{changes:?}"
        );
        assert!(
            changes
                .iter()
                .any(|c| c.action == ACTION_DELETED && c.kind == KIND_DIRECTORY),
            "{changes:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_maps_to_delete_then_create() {
        let dir = temp_dir("rename");
        let handle = watch(&dir.to_string_lossy(), true).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let from = dir.join("old.txt");
        let to = dir.join("new.txt");
        std::fs::write(&from, "x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(400));
        std::fs::rename(&from, &to).unwrap();

        let changes = collect(&handle, 1000);
        let deleted = changes
            .iter()
            .find(|c| c.action == ACTION_DELETED && c.path.ends_with("old.txt"));
        let created = changes
            .iter()
            .find(|c| c.action == ACTION_CREATED && c.path.ends_with("new.txt"));
        assert!(deleted.is_some(), "{changes:?}");
        assert!(created.is_some(), "{changes:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn git_paths_are_filtered() {
        assert!(is_git_path("/tmp/wt/.git"));
        assert!(is_git_path("/tmp/wt/a/.git/config"));
        assert!(!is_git_path("/tmp/wt/gitfile"));
        assert!(!is_git_path("/tmp/wt/.gitignore"));
    }

    #[test]
    fn git_events_are_dropped() {
        let dir = temp_dir("git");
        let handle = watch(&dir.to_string_lossy(), true).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));

        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git").join("config"), "x").unwrap();
        std::fs::write(dir.join("ok.txt"), "y").unwrap();

        let changes = collect(&handle, 1000);
        assert!(
            changes.iter().all(|c| !c.path.contains(".git")),
            "{changes:?}"
        );
        assert!(
            changes.iter().any(|c| c.path.ends_with("ok.txt")),
            "{changes:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pre_existing_files_are_not_reported_but_changes_are() {
        // chokidar `ignoreInitial` mirror: files present when the watch starts
        // never fire `created` (FSEvents re-reports them), but later changes do.
        let dir = temp_dir("initial");
        let file = dir.join("pre.txt");
        std::fs::write(&file, "v0").unwrap();

        let handle = watch(&dir.to_string_lossy(), true).unwrap();
        // Collect immediately and long — FSEvents re-reports pre-existing
        // files during the watch startup window (observed up to ~1s);
        // nothing about `pre.txt` may surface.
        let changes = collect(&handle, 2000);
        assert!(
            !changes.iter().any(|c| c.path.ends_with("pre.txt")),
            "{changes:?}"
        );

        // Modifications of pre-existing files still fire.
        std::fs::write(&file, "v1").unwrap();
        let changes = collect(&handle, 500);
        assert!(
            changes
                .iter()
                .any(|c| c.action == ACTION_MODIFIED && c.path.ends_with("pre.txt")),
            "{changes:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn initial_content_replay_is_suppressed_by_mtime() {
        // FSEvents re-reports the *content* change of a pre-existing file
        // (`Modify(Data)`) during startup; the file's mtime is unchanged, so
        // the replay must be dropped — but a real modification (new mtime)
        // still fires.
        let dir = temp_dir("mtime");
        let file = dir.join("pre.txt");
        std::fs::write(&file, "v0").unwrap();
        let path = std::path::Path::new(&file);

        let mut state = WatchState::new(
            HashSet::from([file.clone()]),
            HashMap::from([(
                file.clone(),
                path.metadata().ok().and_then(|m| m.modified().ok()),
            )]),
        );

        // Replay: same mtime as the snapshot → suppressed.
        let changes = map_event(
            &EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            path,
            &mut state,
        );
        assert!(changes.is_empty(), "{changes:?}");

        // Real modification: new mtime → reported.
        std::fs::write(&file, "v1").unwrap();
        let changes = map_event(
            &EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            path,
            &mut state,
        );
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].action, ACTION_MODIFIED);
        std::fs::remove_dir_all(&dir).ok();
    }
}
