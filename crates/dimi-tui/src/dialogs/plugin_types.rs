//! Plugin types + pure helpers used by the plugins selector — a Rust-side
//! mirror of `@dimi-agent/dimi-sdk` plugin shapes plus ports of
//! `apps/dimi/src/utils/plugin-marketplace.ts` (`computeUpdateStatus`) and
//! `apps/dimi/src/tui/utils/plugin-source-label.ts` (`formatPluginSourceLabel`,
//! `pluginTrustLabel`).

/// `PluginMarketplaceTier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginMarketplaceTier {
    Official,
    Curated,
}

impl PluginMarketplaceTier {
    /// `marketplaceTierLabel`.
    pub fn label(&self) -> &'static str {
        match self {
            PluginMarketplaceTier::Official => "Official plugin",
            PluginMarketplaceTier::Curated => "Curated plugin",
        }
    }
}

/// `PluginMarketplaceEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMarketplaceEntry {
    pub id: String,
    pub display_name: String,
    pub source: String,
    pub tier: Option<PluginMarketplaceTier>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub keywords: Vec<String>,
}

/// `PluginUpdateStatus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginUpdateStatus {
    NotInstalled,
    UpToDate { version: Option<String> },
    Update { local: String, latest: String },
}

// ── minimal semver (ported from the `semver` package usage) ────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<PrereleaseId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrereleaseId {
    Numeric(u64),
    Alpha(String),
}

/// `valid` — returns the canonical version when `s` parses as semver.
fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (core, prerelease) = match s.split_once('-') {
        Some((core, pre)) => {
            let pre = pre.split('+').next().unwrap_or(pre);
            (core, Some(pre))
        }
        None => {
            let core = s.split('+').next().unwrap_or(s);
            (core, None)
        }
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let prerelease = match prerelease {
        None => Vec::new(),
        Some(pre) => {
            let ids: Vec<&str> = pre.split('.').collect();
            if ids.is_empty() || ids.iter().any(|id| id.is_empty()) {
                return None;
            }
            let mut out = Vec::new();
            for id in ids {
                if id.chars().all(|c| c.is_ascii_digit()) {
                    // Numeric identifiers must not have leading zeros.
                    if id.len() > 1 && id.starts_with('0') {
                        return None;
                    }
                    out.push(PrereleaseId::Numeric(id.parse().ok()?));
                } else {
                    out.push(PrereleaseId::Alpha(id.to_owned()));
                }
            }
            out
        }
    };
    Some(Version {
        major,
        minor,
        patch,
        prerelease,
    })
}

