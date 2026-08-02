//! M2 fs bridge — `RustFileSystem` / `RustReadLines` napi surface over
//! `dimi-exec::fs`, the swap-in socket for the App-scope `IHostFileSystem`
//! (`hostFsService.ts`).
//!
//! Every method is async (spawn_blocking on the napi tokio runtime) around a
//! synchronous std::fs call. Errors are formatted as
//! `"{ERRNO} {op} failed: {message}"` (see `FsError::fmt`); the TS adapter
//! extracts the leading errno symbol and maps it through `toHostFsError`.

use std::sync::Arc;
use std::sync::Mutex;

use dimi_exec::fs as fs_core;
use napi::bindgen_prelude::*;
use napi_derive::napi;

/// `readText` / `readLines` options — `BufferEncoding` + `TextDecodeErrors`.
#[napi(object)]
#[derive(Default)]
pub struct RustReadTextOptions {
    pub encoding: Option<String>,
    pub errors: Option<String>,
}

/// `HostFileStat` mirror (`isSymbolicLink`/`mtimeMs`/`ino` optional like TS).
#[napi(object)]
pub struct RustFileStat {
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: Option<bool>,
    pub size: f64,
    pub mtime_ms: Option<f64>,
    pub ino: Option<f64>,
}

impl From<fs_core::FileStat> for RustFileStat {
    fn from(stat: fs_core::FileStat) -> Self {
        Self {
            is_file: stat.is_file,
            is_directory: stat.is_directory,
            is_symbolic_link: Some(stat.is_symbolic_link),
            size: stat.size as f64,
            mtime_ms: stat.mtime_ms,
            ino: stat.ino.map(|ino| ino as f64),
        }
    }
}

/// `HostDirEntry` mirror.
#[napi(object)]
pub struct RustDirEntry {
    pub name: String,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: Option<bool>,
}

impl From<fs_core::DirEntry> for RustDirEntry {
    fn from(entry: fs_core::DirEntry) -> Self {
        Self {
            name: entry.name,
            is_file: entry.is_file,
            is_directory: entry.is_directory,
            is_symbolic_link: Some(entry.is_symbolic_link),
        }
    }
}

/// `spawn_blocking` with the JoinError mapped into a napi error.
async fn spawn_blocking<F, T>(f: F) -> napi::Result<T>
where
    F: FnOnce() -> napi::Result<T> + Send + 'static,
    T: Send + 'static,
{
    napi::tokio::task::spawn_blocking(f)
        .await
        .map_err(|error| napi::Error::from_reason(error.to_string()))?
}

/// `RustFileSystem` — stateless facade over `dimi-exec::fs`, mirroring every
/// `IHostFileSystem` method.
#[napi]
pub struct RustFileSystem;

#[napi]
impl RustFileSystem {
    #[napi]
    pub async fn read_text(
        path: String,
        options: Option<RustReadTextOptions>,
    ) -> napi::Result<String> {
        let options = options.map(|o| fs_core::ReadTextOptions {
            encoding: o.encoding,
            errors: o.errors,
        });
        spawn_blocking(move || fs_core::read_text(&path, options.as_ref()).map_err(napi_error))
            .await
    }

    #[napi]
    pub async fn write_text(path: String, data: String) -> napi::Result<()> {
        spawn_blocking(move || fs_core::write_text(&path, &data).map_err(napi_error)).await
    }

    #[napi]
    pub async fn append_text(path: String, data: String) -> napi::Result<()> {
        spawn_blocking(move || fs_core::append_text(&path, &data).map_err(napi_error)).await
    }

    #[napi]
    pub async fn read_bytes(path: String, n: Option<u32>) -> napi::Result<Buffer> {
        spawn_blocking(move || {
            fs_core::read_bytes(&path, n.map(|n| n as usize))
                .map(Buffer::from)
                .map_err(napi_error)
        })
        .await
    }

