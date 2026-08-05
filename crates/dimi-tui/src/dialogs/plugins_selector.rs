//! Plugins selectors — MCP server picker, remove/install-trust confirmations,
//! and the unified 4-tab plugins panel. Port of
//! `apps/dimi/src/tui/components/dialogs/plugins-selector.ts`.

use crate::component::{Component, Focusable};
use crate::dialogs::SELECT_POINTER;
use crate::dialogs::input_line::{InputEvent, InputLine};
use crate::dialogs::plugin_types::{
    PluginMarketplaceEntry, PluginMarketplaceTier, compute_update_status,
    format_plugin_source_label, plugin_trust_label,
};
use crate::keys::{matches_key, printable_char};
use crate::tab_strip::render_tab_strip;
use crate::theme::{ColorToken, current_theme};
use crate::width::visible_width;
use crate::wrap::truncate_to_width;

const MCP_SERVER_PREFIX: &str = "mcp:";
const ELLIPSIS: &str = "…";

/// Web Bridge pinned entry (hardcoded promotion on the Official tab).
pub const WEB_BRIDGE_URL: &str = "https://github.com/zzj3720/dimi";
pub fn web_bridge_entry() -> PluginMarketplaceEntry {
    PluginMarketplaceEntry {
        id: "dimi-webbridge".to_owned(),
        display_name: "Dimi WebBridge".to_owned(),
        source: WEB_BRIDGE_URL.to_owned(),
        tier: Some(PluginMarketplaceTier::Official),
        homepage: Some(WEB_BRIDGE_URL.to_owned()),
        description: Some(
            "Control your real browser from Dimi — navigate, click, type, and screenshot"
                .to_owned(),
        ),
        version: None,
        keywords: Vec::new(),
    }
}

/// `PluginMcpServerInfo`.
#[derive(Debug, Clone)]
pub struct PluginMcpServerInfo {
    pub name: String,
    pub enabled: bool,
    /// `'stdio' | 'http' | 'sse'`.
    pub transport: &'static str,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub runtime_name: Option<String>,
    pub cwd: Option<String>,
}

/// `PluginInfo` (fields the MCP selector reads).
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub display_name: String,
    pub mcp_servers: Vec<PluginMcpServerInfo>,
}

impl PluginInfo {
    pub fn mcp_server_count(&self) -> usize {
        self.mcp_servers.len()
    }

    pub fn enabled_mcp_server_count(&self) -> usize {
        self.mcp_servers.iter().filter(|s| s.enabled).count()
    }
}

/// `PluginSummary` (fields the plugins panel reads).
#[derive(Debug, Clone)]
pub struct PluginSummary {
    pub id: String,
    pub display_name: String,
    /// `'ok'` or another state string.
    pub state: String,
    pub enabled: bool,
    pub skill_count: usize,
    pub mcp_server_count: usize,
    pub enabled_mcp_server_count: usize,
    pub has_errors: bool,
    pub version: Option<String>,
    /// Install source kind (`'github' | 'zip-url' | 'local-path' | …`).
    pub source: String,
    /// `(owner, repo, ref)` for github sources.
    pub github: Option<(String, String, String)>,
    pub original_source: Option<String>,
}

/// `PluginsPanelTabId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginsPanelTabId {
    Installed,
    Official,
    ThirdParty,
    Custom,
}

/// Selection the plugins panel reports to the host (`PluginsPanelSelection`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginsPanelSelection {
    Toggle { id: String, enabled: bool },
    Remove { id: String },
    Mcp { id: String },
    Details { id: String },
    Reload,
    Install { entry: PluginMarketplaceEntry },
    InstallSource { source: String },
    OpenUrl { url: String, label: String },
}

/// Action surfaced by the panel host-side poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginsPanelAction {
    Select(PluginsPanelSelection),
    Cancel,
    /// The host should fetch the marketplace catalog (`onRequestMarketplace`).
    RequestMarketplace,
}

/// `PluginsPanelOptions`.
#[derive(Debug, Clone)]
pub struct PluginsPanelOptions {
    pub installed: Vec<PluginSummary>,
    /// id → installed version (None when unknown). Used for the marketplace
    /// update status; a plain id set would compare the marketplace version
    /// against itself and never report an update.
    pub installed_versions: std::collections::HashMap<String, Option<String>>,
    pub initial_tab: Option<PluginsPanelTabId>,
    pub selected_id: Option<String>,
    pub plugin_hint: Option<(String, String)>,
}

// ── helpers ────────────────────────────────────────────────────────────────

fn muted_hint_line(text: &str) -> String {
    current_theme().fg(ColorToken::TextMuted, text)
}

fn section_label(label: &str) -> String {
    current_theme().bold_fg(ColorToken::TextDim, &format!(" {label}"))
}

