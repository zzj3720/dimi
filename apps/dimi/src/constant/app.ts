import { ErrorCodes } from "@dimi-agent/dimi-sdk";

export const PRODUCT_NAME = "Dimi";
export const CLI_COMMAND_NAME = "dimi";
export const PROCESS_NAME = "dimi";

// Used in HTTP User-Agent headers and (historically) telemetry app names.
export const CLI_USER_AGENT_PRODUCT = "dimi-cli";
export const CLI_UI_MODE = "shell";
// UI mode for the `dimi web` host. Same product
// as the CLI (CLI_USER_AGENT_PRODUCT); the surface is distinguished by ui_mode.
export const WEB_UI_MODE = "web";

// Upper bound for CLI shutdown: a wedged cleanup step must not hold the process
// hostage. Telemetry no longer flushes anywhere, but other best-effort cleanup
// still respects this deadline.
export const CLI_SHUTDOWN_TIMEOUT_MS = 3000;

// Upper bound on headless (`dimi -p`) shutdown. A wedged cleanup step (e.g. a
// SessionEnd hook, an MCP shutdown, or a connection blackholed by a restrictive
// firewall) must not keep a completed run alive indefinitely — once this elapses
// we stop waiting on cleanup and let the run return.
export const PROMPT_CLEANUP_TIMEOUT_MS = 8000;

// Grace after a headless run has fully completed (turn done, cleanup attempted)
// before force-exiting. `dimi -p` otherwise relies on the event loop draining to
// exit; a stray ref'd handle (socket/timer/child) left over from the run would
// wedge it. The guard timer is unref'd, so a healthy run still exits naturally
// well before this fires.
export const HEADLESS_FORCE_EXIT_GRACE_MS = 2000;

// Max time to wait for buffered stdout/stderr to flush before arming the
// force-exit fallback. A slow/piped consumer's still-draining stdio is a
// legitimate ref'd handle — flushing first prevents the fallback from
// truncating completed output. Bounded so a permanently-stuck consumer can't
// re-introduce the hang.
export const HEADLESS_STDIO_DRAIN_TIMEOUT_MS = 10000;

// Published npm package name; this can differ from the executable command.
export const NPM_PACKAGE_NAME = "@dimi-agent/cli";

// App-owned data paths. SDK/core runtime config is intentionally not routed here.
export const DIMI_CODE_HOME_ENV = "DIMI_CODE_HOME";
export const DIMI_CODE_DATA_DIR_NAME = ".dimi";
export const DIMI_CODE_LOG_DIR_NAME = "logs";
export const DIMI_CODE_CACHE_DIR_NAME = "cache";
export const DIMI_CODE_UPDATE_DIR_NAME = "updates";
export const DIMI_CODE_BIN_DIR_NAME = "bin";
export const DIMI_CODE_UPDATE_STATE_FILE_NAME = "latest.json";
export const DIMI_CODE_UPDATE_INSTALL_STATE_FILE_NAME = "install.json";
export const DIMI_CODE_UPDATE_INSTALL_LOCK_FILE_NAME = "install.lock";
export const DIMI_CODE_UPDATE_ROLLOUT_LOG_FILE_NAME = "rollout.log";
export const DIMI_CODE_PLUGIN_UPDATE_NOTICE_STATE_FILE_NAME = "plugin-notices.json";
export const DIMI_CODE_INPUT_HISTORY_DIR_NAME = "user-history";
export const DIMI_CODE_BANNER_DIR_NAME = "banner";
export const DIMI_CODE_BANNER_STATE_FILE_NAME = "state.json";

// Managed Dimi auth provider key shared with OAuth/SDK config.
export const DEFAULT_OAUTH_PROVIDER_NAME = "kimi-coding";

// SDK/core error code that tells the TUI to show a login-required startup
// notice. Derived from sdk's ErrorCodes so a future rename in core
// auto-propagates instead of silently breaking the startup recovery path.
export const AUTH_LOGIN_REQUIRED_CODE = ErrorCodes.AUTH_LOGIN_REQUIRED;

export const FEEDBACK_ISSUE_URL = "https://github.com/zzj3720/dimi/issues";

// Sent in the feedback `version` field so the backend can distinguish this
// TypeScript client from clients that send a bare version.
export const FEEDBACK_VERSION_PREFIX = "dimi-";

// Telemetry event name; keep stable for dashboard queries.
export const FEEDBACK_TELEMETRY_EVENT = "feedback_submitted";

// Legacy CDN used by the pre-fork installer and plugin marketplace.
export const DIMI_CODE_CDN_BASE = "https://github.com/zzj3720/dimi/releases/latest/download";
/**
 * Dimi's own update authority: the `latest.json` manifest attached to the
 * newest GitHub Release. GitHub's `releases/latest/download/<asset>` endpoint
 * redirects to the latest release's asset, so publishing a tag and attaching
 * `latest.json` (see `.github/workflows/release.yml`) is the entire release
 * process. The URL 404s until the first release exists, which degrades
 * cleanly to "no update available".
 */
export const DIMI_CODE_UPDATE_CHANNEL_URL =
  "https://github.com/zzj3720/dimi/releases/latest/download/latest.json";
/** The old product's remotely controlled banner must not be shown by this build. */
export const DIMI_CODE_TIPS_BANNER_URL: string | undefined = undefined;
// GitHub Release assets are flat (no subdirectories), so the marketplace
// catalog lives at the release root next to the native bundles.
export const DIMI_CODE_PLUGIN_MARKETPLACE_URL = `${DIMI_CODE_CDN_BASE}/marketplace.json`;
export const DIMI_CODE_PLUGIN_MARKETPLACE_URL_ENV = "DIMI_CODE_PLUGIN_MARKETPLACE_URL";
// Official plugins whose usage bills against the user's plan quota. Installing
// one of these shows a quota note after the install result.
export const QUOTA_CONSUMING_PLUGIN_IDS: readonly string[] = ["dimi-datasource"];
export const DIMI_CODE_INSTALL_SH_URL = `${DIMI_CODE_CDN_BASE}/install.sh`;
export const DIMI_CODE_INSTALL_PS1_URL = `${DIMI_CODE_CDN_BASE}/install.ps1`;
// Official download page, referenced by prompt copy that steers users away
// from third-party install sources.
export const DIMI_CODE_OFFICIAL_INSTALL_URL = "https://github.com/zzj3720/dimi";

// Native install commands, split by platform. Use these for prompt copy and spawn calls only; do not assemble the strings elsewhere.
export const NATIVE_INSTALL_COMMAND_UNIX = `curl -fsSL ${DIMI_CODE_INSTALL_SH_URL} | bash`;
export const NATIVE_INSTALL_COMMAND_WIN = `irm ${DIMI_CODE_INSTALL_PS1_URL} | iex`;