    #[napi]
    pub async fn write_bytes(path: String, data: Buffer) -> napi::Result<()> {
        spawn_blocking(move || fs_core::write_bytes(&path, &data).map_err(napi_error)).await
    }

    /// `readLines` — returns a handle whose `next()` yields one line at a
    /// time (`null` at EOF); the TS adapter wraps it in an `AsyncGenerator`.
    #[napi]
    pub async fn read_lines(
        path: String,
        options: Option<RustReadTextOptions>,
    ) -> napi::Result<RustReadLines> {
        let options = options.map(|o| fs_core::ReadTextOptions {
            encoding: o.encoding,
            errors: o.errors,
        });
        spawn_blocking(move || RustReadLines::open(&path, options.as_ref())).await
    }

    #[napi]
    pub async fn create_exclusive(path: String, data: Buffer) -> napi::Result<bool> {
        spawn_blocking(move || fs_core::create_exclusive(&path, &data).map_err(napi_error)).await
    }

    #[napi]
    pub async fn stat(path: String) -> napi::Result<RustFileStat> {
        spawn_blocking(move || {
            fs_core::stat(&path)
                .map(RustFileStat::from)
                .map_err(napi_error)
        })
        .await
    }

    #[napi]
    pub async fn lstat(path: String) -> napi::Result<RustFileStat> {
        spawn_blocking(move || {
            fs_core::lstat(&path)
                .map(RustFileStat::from)
                .map_err(napi_error)
        })
        .await
    }

    #[napi]
    pub async fn readdir(path: String) -> napi::Result<Vec<RustDirEntry>> {
        spawn_blocking(move || {
            fs_core::readdir(&path)
                .map(|entries| entries.into_iter().map(RustDirEntry::from).collect())
                .map_err(napi_error)
        })
        .await
    }

    #[napi]
    pub async fn mkdir(path: String, recursive: Option<bool>) -> napi::Result<()> {
        spawn_blocking(move || {
            fs_core::mkdir(&path, recursive.unwrap_or(false)).map_err(napi_error)
        })
        .await
    }

    #[napi]
    pub async fn remove(path: String) -> napi::Result<()> {
        spawn_blocking(move || fs_core::remove(&path).map_err(napi_error)).await
    }

    #[napi]
    pub async fn realpath(path: String) -> napi::Result<String> {
        spawn_blocking(move || fs_core::realpath(&path).map_err(napi_error)).await
    }
}

/// `RustReadLines` — the `readLines` async-iterator handle. The generator
/// state machine lives on the Rust side (bounded 64KB streaming); `next()`
/// reads one chunk per call under a mutex, so concurrent `next()` calls are
/// serialized instead of interleaving file offsets.
#[napi]
pub struct RustReadLines {
    inner: Arc<Mutex<Option<fs_core::ReadLines>>>,
    path: String,
}

#[napi]
impl RustReadLines {
    fn open(path: &str, options: Option<&fs_core::ReadTextOptions>) -> napi::Result<Self> {
        let encoding = options.and_then(|o| o.encoding.as_deref());
        let errors = options.and_then(|o| o.errors.as_deref());
        let lines = fs_core::ReadLines::open(path, encoding, errors).map_err(napi_error)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Some(lines))),
            path: path.to_owned(),
        })
    }

    /// Next line including its `\n` terminator; `null` at EOF.
    #[napi]
    pub async fn next(&self) -> napi::Result<Option<String>> {
        let path = self.path.clone();
        let inner = Arc::clone(&self.inner);
        spawn_blocking(move || {
            let mut guard = inner.lock().unwrap_or_else(|poison| poison.into_inner());
            match guard.as_mut() {
                Some(lines) => lines.next_line(&path).map_err(napi_error),
                None => Ok(None),
            }
        })
        .await
    }

    /// Drop the file handle early (the generator is done).
    #[napi]
    pub fn dispose(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.take();
        }
    }
}

fn napi_error(error: fs_core::FsError) -> napi::Error {
    napi::Error::from_reason(error.to_string())
}