fn wrap_overview_description(text: &str, width: usize) -> Vec<String> {
    let max_width = width.max(1);
    let words: Vec<&str> = text.split_whitespace().filter(|w| !w.is_empty()).collect();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in words {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if visible_width(&candidate) <= max_width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(current);
        }
        current = if visible_width(word) <= max_width {
            word.to_owned()
        } else {
            truncate_to_width(word, max_width, ELLIPSIS, false)
        };
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn status_style(status: &str, kind_is_action: bool) -> String {
    let theme = current_theme();
    if kind_is_action {
        return theme.fg(ColorToken::TextDim, status);
    }
    if status == "enabled" || status == "installed" {
        return theme.fg(ColorToken::Success, status);
    }
    if status.starts_with("install") {
        return theme.fg(ColorToken::Primary, status);
    }
    if status == "disabled" {
        return theme.fg(ColorToken::TextDim, status);
    }
    if status.starts_with(|c: char| c.is_ascii_digit()) {
        return theme.fg(ColorToken::TextDim, status);
    }
    theme.fg(ColorToken::Warning, status)
}

fn marketplace_status_style(status: &str) -> String {
    let theme = current_theme();
    if status.starts_with("update") {
        return theme.fg(ColorToken::Warning, status);
    }
    if status.starts_with("installed") {
        return theme.fg(ColorToken::Success, status);
    }
    theme.fg(ColorToken::Primary, status)
}

/// `overviewPluginDescription`.
fn overview_plugin_description(plugin: &PluginSummary) -> String {
    let state = if plugin.state == "ok" {
        ""
    } else {
        &format!(" · state {}", plugin.state)
    };
    let skills = format!(
        "{} skill{}",
        plugin.skill_count,
        if plugin.skill_count == 1 { "" } else { "s" }
    );
    let mcp = if plugin.mcp_server_count > 0 {
        format!(
            " · MCP {}/{}",
            plugin.enabled_mcp_server_count, plugin.mcp_server_count
        )
    } else {
        String::new()
    };
    let diagnostics = if plugin.has_errors {
        " · diagnostics available"
    } else {
        ""
    };
    let source = format!(
        " · {}",
        format_plugin_source_label(
            &plugin.source,
            plugin
                .github
                .as_ref()
                .map(|(o, r, rf)| (o.as_str(), r.as_str(), rf.as_str())),
            plugin.original_source.as_deref(),
        )
    );
    let trust = format!(
        " · {}",
        plugin_trust_label(&plugin.source, plugin.original_source.as_deref())
    );
    format!(
        "id {} · {skills}{mcp}{source}{trust}{state}{diagnostics}",
        plugin.id
    )
}

fn plugin_status(plugin: &PluginSummary) -> Option<&str> {
    if plugin.state != "ok" {
        return Some(&plugin.state);
    }
    if plugin.enabled {
        Some("enabled")
    } else {
        Some("disabled")
    }
}

// ===========================================================================
// PluginMcpSelectorComponent
// ===========================================================================

struct PluginsOverviewItem {
    value: String,
    kind: bool, // true = plugin, false = action
    label: String,
    status: Option<String>,
    description: String,
}

/// `PluginMcpSelection`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginMcpSelection {
    Toggle {
        plugin_id: String,
        server: String,
        enabled: bool,
    },
    Back {
        plugin_id: String,
    },
}

/// `PluginMcpSelectorOptions`.
#[derive(Debug, Clone)]
pub struct PluginMcpSelectorOptions {
    pub info: PluginInfo,
    pub selected_server: Option<String>,
    pub server_hint: Option<(String, String)>,
}

/// `PluginMcpSelectorComponent`.
pub struct PluginMcpSelectorComponent {
    opts: PluginMcpSelectorOptions,
    items: Vec<PluginsOverviewItem>,
    selected_index: usize,
    focused: bool,
    action: Option<PluginMcpSelection>,
}

impl PluginMcpSelectorComponent {
    pub fn new(opts: PluginMcpSelectorOptions) -> Self {
        let items = build_mcp_items(&opts.info);
        let selected_index = items
            .iter()
            .position(|item| {
                item.value
                    == format!(
                        "{MCP_SERVER_PREFIX}{}",
                        opts.selected_server.as_deref().unwrap_or("")
                    )
            })
            .unwrap_or(0);
        PluginMcpSelectorComponent {
            opts,
            items,
            selected_index,
            focused: false,
            action: None,
        }
    }

    pub fn take_action(&mut self) -> Option<PluginMcpSelection> {
        self.action.take()
    }

    fn render_item(&self, item: &PluginsOverviewItem, index: usize, width: usize) -> Vec<String> {
        let theme = current_theme();
        let selected = index == self.selected_index;
        let pointer = if selected { SELECT_POINTER } else { " " };
        let label = if selected {
            theme.bold_fg(ColorToken::Primary, &item.label)
        } else {
            theme.fg(ColorToken::Text, &item.label)
        };
        let prefix = theme.fg(
            if selected {
                ColorToken::Primary
            } else {
                ColorToken::TextDim
            },
            &format!("  {pointer} "),
        );
        let mut line = format!("{prefix}{label}");
        if let Some(status) = &item.status {
            line.push_str(&format!("  {}", status_style(status, !item.kind)));
        }
        if let Some((server, text)) = &self.opts.server_hint {
            if server_name_of(item).as_deref() == Some(server.as_str()) {
                line.push_str(&format!("  {}", theme.fg(ColorToken::Warning, text)));
            }
        }
        let description_width = (width.saturating_sub(4)).max(1);
        let mut lines = vec![line];
        for desc_line in wrap_overview_description(&item.description, description_width) {
            lines.push(muted_hint_line(&format!("    {desc_line}")));
        }
        lines
    }
}

