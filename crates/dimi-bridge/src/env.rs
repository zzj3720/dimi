//! M2 env bridge — `RustHostEnvironment` napi surface over
//! `dimi-exec::env`, the swap-in socket for the App-scope
//! `IHostEnvironment` (`hostEnvironmentService.ts`).

use dimi_exec::HostEnvironmentInfo;
use napi_derive::napi;

/// `HostEnvironmentInfo` mirror — napi object with optional fields where the
/// Windows pass may not have values yet.
#[napi(object)]
pub struct RustHostEnvironmentInfo {
    pub os_kind: String,
    pub os_arch: String,
    pub os_version: String,
    pub shell_name: String,
    pub shell_path: String,
    pub path_class: String,
    pub home_dir: String,
}

impl From<HostEnvironmentInfo> for RustHostEnvironmentInfo {
    fn from(info: HostEnvironmentInfo) -> Self {
        Self {
            os_kind: info.os_kind,
            os_arch: info.os_arch,
            os_version: info.os_version,
            shell_name: info.shell_name,
            shell_path: info.shell_path,
            path_class: info.path_class,
            home_dir: info.home_dir,
        }
    }
}

/// `RustHostEnvironment` — stateless probe facade.
#[napi]
pub struct RustHostEnvironment;

#[napi]
impl RustHostEnvironment {
    /// `probeHostEnvironmentFromNode` — the immutable host snapshot.
    #[napi]
    pub fn probe() -> RustHostEnvironmentInfo {
        dimi_exec::env::probe().into()
    }
}
