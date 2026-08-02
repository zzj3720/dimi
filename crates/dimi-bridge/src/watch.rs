//! M2 watch bridge — `RustFsWatch` / `RustFsWatchHandle` napi surface over
//! `dimi-exec::watch`, the swap-in socket for the App-scope
//! `IHostFsWatchService` (`hostFsWatchService.ts`).
//!
//! Event delivery mirrors `RustHostProcess`: a pump thread forwards
//! normalized changes through a ThreadsafeFunction (`CalleeHandled = false`,
//! one `RustFsChange` argument); `dispose()` stops the watcher.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dimi_exec::watch as watch_core;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;

/// `HostFsChange` mirror (hostFsWatch.ts).
#[napi(object)]
pub struct RustFsChange {
    pub path: String,
    pub action: String,
    pub kind: String,
}

impl From<watch_core::FsChange> for RustFsChange {
    fn from(change: watch_core::FsChange) -> Self {
        Self {
            path: change.path,
            action: change.action.to_owned(),
            kind: change.kind.to_owned(),
        }
    }
}

/// `HostFsWatchOptions` — `recursive` only (the `ignored` callback is a JS
/// function and stays on the TS adapter; the default `.git` filter is baked
/// into the Rust core).
#[napi(object)]
#[derive(Default)]
pub struct RustFsWatchOptions {
    pub recursive: Option<bool>,
}

/// `RustFsWatch` — stateless facade over `dimi-exec::watch`.
#[napi]
pub struct RustFsWatch;

#[napi]
impl RustFsWatch {
    /// `watch(path, options?)` — returns a handle; events stream through
    /// `setOnChange` until `dispose`.
    #[napi]
    pub fn watch(
        path: String,
        options: Option<RustFsWatchOptions>,
    ) -> napi::Result<RustFsWatchHandle> {
        let recursive = options.and_then(|o| o.recursive).unwrap_or(true);
        let inner = watch_core::watch(&path, recursive).map_err(napi_error)?;
        Ok(RustFsWatchHandle {
            inner: Arc::new(inner),
            disposed: AtomicBool::new(false),
            pump_started: AtomicBool::new(false),
        })
    }
}

/// `RustFsWatchHandle` — one watch session.
#[napi]
pub struct RustFsWatchHandle {
    inner: Arc<watch_core::FsWatchHandle>,
    disposed: AtomicBool,
    pump_started: AtomicBool,
}

/// `CalleeHandled = false` — the callback receives the change directly.
type ChangeCallback =
    ThreadsafeFunction<RustFsChange, Unknown<'static>, RustFsChange, Status, false>;

#[napi]
impl RustFsWatchHandle {
    /// Wire the change pump. Call once: `onChange(change)` fires per
    /// normalized event until `dispose()`.
    #[napi]
    pub fn set_on_change(&self, on_change: ChangeCallback) -> napi::Result<()> {
        if self.pump_started.swap(true, Ordering::AcqRel) {
            return Err(napi::Error::from_reason(
                "setOnChange may only be called once per handle",
            ));
        }
        let inner = Arc::clone(&self.inner);
        std::thread::spawn(move || {
            while let Some(change) = inner.recv() {
                let _ = on_change.call(change.into(), ThreadsafeFunctionCallMode::NonBlocking);
            }
        });
        Ok(())
    }

    /// Stop the watcher and the event pump.
    #[napi]
    pub fn dispose(&self) {
        if self.disposed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.inner.stop();
    }
}

fn napi_error(error: String) -> napi::Error {
    napi::Error::from_reason(format!("fs.watch failed: {error}"))
}