fn build_mcp_items(info: &PluginInfo) -> Vec<PluginsOverviewItem> {
    let mut items: Vec<PluginsOverviewItem> = info
        .mcp_servers
        .iter()
        .map(|server| PluginsOverviewItem {
            value: format!("{MCP_SERVER_PREFIX}{}", server.name),
            kind: true,
            label: server.name.clone(),
            status: Some(
                if server.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
                .to_owned(),
            ),
            description: mcp_server_description(server),
        })
        .collect();
    items.push(PluginsOverviewItem {
        value: "back".to_owned(),
        kind: false,
        label: "Back to installed plugins".to_owned(),
        status: None,
        description: "Return to the local plugin manager.".to_owned(),
    });
    items
}

fn mcp_server_description(server: &PluginMcpServerInfo) -> String {
    let action = if server.enabled {
        "Enter/Space disable"
    } else {
        "Enter/Space enable"
    };
    if server.transport == "http" || server.transport == "sse" {
        let url = server
            .url
            .clone()
            .or_else(|| server.runtime_name.clone())
            .unwrap_or_default();
        return format!("{action} · {} · {url}", server.transport.to_uppercase());
    }
    let args = if !server.args.is_empty() {
        format!(" {}", server.args.join(" "))
    } else {
        String::new()
    };
    // TS: `${server.command ?? ''}${args}`.trim() — command and runtimeName
    // are alternatives (`command || server.runtimeName`), not concatenated.
    let command = format!("{}{}", server.command.clone().unwrap_or_default(), args)
        .trim()
        .to_owned();
    let cwd = server
        .cwd
        .as_deref()
        .map(|c| format!(" · cwd {c}"))
        .unwrap_or_default();
    let runtime = server.runtime_name.clone().unwrap_or_default();
    let command = if command.is_empty() { runtime } else { command };
    format!("{action} · stdio · {command}{cwd}")
}

fn server_name_of(item: &PluginsOverviewItem) -> Option<String> {
    if !item.value.starts_with(MCP_SERVER_PREFIX) {
        return None;
    }
    Some(item.value[MCP_SERVER_PREFIX.len()..].to_owned())
}

impl Component for PluginMcpSelectorComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let colors = theme.palette();
        let server_items: Vec<&PluginsOverviewItem> =
            self.items.iter().filter(|i| i.kind).collect();
        let action_items: Vec<&PluginsOverviewItem> =
            self.items.iter().filter(|i| !i.kind).collect();
        let mut lines: Vec<String> = vec![
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
            theme.bold_fg(
                ColorToken::Primary,
                &format!(" MCP servers · {}", self.opts.info.display_name),
            ),
            muted_hint_line(" ↑↓ navigate · Enter/Space enable/disable · Esc cancel"),
            String::new(),
            section_label(&format!(
                "MCP servers ({}/{} enabled)",
                self.opts.info.enabled_mcp_server_count(),
                self.opts.info.mcp_server_count()
            )),
        ];
        if server_items.is_empty() {
            lines.push(theme.fg(ColorToken::TextMuted, "  No MCP servers declared."));
        } else {
            for (i, item) in server_items.iter().enumerate() {
                lines.extend(self.render_item(item, i, width));
            }
        }
        lines.push(String::new());
        lines.push(section_label("Actions"));
        for (i, item) in action_items.iter().enumerate() {
            lines.extend(self.render_item(item, server_items.len() + i, width));
        }
        lines.push(String::new());
        lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
        let _ = colors;
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, ELLIPSIS, false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") {
            self.action = Some(PluginMcpSelection::Back {
                plugin_id: self.opts.info.id.clone(),
            });
            return;
        }
        if matches_key(data, "up") {
            self.selected_index = self.selected_index.saturating_sub(1);
            return;
        }
        if matches_key(data, "down") {
            self.selected_index = (self.selected_index + 1).min(self.items.len() - 1);
            return;
        }
        if matches_key(data, "enter") || matches_key(data, "space") || printable_char(data) == " " {
            let Some(chosen) = self.items.get(self.selected_index) else {
                return;
            };
            if chosen.value == "back" {
                self.action = Some(PluginMcpSelection::Back {
                    plugin_id: self.opts.info.id.clone(),
                });
                return;
            }
            let Some(server_name) = server_name_of(chosen) else {
                return;
            };
            let Some(server) = self
                .opts
                .info
                .mcp_servers
                .iter()
                .find(|item| item.name == server_name)
            else {
                return;
            };
            self.action = Some(PluginMcpSelection::Toggle {
                plugin_id: self.opts.info.id.clone(),
                server: server.name.clone(),
                enabled: !server.enabled,
            });
        }
    }

    fn invalidate(&mut self) {}

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }
}