fn compare_prerelease(a: &[PrereleaseId], b: &[PrereleaseId]) -> std::cmp::Ordering {
    // A version with a prerelease has lower precedence than one without.
    if a.is_empty() && !b.is_empty() {
        return std::cmp::Ordering::Greater;
    }
    if !a.is_empty() && b.is_empty() {
        return std::cmp::Ordering::Less;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        let ord = match (x, y) {
            (PrereleaseId::Numeric(xn), PrereleaseId::Numeric(yn)) => xn.cmp(yn),
            (PrereleaseId::Numeric(_), PrereleaseId::Alpha(_)) => std::cmp::Ordering::Less,
            (PrereleaseId::Alpha(_), PrereleaseId::Numeric(_)) => std::cmp::Ordering::Greater,
            (PrereleaseId::Alpha(xa), PrereleaseId::Alpha(ya)) => xa.cmp(ya),
        };
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

fn semver_gt(a: &Version, b: &Version) -> bool {
    let ord = (a.major, a.minor, a.patch)
        .cmp(&(b.major, b.minor, b.patch))
        .then_with(|| compare_prerelease(&a.prerelease, &b.prerelease));
    ord == std::cmp::Ordering::Greater
}

/// `computeUpdateStatus` — only reports `update` when both versions are valid
/// semver and latest > local.
pub fn compute_update_status(
    latest: Option<&str>,
    local: Option<&str>,
    installed: bool,
) -> PluginUpdateStatus {
    if !installed {
        return PluginUpdateStatus::NotInstalled;
    }
    if let (Some(latest), Some(local)) = (latest, local) {
        if let (Some(lv), Some(lv2)) = (parse_version(latest), parse_version(local)) {
            if semver_gt(&lv, &lv2) {
                return PluginUpdateStatus::Update {
                    local: local.to_owned(),
                    latest: latest.to_owned(),
                };
            }
        }
    }
    PluginUpdateStatus::UpToDate {
        version: local.map(str::to_owned),
    }
}

// ── plugin-source-label helpers ────────────────────────────────────────────

/// `DIMI_RELEASE_PREFIX` — official release asset path prefix.
const DIMI_RELEASE_PREFIX: &str = "/zzj3720/dimi/releases/latest/download/";

/// `releaseAssetName` — asset filename for an official Dimi release URL.
fn release_asset_name(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "https" {
        return None;
    }
    let (host, path) = rest.split_once('/')?;
    if host != "github.com" {
        return None;
    }
    let path = format!("/{path}");
    if !path.starts_with(DIMI_RELEASE_PREFIX) {
        return None;
    }
    let asset = &path[DIMI_RELEASE_PREFIX.len()..];
    if asset.is_empty() {
        return None;
    }
    Some(asset.to_owned())
}

/// `pluginTrustLabel` — official / curated / third-party.
pub fn plugin_trust_label(source: &str, original_source: Option<&str>) -> &'static str {
    if source != "zip-url" {
        return "third-party";
    }
    let Some(original) = original_source else {
        return "third-party";
    };
    let Some(asset) = release_asset_name(original) else {
        return "third-party";
    };
    if asset.starts_with("dimi-") && asset.ends_with(".zip") {
        return "official";
    }
    if asset.starts_with("curated-") && asset.ends_with(".zip") {
        return "curated";
    }
    "third-party"
}

/// `hostFromUrl` — host[:port] from a URL.
fn host_from_url(raw: &str) -> Option<String> {
    let (_, rest) = raw.split_once("://")?;
    let host_port = rest.split('/').next().unwrap_or(rest);
    Some(host_port.to_owned())
}

/// `formatPluginSourceLabel`.
pub fn format_plugin_source_label(
    source: &str,
    github: Option<(&str, &str, &str)>, // (owner, repo, ref)
    original_source: Option<&str>,
) -> String {
    if source == "github" {
        if let Some((owner, repo, reference)) = github {
            return format!("github {owner}/{repo}@{reference}");
        }
    }
    if source == "zip-url" {
        if let Some(original) = original_source {
            if let Some(host) = host_from_url(original) {
                return format!("via {host}");
            }
        }
    }
    source.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_update_status_cases() {
        assert_eq!(
            compute_update_status(Some("1.2.0"), Some("1.1.0"), true),
            PluginUpdateStatus::Update {
                local: "1.1.0".to_owned(),
                latest: "1.2.0".to_owned()
            }
        );
        assert_eq!(
            compute_update_status(Some("1.1.0"), Some("1.2.0"), true),
            PluginUpdateStatus::UpToDate {
                version: Some("1.2.0".to_owned())
            }
        );
        // Non-semver never reports an update.
        assert_eq!(
            compute_update_status(Some("latest"), Some("1.0.0"), true),
            PluginUpdateStatus::UpToDate {
                version: Some("1.0.0".to_owned())
            }
        );
        assert_eq!(
            compute_update_status(Some("1.0.0"), Some("1.0.0"), false),
            PluginUpdateStatus::NotInstalled
        );
    }

    #[test]
    fn prerelease_ordering() {
        assert_eq!(
            compute_update_status(Some("1.0.1"), Some("1.0.0-rc.1"), true),
            PluginUpdateStatus::Update {
                local: "1.0.0-rc.1".to_owned(),
                latest: "1.0.1".to_owned()
            }
        );
        // Pre-release is older than the release.
        assert_eq!(
            compute_update_status(Some("1.0.0"), Some("1.0.0-rc.1"), true),
            PluginUpdateStatus::Update {
                local: "1.0.0-rc.1".to_owned(),
                latest: "1.0.0".to_owned()
            }
        );
    }

    #[test]
    fn trust_label() {
        assert_eq!(
            plugin_trust_label(
                "zip-url",
                Some("https://github.com/zzj3720/dimi/releases/latest/download/dimi-web.zip")
            ),
            "official"
        );
        assert_eq!(
            plugin_trust_label(
                "zip-url",
                Some("https://github.com/zzj3720/dimi/releases/latest/download/curated-tools.zip")
            ),
            "curated"
        );
        assert_eq!(
            plugin_trust_label("github", Some("https://github.com/foo/bar")),
            "third-party"
        );
    }

    #[test]
    fn source_label() {
        assert_eq!(
            format_plugin_source_label("github", Some(("zzj3720", "dimi", "v1.0.0")), None),
            "github zzj3720/dimi@v1.0.0"
        );
        assert_eq!(
            format_plugin_source_label("zip-url", None, Some("https://example.com/plugin.zip")),
            "via example.com"
        );
        assert_eq!(
            format_plugin_source_label("local-path", None, None),
            "local-path"
        );
    }
}
