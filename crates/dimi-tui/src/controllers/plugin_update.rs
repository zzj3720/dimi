//! Plugin update notifier — port of `apps/dimi/src/tui/controllers/
//! plugin-update-notifier.ts`.
//!
//! The pure parts: the `mcp__plugin-` tool-name prefix check, the plugin-id
//! resolution from plugin MCP server names (longest-prefix match with the
//! boundary check), the semver update decision (`computeUpdateStatus`), the
//! already-notified latch, and the in-flight guard. The marketplace fetch,
//! `session.listMcpServers` / `listPlugins`, and the notice-state
//! read/write are `// TODO(legacy)`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

/// `MCP_TOOL_NAME_PREFIX`.
pub const MCP_TOOL_NAME_PREFIX: &str = "mcp__";
/// `PLUGIN_MCP_TOOL_NAME_PREFIX`.
pub const PLUGIN_MCP_TOOL_NAME_PREFIX: &str = "mcp__plugin-";

/// `DIMI_CODE_PLUGIN_MARKETPLACE_URL` — the official marketplace source that
/// can back an "Official Marketplace" notice.
pub const DIMI_CODE_PLUGIN_MARKETPLACE_URL: &str = "https://plugins.dimi.sh/marketplace.json";

/// `PLUGIN_MCP_RUNTIME_NAME` — plugin MCP servers run under the runtime name
/// `plugin-<id>:<server>`.
fn plugin_mcp_runtime_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^plugin-([a-z0-9][a-z0-9_-]{0,63}):").expect("valid plugin runtime regex")
    })
}

/// Cheap name check for plugin-provided MCP tools (`mcp__plugin-…`).
pub fn is_plugin_mcp_tool_name(tool_name: &str) -> bool {
    tool_name.starts_with(PLUGIN_MCP_TOOL_NAME_PREFIX)
}

/// Mirror of `sanitizeMcpNamePart` — MCP tool names on the wire carry the
/// sanitized server name; the collapse step guarantees the `__` separator
/// never appears inside a name part.
pub fn sanitize_mcp_server_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut out = String::with_capacity(sanitized.len());
    let mut prev_underscore = false;
    for c in sanitized.chars() {
        if c == '_' {
            if prev_underscore {
                continue;
            }
            prev_underscore = true;
        } else {
            prev_underscore = false;
        }
        out.push(c);
    }
    out
}

/// Extract the plugin id from a plugin MCP server runtime name
/// (`plugin-<id>:<server>`).
pub fn extract_plugin_id_from_mcp_server_name(server_name: &str) -> Option<String> {
    plugin_mcp_runtime_re()
        .captures(server_name)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

/// Find the plugin behind a qualified MCP tool name by longest-prefix match
/// against known server names.
pub fn match_plugin_by_tool_name(
    tool_name: &str,
    server_plugin_ids: &BTreeMap<String, String>,
) -> Option<String> {
    let mut best: Option<String> = None;
    let mut best_length = 0usize;
    for (server_name, plugin_id) in server_plugin_ids {
        let prefix = format!("{MCP_TOOL_NAME_PREFIX}{server_name}");
        if !tool_name.starts_with(&prefix) {
            continue;
        }
        let boundary = tool_name[prefix.len()..].chars().next();
        if boundary.is_some() && boundary != Some('_') {
            continue;
        }
        if prefix.len() > best_length {
            best = Some(plugin_id.clone());
            best_length = prefix.len();
        }
    }
    best
}

/// `PluginUpdateStatus` — port of the SDK/marketplace helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    NotInstalled,
    UpToDate { version: Option<String> },
    Update { local: String, latest: String },
}

/// Parse a loose semver `major.minor.patch` (with optional `v` prefix,
/// pre-release, build). Returns `None` for anything `semver.valid` would
/// reject (so a stale/non-semver version never yields an update).
fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn semver_cmp(a: &str, b: &str) -> Option<Ordering> {
    match (parse_semver(a), parse_semver(b)) {
        (Some(a), Some(b)) => Some(a.cmp(&b)),
        _ => None,
    }
}