impl Focusable for PluginMcpSelectorComponent {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

// ===========================================================================
// Remove / install-trust confirmations (choice-picker subclasses)
// ===========================================================================

/// `PluginRemoveConfirmComponent`.
pub fn plugin_remove_confirm_component(
    id: &str,
    display_name: &str,
) -> crate::chrome::ChoicePickerComponent {
    crate::chrome::ChoicePickerComponent::new(crate::chrome::ChoicePickerOptions {
        title: format!("Remove {display_name} ({id})?"),
        hint: Some("↑↓ navigate · Enter/Space select · ←/Esc cancel".to_owned()),
        format_hint: Some(muted_hint_line),
        options: vec![
            crate::chrome::ChoiceOption {
                value: "cancel".to_owned(),
                label: "Cancel".to_owned(),
                description: Some("Keep this plugin installed.".to_owned()),
                tone: None,
                description_tone: None,
            },
            crate::chrome::ChoiceOption {
                value: "remove".to_owned(),
                label: "Remove plugin".to_owned(),
                description: Some(
                    "Remove only the install record; plugin files are left in place.".to_owned(),
                ),
                tone: Some(ColorToken::Error),
                description_tone: None,
            },
        ],
        current_value: None,
        notice: None,
        notice_tone: None,
        searchable: false,
        page_size: None,
        has_session_only: false,
    })
}

/// `PluginInstallTrustConfirmComponent`.
pub fn plugin_install_trust_confirm_component(label: &str) -> crate::chrome::ChoicePickerComponent {
    crate::chrome::ChoicePickerComponent::new(crate::chrome::ChoicePickerOptions {
        title: format!("Install third-party plugin {label}?"),
        hint: Some("↑↓ navigate · Enter/Space select · ←/Esc cancel".to_owned()),
        format_hint: Some(muted_hint_line),
        notice: Some(
            "⚠️ This is a third-party plugin that Dimi has not reviewed. It can bundle MCP \
servers, skills, or files that run code and access your workspace. Install it only if you \
trust the source."
                .to_owned(),
        ),
        notice_tone: Some(ColorToken::Warning),
        options: vec![
            crate::chrome::ChoiceOption {
                value: "exit".to_owned(),
                label: "Exit".to_owned(),
                description: Some("Cancel the installation.".to_owned()),
                tone: None,
                description_tone: None,
            },
            crate::chrome::ChoiceOption {
                value: "trust".to_owned(),
                label: "Trust and install".to_owned(),
                description: Some("Install this third-party plugin anyway.".to_owned()),
                tone: Some(ColorToken::Error),
                description_tone: None,
            },
        ],
        current_value: None,
        searchable: false,
        page_size: None,
        has_session_only: false,
    })
}

// ===========================================================================
// PluginsPanelComponent
// ===========================================================================

#[derive(Debug, Clone)]
enum MarketState {
    Idle,
    Loading,
    Error {
        message: String,
    },
    Loaded {
        entries: Vec<PluginMarketplaceEntry>,
        source: String,
    },
}

/// `PluginsPanelComponent`.
pub struct PluginsPanelComponent {
    opts: PluginsPanelOptions,
    custom_input: InputLine,
    active_tab_index: usize,
    selected_index: usize,
    market: MarketState,
    installing: Option<String>,
    focused: bool,
    action: Option<PluginsPanelAction>,
    marketplace_requested: bool,
}

impl PluginsPanelComponent {
    pub fn new(opts: PluginsPanelOptions) -> Self {
        let active_tab_index = PLUGINS_PANEL_TABS
            .iter()
            .position(|(id, _)| Some(*id) == opts.initial_tab)
            .unwrap_or(0);
        let mut component = PluginsPanelComponent {
            opts,
            custom_input: InputLine::new(),
            active_tab_index,
            selected_index: 0,
            market: MarketState::Idle,
            installing: None,
            focused: false,
            action: None,
            marketplace_requested: false,
        };
        if component.opts.selected_id.is_some()
            && component.active_tab().0 == PluginsPanelTabId::Installed
        {
            if let Some(idx) = component
                .opts
                .installed
                .iter()
                .position(|p| Some(&p.id) == component.opts.selected_id.as_ref())
            {
                component.selected_index = idx;
            }
        }
        component
    }

    /// Host polls after `handle_input`.
    pub fn take_action(&mut self) -> Option<PluginsPanelAction> {
        self.action.take()
    }

    /// Whether the host still owes the marketplace catalog a fetch.
    pub fn take_marketplace_request(&mut self) -> bool {
        let requested = self.marketplace_requested;
        self.marketplace_requested = false;
        requested
    }

    pub fn set_marketplace_loading(&mut self) {
        self.market = MarketState::Loading;
    }

    pub fn set_marketplace(&mut self, entries: Vec<PluginMarketplaceEntry>, source: String) {
        self.market = MarketState::Loaded { entries, source };
    }

    pub fn set_marketplace_error(&mut self, message: String) {
        self.market = MarketState::Error { message };
    }

    pub fn set_installing(&mut self, label: String) {
        self.installing = Some(label);
    }

    pub fn clear_installing(&mut self) {
        self.installing = None;
    }

    fn active_tab(&self) -> (PluginsPanelTabId, &'static str) {
        PLUGINS_PANEL_TABS[self.active_tab_index]
    }

    fn marketplace_entries(&self) -> Vec<PluginMarketplaceEntry> {
        match &self.market {
            MarketState::Loaded { entries, .. } => {
                let mut sorted = entries.clone();
                sorted.sort_by_key(|e| !self.opts.installed_versions.contains_key(&e.id));
                sorted
            }
            _ => Vec::new(),
        }
    }

    fn official_catalog_entries(&self) -> Vec<PluginMarketplaceEntry> {
        let web_id = web_bridge_entry().id;
        self.marketplace_entries()
            .into_iter()
            .filter(|e| e.tier == Some(PluginMarketplaceTier::Official) && e.id != web_id)
            .collect()
    }

    fn third_party_entries(&self) -> Vec<PluginMarketplaceEntry> {
        self.marketplace_entries()
            .into_iter()
            .filter(|e| e.tier != Some(PluginMarketplaceTier::Official))
            .collect()
    }

