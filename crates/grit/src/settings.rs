//! App settings: global state + persistence, and the Settings window opened
//! from the macOS menu bar ("Settings…", ⌘,).
//!
//! Holds the Agent launcher command and the background auto-refresh
//! controls (toggle + status refresh / fetch intervals).

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::theme::Theme;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::github;

/// Command run in the launched Agent terminal tab.
const DEFAULT_AGENT_COMMAND: &str = "opencode -c";

/// Default seconds between background status reloads.
const DEFAULT_AUTO_REFRESH_SECS: u32 = 10;
/// Default seconds between background `git fetch`es.
const DEFAULT_AUTO_FETCH_SECS: u32 = 300;
/// Bounds of the auto-refresh interval (seconds).
const MIN_AUTO_REFRESH_SECS: u32 = 2;
const MAX_AUTO_REFRESH_SECS: u32 = 3_600;
/// Hard ceiling of the auto-fetch interval (seconds; 0 disables fetching).
const MAX_AUTO_FETCH_SECS: u32 = 86_400;

/// App-wide settings, stored as a GPUI Global and persisted to
/// `<data dir>/grit/settings.json`. The setters persist on every change.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GritSettings {
    /// Command run in the Agent terminal tab.
    pub(crate) agent_command: String,
    /// Whether status / branches / log reload periodically in the background.
    pub(crate) auto_refresh_enabled: bool,
    /// Seconds between background status reloads.
    pub(crate) auto_refresh_interval_secs: u32,
    /// Seconds between background `git fetch`es (0 = never).
    pub(crate) auto_fetch_interval_secs: u32,
}

impl Default for GritSettings {
    fn default() -> Self {
        Self {
            agent_command: DEFAULT_AGENT_COMMAND.into(),
            auto_refresh_enabled: true,
            auto_refresh_interval_secs: DEFAULT_AUTO_REFRESH_SECS,
            auto_fetch_interval_secs: DEFAULT_AUTO_FETCH_SECS,
        }
    }
}

impl Global for GritSettings {}

impl GritSettings {
    fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }
}

/// The agent command setting, read by the workspace Agent launcher. A value
/// cleared in the Settings window falls back to the default so the launcher
/// never runs an empty command.
pub(crate) fn agent_command(cx: &App) -> String {
    match GritSettings::global(cx).agent_command.trim() {
        "" => DEFAULT_AGENT_COMMAND.to_string(),
        command => command.to_string(),
    }
}

/// Whether background auto-refresh (status + fetch + fs watcher) is on.
pub(crate) fn auto_refresh_enabled(cx: &App) -> bool {
    GritSettings::global(cx).auto_refresh_enabled
}

/// Seconds between background status reloads, clamped to the Settings UI's
/// bounds (the accessor is the single place the loop reads the value, so a
/// hand-edited settings.json cannot instantly re-tick 60 times a second).
pub(crate) fn auto_refresh_interval(cx: &App) -> u64 {
    (GritSettings::global(cx).auto_refresh_interval_secs as u64)
        .clamp(MIN_AUTO_REFRESH_SECS as u64, MAX_AUTO_REFRESH_SECS as u64)
}

/// Seconds between background `git fetch`es (0 = never), clamped like the
/// refresh interval.
pub(crate) fn auto_fetch_interval(cx: &App) -> u64 {
    (GritSettings::global(cx).auto_fetch_interval_secs as u64).min(MAX_AUTO_FETCH_SECS as u64)
}

/// Load persisted settings (defaults when the file is absent or invalid) and
/// register the Global. Called once at startup, before any window opens.
pub(crate) fn init(cx: &mut App) {
    let data = file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<GritSettings>(&text).ok())
        .unwrap_or_default();
    cx.set_global(data);
}

fn file() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("grit");
    std::fs::create_dir_all(&path).ok()?;
    path.push("settings.json");
    Some(path)
}

/// Write the current settings to disk. Cheap: the file is a few bytes.
fn save(cx: &App) {
    let Some(path) = file() else { return };
    if let Ok(content) = serde_json::to_string_pretty(GritSettings::global(cx)) {
        std::fs::write(path, content).ok();
    }
}

// ---- settings window --------------------------------------------------------