/// `computeUpdateStatus(latest, local, installed)` — only reports `update`
/// when both are valid semver and latest > local.
pub fn compute_update_status(
    latest: Option<&str>,
    local: Option<&str>,
    installed: bool,
) -> UpdateStatus {
    if !installed {
        return UpdateStatus::NotInstalled;
    }
    if let (Some(latest), Some(local)) = (latest, local) {
        if semver_cmp(latest, local) == Some(Ordering::Greater) {
            return UpdateStatus::Update {
                local: local.to_owned(),
                latest: latest.to_owned(),
            };
        }
    }
    // Report only the actual installed version; never borrow the marketplace
    // version (that would falsely claim "up to date" and hide future updates).
    UpdateStatus::UpToDate {
        version: local.map(str::to_owned),
    }
}

/// A marketplace plugin entry (the fields the notifier reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketplacePluginEntry {
    pub id: String,
    pub version: Option<String>,
}

/// An installed plugin (the fields the notifier reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub id: String,
    pub version: Option<String>,
    pub display_name: String,
    /// `isOfficialPluginInstall` — whether this install is tracked against
    /// the Official Marketplace.
    pub official: bool,
}

/// Everything the pure `check_and_notify` decision reads.
#[derive(Debug, Clone)]
pub struct PluginCheckContext<'a> {
    pub marketplace_source: &'a str,
    pub marketplace_plugins: &'a [MarketplacePluginEntry],
    pub installed: Option<&'a InstalledPlugin>,
    /// `state.notified[pluginId]` — the last notified version.
    pub already_notified_latest: Option<&'a str>,
}

/// The outcome of a version check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyDecision {
    Skip,
    Notify(String),
}

/// The plugin update notifier state machine (port of `PluginUpdateNotifier`).
#[derive(Debug, Default)]
pub struct PluginUpdateNotifier {
    in_flight: BTreeSet<String>,
    mcp_server_plugin_ids: Option<BTreeMap<String, String>>,
}

impl PluginUpdateNotifier {
    pub fn new() -> Self {
        PluginUpdateNotifier::default()
    }

    /// `handleMcpToolCompleted` — cheap bail before the RPC layer. Returns
    /// whether the tool name could belong to a plugin MCP server (and thus
    /// warrants a lookup).
    pub fn handle_mcp_tool_completed(&mut self, tool_name: &str) -> bool {
        if !is_plugin_mcp_tool_name(tool_name) {
            return false;
        }
        true
    }

    /// `resolvePluginId` — memoized map first, then a one-time refresh on a
    /// miss.
    pub fn resolve_plugin_id(
        &mut self,
        tool_name: &str,
        server_names: &[String],
    ) -> Option<String> {
        if let Some(cached) = &self.mcp_server_plugin_ids {
            if let Some(hit) = match_plugin_by_tool_name(tool_name, cached) {
                return Some(hit);
            }
        }
        let refreshed = load_mcp_server_plugin_ids(server_names);
        self.mcp_server_plugin_ids = Some(refreshed.clone());
        match_plugin_by_tool_name(tool_name, &refreshed)
    }

    /// `checkAndNotify` — the pure decision. `in_flight` guards concurrent
    /// checks; the marketplace/notice-state I/O is legacy.
    pub fn check_and_notify(
        &mut self,
        plugin_id: &str,
        ctx: &PluginCheckContext<'_>,
    ) -> NotifyDecision {
        if self.in_flight.contains(plugin_id) {
            return NotifyDecision::Skip;
        }
        self.in_flight.insert(plugin_id.to_owned());
        let decision = check_and_notify_decision(plugin_id, ctx);
        self.in_flight.remove(plugin_id);
        decision
    }

    pub fn is_in_flight(&self, plugin_id: &str) -> bool {
        self.in_flight.contains(plugin_id)
    }
}

/// The pure `checkAndNotify` body (no in-flight guard — see the wrapper).
fn check_and_notify_decision(plugin_id: &str, ctx: &PluginCheckContext<'_>) -> NotifyDecision {
    // Only the default official catalog can back an "Official Marketplace"
    // notice — a custom catalog may advertise anything under any id.
    if ctx.marketplace_source != DIMI_CODE_PLUGIN_MARKETPLACE_URL {
        return NotifyDecision::Skip;
    }
    let Some(entry) = ctx.marketplace_plugins.iter().find(|p| p.id == plugin_id) else {
        return NotifyDecision::Skip;
    };
    let Some(installed) = ctx.installed else {
        return NotifyDecision::Skip;
    };
    if installed.id != plugin_id {
        return NotifyDecision::Skip;
    }
    // Only official installs are tracked against the Official Marketplace.
    if !installed.official {
        return NotifyDecision::Skip;
    }
    let status =
        compute_update_status(entry.version.as_deref(), installed.version.as_deref(), true);
    let UpdateStatus::Update { latest, .. } = status else {
        return NotifyDecision::Skip;
    };
    if ctx.already_notified_latest == Some(latest.as_str()) {
        return NotifyDecision::Skip;
    }
    NotifyDecision::Notify(format!(
        "Update detected: {} {latest} is available. Run /plugins to install the latest version from the Official Marketplace.",
        installed.display_name
    ))
}