    fn request_marketplace_if_needed(&mut self) {
        if matches!(self.market, MarketState::Idle)
            && self.active_tab().0 != PluginsPanelTabId::Custom
        {
            self.market = MarketState::Loading;
            self.marketplace_requested = true;
        }
    }

    fn installed_update_status(&self, plugin: &PluginSummary) -> Option<PluginMarketplaceEntry> {
        let MarketState::Loaded { entries, .. } = &self.market else {
            return None;
        };
        let entry = entries.iter().find(|e| e.id == plugin.id)?;
        let status =
            compute_update_status(entry.version.as_deref(), plugin.version.as_deref(), true);
        match status {
            crate::dialogs::plugin_types::PluginUpdateStatus::Update { .. } => Some(entry.clone()),
            _ => None,
        }
    }

    fn installed_hint(&self) -> String {
        let plugin = self.opts.installed.get(self.selected_index);
        let has_update = plugin.is_some_and(|p| self.installed_update_status(p).is_some());
        let enter = if has_update {
            "Enter update"
        } else {
            "Enter details"
        };
        format!(
            " Tab switch · Space toggle · D remove · M MCP · {enter} · I details · R reload · Esc cancel"
        )
    }

    fn render_installed_row(
        &self,
        plugin: &PluginSummary,
        index: usize,
        width: usize,
    ) -> Vec<String> {
        let theme = current_theme();
        let selected = index == self.selected_index;
        let pointer = if selected { SELECT_POINTER } else { " " };
        let label = if selected {
            theme.bold_fg(ColorToken::Primary, &plugin.display_name)
        } else {
            theme.fg(ColorToken::Text, &plugin.display_name)
        };
        let prefix = theme.fg(
            if selected {
                ColorToken::Primary
            } else {
                ColorToken::TextDim
            },
            &format!("  {pointer} "),
        );
        let mut line = format!("{prefix}{label}");
        if let Some(status) = plugin_status(plugin) {
            line.push_str(&format!("  {}", status_style(status, false)));
        }
        if let Some(entry) = self.installed_update_status(plugin) {
            if let crate::dialogs::plugin_types::PluginUpdateStatus::Update { local, latest } =
                compute_update_status(entry.version.as_deref(), plugin.version.as_deref(), true)
            {
                let badge = format!("update {local} → {latest}");
                line.push_str(&format!("  {}", marketplace_status_style(&badge)));
            }
        }
        if let Some((id, text)) = &self.opts.plugin_hint {
            if id == &plugin.id {
                line.push_str(&format!("  {}", theme.fg(ColorToken::Warning, text)));
            }
        }
        let desc_width = (width.saturating_sub(4)).max(1);
        let mut out = vec![line];
        for desc_line in wrap_overview_description(&overview_plugin_description(plugin), desc_width)
        {
            out.push(muted_hint_line(&format!("    {desc_line}")));
        }
        out
    }

    fn render_marketplace_tab(
        &self,
        lines: &mut Vec<String>,
        width: usize,
        entries: &[PluginMarketplaceEntry],
        index_offset: usize,
    ) {
        let theme = current_theme();
        match &self.market {
            MarketState::Loading | MarketState::Idle => {
                lines.push(theme.fg(ColorToken::TextMuted, "  Loading marketplace…"));
                return;
            }
            MarketState::Error { message } => {
                lines.push(theme.fg(
                    ColorToken::Warning,
                    &format!("  Marketplace unavailable: {message}"),
                ));
                lines.push(muted_hint_line(
                    "  Use the Custom tab to install from a URL.",
                ));
                return;
            }
            MarketState::Loaded { .. } => {}
        }
        if entries.is_empty() {
            lines.push(theme.fg(ColorToken::TextMuted, "  No plugins found."));
        } else {
            for (i, entry) in entries.iter().enumerate() {
                lines.extend(self.render_marketplace_row(entry, i + index_offset, width, false));
            }
        }
        let installed_count = entries
            .iter()
            .filter(|e| self.opts.installed_versions.contains_key(&e.id))
            .count();
        lines.push(String::new());
        lines.push(muted_hint_line(&format!(
            " {installed_count} installed · {} available",
            entries.len().saturating_sub(installed_count)
        )));
        if let MarketState::Loaded { source, .. } = &self.market {
            lines.push(muted_hint_line(&format!(" Source: {source}")));
        }
    }

    fn render_marketplace_row(
        &self,
        entry: &PluginMarketplaceEntry,
        index: usize,
        width: usize,
        pinned: bool,
    ) -> Vec<String> {
        let theme = current_theme();
        let selected = index == self.selected_index;
        let pointer = if selected { SELECT_POINTER } else { " " };
        let label = if selected {
            theme.bold_fg(ColorToken::Primary, &entry.display_name)
        } else {
            theme.fg(ColorToken::Text, &entry.display_name)
        };
        let prefix = theme.fg(
            if selected {
                ColorToken::Primary
            } else {
                ColorToken::TextDim
            },
            &format!("  {pointer} "),
        );
        let status = if pinned {
            "open in browser".to_owned()
        } else {
            marketplace_entry_status(entry, &self.opts.installed_versions)
        };
        let line = format!("{prefix}{label}  {}", marketplace_status_style(&status));
        let desc_width = (width.saturating_sub(4)).max(1);
        let mut out = vec![line];
        for desc_line in
            wrap_overview_description(&marketplace_entry_description(entry), desc_width)
        {
            out.push(muted_hint_line(&format!("    {desc_line}")));
        }
        out
    }