/// Open the Settings window from the menu bar action (⌘,).
pub(crate) fn open_settings_window(cx: &mut App) {
    let bounds = WindowBounds::centered(size(px(760.), px(520.)), cx);
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions { title: Some("Settings".into()), ..Default::default() }),
        window_bounds: Some(bounds),
        ..Default::default()
    };
    open_window(options, cx, |window, cx| {
        // Follow the system light/dark appearance, like the other windows.
        window
            .observe_window_appearance(|window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
            })
            .detach();
        cx.new(|cx| {
            let github_panel = cx.new(|cx| github::SignInPanel::new(&github::session(cx), cx));
            SettingsWindow { github_panel }
        })
    })
    .expect("failed to open Settings window");
}

/// Retained view hosting the GPUI Kit `Settings` component.
struct SettingsWindow {
    /// The GitHub sign-in panel; the page field re-renders the same view
    /// entity, whose own observation keeps it current with the session.
    github_panel: Entity<github::SignInPanel>,
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(cx.theme().background).text_color(cx.theme().foreground).child(
            Settings::new("grit-settings")
                .with_group_variant(GroupBoxVariant::Outline)
                .pages(vec![general_page(), github_page(self.github_panel.clone())]),
        )
    }
}

/// The "General" page: the Agent launcher, then the background auto-refresh
/// controls.
fn general_page() -> SettingPage {
    SettingPage::new("General")
        .icon(IconName::Settings)
        .resettable(true)
        .groups(vec![agent_group(), auto_refresh_group()])
}

/// The "GitHub" page: the OAuth Device Flow sign-in panel (status, the
/// one-time code or the failure, sign-out).
fn github_page(panel: Entity<github::SignInPanel>) -> SettingPage {
    SettingPage::new("GitHub").icon(IconName::Github).groups(vec![
        SettingGroup::new()
            .title("Account")
            .item(SettingItem::render(move |_, _, _| panel.clone().into_any_element()))
            .footer(|_, _| "Sign-in uses GitHub's Device Flow; no password ever touches Grit."),
    ])
}

/// The Agent launcher command.
fn agent_group() -> SettingGroup {
    SettingGroup::new().title("Agent").item(
        SettingItem::new(
            "Command",
            SettingField::input(
                |cx: &App| GritSettings::global(cx).agent_command.clone().into(),
                |val: SharedString, cx: &mut App| {
                    GritSettings::global_mut(cx).agent_command = val.to_string();
                    save(cx);
                },
            )
            .default_value(DEFAULT_AGENT_COMMAND),
        )
        .description("Command run in the Grit Agent tab (`warp://tab_config/grit-agent`).")
        .layout(Axis::Vertical),
    )
}

/// Background auto-refresh: toggle + refresh / fetch intervals.
fn auto_refresh_group() -> SettingGroup {
    SettingGroup::new().title("Auto-refresh").items(vec![
        SettingItem::new(
            "Auto refresh",
            SettingField::switch(auto_refresh_enabled, |val: bool, cx: &mut App| {
                GritSettings::global_mut(cx).auto_refresh_enabled = val;
                save(cx);
            }),
        )
        .description(
            "Keep status, branches and the commit log in sync in the background \
             (periodic reload, automatic fetches and file watching).",
        ),
        SettingItem::new(
            "Status refresh interval",
            SettingField::number_input(
                NumberFieldOptions {
                    min: MIN_AUTO_REFRESH_SECS as f64,
                    max: MAX_AUTO_REFRESH_SECS as f64,
                    ..Default::default()
                },
                |cx: &App| auto_refresh_interval(cx) as f64,
                |val: f64, cx: &mut App| {
                    GritSettings::global_mut(cx).auto_refresh_interval_secs =
                        (val as u32).clamp(MIN_AUTO_REFRESH_SECS, MAX_AUTO_REFRESH_SECS);
                    save(cx);
                },
            ),
        )
        .description("Seconds between background status reloads."),
        SettingItem::new(
            "Fetch interval",
            SettingField::number_input(
                NumberFieldOptions {
                    min: 0.,
                    max: MAX_AUTO_FETCH_SECS as f64,
                    ..Default::default()
                },
                |cx: &App| auto_fetch_interval(cx) as f64,
                |val: f64, cx: &mut App| {
                    GritSettings::global_mut(cx).auto_fetch_interval_secs =
                        (val as u32).min(MAX_AUTO_FETCH_SECS);
                    save(cx);
                },
            ),
        )
        .description("Seconds between automatic `git fetch`es (0 disables)."),
    ])
}