/// `loadMcpServerPluginIds` — build the sanitized-server-name → plugin-id map
/// from the plugin MCP runtime names.
pub fn load_mcp_server_plugin_ids(server_names: &[String]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for name in server_names {
        if let Some(plugin_id) = extract_plugin_id_from_mcp_server_name(name) {
            map.insert(sanitize_mcp_server_name(name), plugin_id);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_mcp_tool_name_prefix() {
        assert!(is_plugin_mcp_tool_name("mcp__plugin-foo__read"));
        assert!(is_plugin_mcp_tool_name("mcp__plugin-123__x"));
        assert!(!is_plugin_mcp_tool_name("mcp__read"));
        assert!(!is_plugin_mcp_tool_name("Bash"));
        // `"mcp__plugin-"` itself starts with the prefix (TS startsWith).
        assert!(is_plugin_mcp_tool_name("mcp__plugin-"));
    }

    #[test]
    fn sanitize_mcp_server_name_collapses() {
        assert_eq!(sanitize_mcp_server_name("my server"), "my_server");
        assert_eq!(sanitize_mcp_server_name("a__b"), "a_b");
        assert_eq!(sanitize_mcp_server_name("ok-name_1"), "ok-name_1");
    }

    #[test]
    fn extract_plugin_id_from_runtime_name() {
        assert_eq!(
            extract_plugin_id_from_mcp_server_name("plugin-myplugin:server"),
            Some("myplugin".to_owned())
        );
        assert_eq!(
            extract_plugin_id_from_mcp_server_name("plugin-my_plugin:x"),
            Some("my_plugin".to_owned())
        );
        assert_eq!(extract_plugin_id_from_mcp_server_name("plain-server"), None);
    }

    #[test]
    fn match_plugin_by_tool_name_longest_prefix() {
        let map: BTreeMap<String, String> = BTreeMap::from([
            ("myplugin".to_owned(), "p1".to_owned()),
            ("myplugin_ext".to_owned(), "p2".to_owned()),
        ]);
        // Longest prefix wins.
        assert_eq!(
            match_plugin_by_tool_name("mcp__myplugin_ext__read", &map).as_deref(),
            Some("p2")
        );
        assert_eq!(
            match_plugin_by_tool_name("mcp__myplugin__read", &map).as_deref(),
            Some("p1")
        );
        // Boundary check: a truncated server part must not match.
        assert_eq!(
            match_plugin_by_tool_name("mcp__mypluginX__read", &map),
            None
        );
        assert_eq!(match_plugin_by_tool_name("mcp__other__read", &map), None);
    }

    #[test]
    fn compute_update_status_requires_valid_semver_gt() {
        assert_eq!(
            compute_update_status(Some("1.2.0"), Some("1.1.9"), true),
            UpdateStatus::Update {
                local: "1.1.9".to_owned(),
                latest: "1.2.0".to_owned()
            }
        );
        // Downgrade → up-to-date.
        assert_eq!(
            compute_update_status(Some("1.0.0"), Some("1.2.0"), true),
            UpdateStatus::UpToDate {
                version: Some("1.2.0".to_owned())
            }
        );
        // Non-semver never yields an update.
        assert_eq!(
            compute_update_status(Some("latest"), Some("1.2.0"), true),
            UpdateStatus::UpToDate {
                version: Some("1.2.0".to_owned())
            }
        );
        assert_eq!(
            compute_update_status(Some("1.2.0"), Some("not-a-version"), true),
            UpdateStatus::UpToDate {
                version: Some("not-a-version".to_owned())
            }
        );
        // Not installed.
        assert_eq!(
            compute_update_status(Some("1.2.0"), Some("1.0.0"), false),
            UpdateStatus::NotInstalled
        );
        // Pre-release versions are not newer than a release.
        assert_eq!(
            compute_update_status(Some("1.1.0-rc.1"), Some("1.1.0"), true),
            UpdateStatus::UpToDate {
                version: Some("1.1.0".to_owned())
            }
        );
    }

    fn ctx<'a>(
        source: &'a str,
        plugins: &'a [MarketplacePluginEntry],
        installed: Option<&'a InstalledPlugin>,
        notified: Option<&'a str>,
    ) -> PluginCheckContext<'a> {
        PluginCheckContext {
            marketplace_source: source,
            marketplace_plugins: plugins,
            installed,
            already_notified_latest: notified,
        }
    }

    #[test]
    fn check_and_notify_decisions() {
        let plugins = vec![MarketplacePluginEntry {
            id: "p1".to_owned(),
            version: Some("2.0.0".to_owned()),
        }];
        let installed = InstalledPlugin {
            id: "p1".to_owned(),
            version: Some("1.0.0".to_owned()),
            display_name: "My Plugin".to_owned(),
            official: true,
        };
        // Official marketplace + update → notify.
        let decision = check_and_notify_decision(
            "p1",
            &ctx(
                DIMI_CODE_PLUGIN_MARKETPLACE_URL,
                &plugins,
                Some(&installed),
                None,
            ),
        );
        assert!(
            matches!(decision, NotifyDecision::Notify(msg) if msg.starts_with("Update detected: My Plugin 2.0.0"))
        );

        // Custom marketplace → skip.
        assert_eq!(
            check_and_notify_decision(
                "p1",
                &ctx("https://custom.example", &plugins, Some(&installed), None)
            ),
            NotifyDecision::Skip
        );
        // Already notified at the latest → skip.
        assert_eq!(
            check_and_notify_decision(
                "p1",
                &ctx(
                    DIMI_CODE_PLUGIN_MARKETPLACE_URL,
                    &plugins,
                    Some(&installed),
                    Some("2.0.0")
                )
            ),
            NotifyDecision::Skip
        );
        // Not official → skip.
        let unofficial = InstalledPlugin {
            official: false,
            ..installed.clone()
        };
        assert_eq!(
            check_and_notify_decision(
                "p1",
                &ctx(
                    DIMI_CODE_PLUGIN_MARKETPLACE_URL,
                    &plugins,
                    Some(&unofficial),
                    None
                )
            ),
            NotifyDecision::Skip
        );
        // Not installed → skip.
        assert_eq!(
            check_and_notify_decision(
                "p1",
                &ctx(DIMI_CODE_PLUGIN_MARKETPLACE_URL, &plugins, None, None)
            ),
            NotifyDecision::Skip
        );
    }

    #[test]
    fn in_flight_guard_prevents_reentry() {
        let mut notifier = PluginUpdateNotifier::new();
        let plugins = vec![MarketplacePluginEntry {
            id: "p1".to_owned(),
            version: Some("2.0.0".to_owned()),
        }];
        let installed = InstalledPlugin {
            id: "p1".to_owned(),
            version: Some("1.0.0".to_owned()),
            display_name: "P".to_owned(),
            official: true,
        };
        // Simulate a re-entrant call while p1 is in flight.
        notifier.in_flight.insert("p1".to_owned());
        let decision = notifier.check_and_notify(
            "p1",
            &ctx(
                DIMI_CODE_PLUGIN_MARKETPLACE_URL,
                &plugins,
                Some(&installed),
                None,
            ),
        );
        assert_eq!(decision, NotifyDecision::Skip);
        // TS `checkAndNotify` returns early when already in flight without
        // removing the guard — the outer (first) call owns the removal.
        assert!(notifier.is_in_flight("p1"));
    }

    #[test]
    fn resolve_plugin_id_refreshes_on_miss() {
        let mut notifier = PluginUpdateNotifier::new();
        let servers = vec!["plugin-p1:main".to_owned()];
        // MCP tool names carry the sanitized server name (`:` → `_`), so the
        // tool for server `plugin-p1:main` is `mcp__plugin-p1_main__read`.
        assert_eq!(
            notifier
                .resolve_plugin_id("mcp__plugin-p1_main__read", &servers)
                .as_deref(),
            Some("p1")
        );
        // Second call hits the cache.
        assert_eq!(
            notifier
                .resolve_plugin_id("mcp__plugin-p1_main__read", &servers)
                .as_deref(),
            Some("p1")
        );
        // Unknown tool → None.
        assert_eq!(
            notifier.resolve_plugin_id("mcp__other__read", &servers),
            None
        );
    }
}