    fn render_installed(&self, lines: &mut Vec<String>, width: usize) {
        let theme = current_theme();
        let installed = &self.opts.installed;
        if installed.is_empty() {
            lines.push(theme.fg(ColorToken::TextMuted, "  No plugins installed."));
        } else {
            for (i, plugin) in installed.iter().enumerate() {
                lines.extend(self.render_installed_row(plugin, i, width));
            }
        }
        lines.push(String::new());
        lines.push(muted_hint_line(&format!(" {} installed", installed.len())));
    }

    fn render_custom(&self, lines: &mut Vec<String>, width: usize) {
        lines.push(muted_hint_line(
            " Install from a GitHub URL (or zip URL / local path):",
        ));
        lines.push(String::new());
        lines.extend(self.render_url_input_box(width));
    }

    fn render_url_input_box(&self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let box_width = (width.saturating_sub(2)).max(24);
        let inner_width = (box_width.saturating_sub(4)).max(10);
        let input_line = self.custom_input.render(inner_width);
        let right_pad = inner_width.saturating_sub(visible_width(&input_line));
        let border = |s: String| theme.fg(ColorToken::Primary, &s);
        vec![
            format!(" {}", border(format!("╭{}╮", "─".repeat(box_width - 2)))),
            format!(
                " {}  {input_line}{}{}",
                border("│".to_owned()),
                " ".repeat(right_pad),
                border("│".to_owned())
            ),
            format!(" {}", border(format!("╰{}╯", "─".repeat(box_width - 2)))),
        ]
    }

    fn render_installing(&self, width: usize) -> Vec<String> {
        let theme = current_theme();
        let label = self.installing.as_deref().unwrap_or("");
        let lines = [
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
            theme.bold_fg(ColorToken::Primary, " Plugins"),
            String::new(),
            theme.fg(
                ColorToken::TextMuted,
                &format!("  Installing {label} from marketplace…"),
            ),
            String::new(),
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
        ];
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, ELLIPSIS, false))
            .collect()
    }

    fn handle_installed_input(&mut self, data: &str) {
        let plugins = &self.opts.installed;
        if matches_key(data, "up") {
            self.selected_index = self.selected_index.saturating_sub(1);
            return;
        }
        if matches_key(data, "down") {
            self.selected_index = (self.selected_index + 1).min(plugins.len().saturating_sub(1));
            return;
        }
        let ch = printable_char(data);
        if matches_key(data, "space") || ch == " " {
            if let Some(plugin) = plugins.get(self.selected_index) {
                self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::Toggle {
                    id: plugin.id.clone(),
                    enabled: !plugin.enabled,
                }));
            }
            return;
        }
        if ch == "d" || ch == "D" {
            if let Some(plugin) = plugins.get(self.selected_index) {
                self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::Remove {
                    id: plugin.id.clone(),
                }));
            }
            return;
        }
        if ch == "m" || ch == "M" {
            if let Some(plugin) = plugins.get(self.selected_index) {
                self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::Mcp {
                    id: plugin.id.clone(),
                }));
            }
            return;
        }
        if ch == "r" || ch == "R" {
            self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::Reload));
            return;
        }
        if matches_key(data, "enter") {
            let Some(plugin) = plugins.get(self.selected_index) else {
                return;
            };
            if let Some(entry) = self.installed_update_status(plugin) {
                self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::Install {
                    entry,
                }));
            } else {
                self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::Details {
                    id: plugin.id.clone(),
                }));
            }
            return;
        }
        if ch == "i" || ch == "I" {
            if let Some(plugin) = plugins.get(self.selected_index) {
                self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::Details {
                    id: plugin.id.clone(),
                }));
            }
        }
    }

    fn handle_marketplace_input(&mut self, data: &str) {
        let entries: Vec<PluginMarketplaceEntry> = match self.active_tab().0 {
            PluginsPanelTabId::Official => {
                let mut pinned = vec![web_bridge_entry()];
                pinned.extend(self.official_catalog_entries());
                pinned
            }
            _ => self.third_party_entries(),
        };
        if matches_key(data, "up") {
            self.selected_index = self.selected_index.saturating_sub(1);
            return;
        }
        if matches_key(data, "down") {
            self.selected_index = if entries.is_empty() {
                0
            } else {
                (self.selected_index + 1).min(entries.len() - 1)
            };
            return;
        }
        if matches_key(data, "enter") {
            let Some(entry) = entries.get(self.selected_index) else {
                return;
            };
            // The Web Bridge pinned row only leads the Official tab (TS
            // `isPinnedWebBridgeEntry` reference-checks the pinned constant
            // rendered there); a catalog entry on another tab that happens to
            // reuse the id installs normally instead of being hijacked.
            let is_official_tab = self.active_tab().0 == PluginsPanelTabId::Official;
            if is_official_tab
                && entry.id == web_bridge_entry().id
                && entry.source == WEB_BRIDGE_URL
            {
                self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::OpenUrl {
                    url: WEB_BRIDGE_URL.to_owned(),
                    label: entry.display_name.clone(),
                }));
                return;
            }
            self.action = Some(PluginsPanelAction::Select(PluginsPanelSelection::Install {
                entry: entry.clone(),
            }));
        }
    }
}

const PLUGINS_PANEL_TABS: [(PluginsPanelTabId, &str); 4] = [
    (PluginsPanelTabId::Installed, "Installed"),
    (PluginsPanelTabId::Official, "Official"),
    (PluginsPanelTabId::ThirdParty, "Third-party"),
    (PluginsPanelTabId::Custom, "Custom"),
];

fn marketplace_entry_description(entry: &PluginMarketplaceEntry) -> String {
    let tier = match entry.tier {
        Some(t) => t.label(),
        None => "Plugin",
    };
    let description = entry.description.clone().unwrap_or_else(|| tier.to_owned());
    let version = entry
        .version
        .as_deref()
        .map(|v| format!(" · v{v}"))
        .unwrap_or_default();
    let keywords = if entry.keywords.is_empty() {
        String::new()
    } else {
        format!(" · {}", entry.keywords.join(", "))
    };
    let tier_suffix = if entry.description.is_some() {
        format!(" · {tier}")
    } else {
        String::new()
    };
    format!(
        "{description} · id {}{version}{tier_suffix}{keywords}",
        entry.id
    )
}

fn marketplace_entry_status(
    entry: &PluginMarketplaceEntry,
    installed_versions: &std::collections::HashMap<String, Option<String>>,
) -> String {
    let installed = installed_versions.contains_key(&entry.id);
    let local = installed_versions.get(&entry.id).and_then(|v| v.as_deref());
    let status = compute_update_status(entry.version.as_deref(), local, installed);
    match status {
        crate::dialogs::plugin_types::PluginUpdateStatus::Update { local, latest } => {
            format!("update {local} → {latest}")
        }
        crate::dialogs::plugin_types::PluginUpdateStatus::UpToDate { version } => match version {
            Some(v) => format!("installed · v{v}"),
            None => "installed".to_owned(),
        },
        crate::dialogs::plugin_types::PluginUpdateStatus::NotInstalled => match &entry.version {
            Some(v) => format!("install v{v}"),
            None => "install".to_owned(),
        },
    }
}

impl Component for PluginsPanelComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.installing.is_some() {
            return self.render_installing(width);
        }
        self.custom_input.set_focused(self.focused);
        let theme = current_theme();
        let tab = self.active_tab().0;
        let hint = match tab {
            PluginsPanelTabId::Installed => self.installed_hint(),
            PluginsPanelTabId::Custom => " Tab switch · Enter install · Esc cancel".to_owned(),
            _ => " Tab switch · ↑↓ navigate · Enter open/install · Esc cancel".to_owned(),
        };
        let labels: Vec<&str> = PLUGINS_PANEL_TABS.iter().map(|(_, l)| *l).collect();
        let mut lines: Vec<String> = vec![
            theme.fg(ColorToken::Primary, &"─".repeat(width)),
            theme.bold_fg(ColorToken::Primary, " Plugins"),
            muted_hint_line(&hint),
            String::new(),
            render_tab_strip(&labels, self.active_tab_index, width, theme.palette()),
            String::new(),
        ];
        match tab {
            PluginsPanelTabId::Installed => self.render_installed(&mut lines, width),
            PluginsPanelTabId::Official => {
                lines.extend(self.render_marketplace_row(&web_bridge_entry(), 0, width, true));
                self.render_marketplace_tab(&mut lines, width, &self.official_catalog_entries(), 1);
            }
            PluginsPanelTabId::ThirdParty => {
                let entries = self.third_party_entries();
                self.render_marketplace_tab(&mut lines, width, &entries, 0);
            }
            PluginsPanelTabId::Custom => self.render_custom(&mut lines, width),
        }
        lines.push(theme.fg(ColorToken::Primary, &"─".repeat(width)));
        lines
            .iter()
            .map(|line| truncate_to_width(line, width, ELLIPSIS, false))
            .collect()
    }

    fn handle_input(&mut self, data: &str) {
        if matches_key(data, "escape") {
            self.action = Some(PluginsPanelAction::Cancel);
            return;
        }
        if matches_key(data, "tab") {
            self.active_tab_index = (self.active_tab_index + 1) % PLUGINS_PANEL_TABS.len();
            self.selected_index = 0;
            self.request_marketplace_if_needed();
            return;
        }
        if matches_key(data, "shift+tab") {
            self.active_tab_index =
                (self.active_tab_index + PLUGINS_PANEL_TABS.len() - 1) % PLUGINS_PANEL_TABS.len();
            self.selected_index = 0;
            self.request_marketplace_if_needed();
            return;
        }
        match self.active_tab().0 {
            PluginsPanelTabId::Installed => self.handle_installed_input(data),
            PluginsPanelTabId::Official | PluginsPanelTabId::ThirdParty => {
                self.handle_marketplace_input(data)
            }
            PluginsPanelTabId::Custom => {
                if self.custom_input.handle_input(data) == InputEvent::Submit {
                    let source = self.custom_input.get_value().trim().to_owned();
                    if !source.is_empty() {
                        self.action = Some(PluginsPanelAction::Select(
                            PluginsPanelSelection::InstallSource { source },
                        ));
                    }
                }
            }
        }
    }

    fn invalidate(&mut self) {}

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }
}

impl Focusable for PluginsPanelComponent {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_COLORS, set_palette};

    fn plain(joined: &str) -> String {
        crate::ansi::strip_ansi(joined)
    }

    fn summary(id: &str, enabled: bool) -> PluginSummary {
        PluginSummary {
            id: id.to_owned(),
            display_name: id.to_owned(),
            state: "ok".to_owned(),
            enabled,
            skill_count: 2,
            mcp_server_count: 1,
            enabled_mcp_server_count: 1,
            has_errors: false,
            version: Some("1.0.0".to_owned()),
            source: "github".to_owned(),
            github: Some(("zzj3720".to_owned(), "dimi".to_owned(), "v1.0.0".to_owned())),
            original_source: None,
        }
    }

    #[test]
    fn mcp_selector_renders() {
        set_palette(DARK_COLORS);
        let info = PluginInfo {
            id: "demo".to_owned(),
            display_name: "Demo".to_owned(),
            mcp_servers: vec![
                PluginMcpServerInfo {
                    name: "fs".to_owned(),
                    enabled: true,
                    transport: "stdio",
                    command: Some("node".to_owned()),
                    args: vec!["server.js".to_owned()],
                    url: None,
                    runtime_name: None,
                    cwd: Some("/home/user".to_owned()),
                },
                PluginMcpServerInfo {
                    name: "web".to_owned(),
                    enabled: false,
                    transport: "http",
                    command: None,
                    args: vec![],
                    url: Some("http://localhost:9000".to_owned()),
                    runtime_name: None,
                    cwd: None,
                },
            ],
        };
        let mut c = PluginMcpSelectorComponent::new(PluginMcpSelectorOptions {
            info,
            selected_server: None,
            server_hint: None,
        });
        let joined = plain(&c.render(80).join("\n"));
        assert!(joined.contains("MCP servers · Demo"), "{joined}");
        assert!(joined.contains("MCP servers (1/2 enabled)"), "{joined}");
        assert!(joined.contains("fs"), "{joined}");
        assert!(
            joined.contains("Enter/Space disable · stdio · node server.js · cwd /home/user"),
            "{joined}"
        );
        assert!(joined.contains("Actions"), "{joined}");
        assert!(joined.contains("Back to installed plugins"), "{joined}");
    }

    #[test]
    fn mcp_toggle_action() {
        set_palette(DARK_COLORS);
        let info = PluginInfo {
            id: "demo".to_owned(),
            display_name: "Demo".to_owned(),
            mcp_servers: vec![PluginMcpServerInfo {
                name: "fs".to_owned(),
                enabled: true,
                transport: "stdio",
                command: None,
                args: vec![],
                url: None,
                runtime_name: Some("node".to_owned()),
                cwd: None,
            }],
        };
        let mut c = PluginMcpSelectorComponent::new(PluginMcpSelectorOptions {
            info,
            selected_server: None,
            server_hint: None,
        });
        c.handle_input("\r");
        assert_eq!(
            c.take_action(),
            Some(PluginMcpSelection::Toggle {
                plugin_id: "demo".to_owned(),
                server: "fs".to_owned(),
                enabled: false,
            })
        );
    }

    #[test]
    fn confirm_dialogs_build() {
        set_palette(DARK_COLORS);
        let mut remove = plugin_remove_confirm_component("demo", "Demo Plugin");
        let joined = plain(&remove.render(80).join("\n"));
        assert!(joined.contains("Remove Demo Plugin (demo)?"), "{joined}");
        assert!(joined.contains("Remove plugin"), "{joined}");

        let mut trust = plugin_install_trust_confirm_component("demo");
        let joined = plain(&trust.render(80).join("\n"));
        assert!(
            joined.contains("Install third-party plugin demo?"),
            "{joined}"
        );
        assert!(joined.contains("Trust and install"), "{joined}");
        assert!(joined.contains("has not reviewed"), "{joined}");
    }

    #[test]
    fn plugins_panel_installed_tab() {
        set_palette(DARK_COLORS);
        let opts = PluginsPanelOptions {
            installed: vec![summary("alpha", true), summary("beta", false)],
            installed_versions: std::collections::HashMap::new(),
            initial_tab: Some(PluginsPanelTabId::Installed),
            selected_id: None,
            plugin_hint: None,
        };
        let mut c = PluginsPanelComponent::new(opts);
        let joined = plain(&c.render(80).join("\n"));
        assert!(joined.contains("Plugins"), "{joined}");
        assert!(joined.contains(" Installed "), "{joined}");
        assert!(joined.contains("alpha"), "{joined}");
        assert!(joined.contains("beta"), "{joined}");
        assert!(joined.contains("Space toggle"), "{joined}");
    }

    #[test]
    fn plugins_panel_requests_marketplace_on_tab_switch() {
        set_palette(DARK_COLORS);
        let opts = PluginsPanelOptions {
            installed: vec![],
            installed_versions: std::collections::HashMap::new(),
            initial_tab: Some(PluginsPanelTabId::Installed),
            selected_id: None,
            plugin_hint: None,
        };
        let mut c = PluginsPanelComponent::new(opts);
        assert!(!c.take_marketplace_request());
        c.handle_input("\t"); // → Official
        assert!(c.take_marketplace_request());
        // Now loading: render shows the loading line.
        let joined = plain(&c.render(80).join("\n"));
        assert!(joined.contains("Loading marketplace…"), "{joined}");
    }
}
