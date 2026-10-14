//! App settings: global state + persistence, and the Settings window opened
//! from the macOS menu bar ("Settings…", ⌘,).
//!
//! Currently holds the command the Agent toolbar shortcut runs in the Warp
//! tab Grit generates (`warp://tab_config/grit-agent`).

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::theme::Theme;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Command run in the launched Agent terminal tab.
const DEFAULT_AGENT_COMMAND: &str = "opencode -c";

/// App-wide settings, stored as a GPUI Global and persisted to
/// `<data dir>/grit/settings.json`. Fields are agent-launcher related; the
/// setters persist on every change.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GritSettings {
    /// Command run in the Agent terminal tab.
    pub(crate) agent_command: String,
}

impl Default for GritSettings {
    fn default() -> Self {
        Self { agent_command: DEFAULT_AGENT_COMMAND.into() }
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
        cx.new(|_| SettingsWindow)
    })
    .expect("failed to open Settings window");
}

/// Retained view hosting the GPUI Kit `Settings` component.
struct SettingsWindow;

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(cx.theme().background).text_color(cx.theme().foreground).child(
            Settings::new("grit-settings")
                .with_group_variant(GroupBoxVariant::Outline)
                .pages(vec![agent_page()]),
        )
    }
}

/// The "General" page: an Agent group with the launcher command.
fn agent_page() -> SettingPage {
    SettingPage::new("General").icon(IconName::Settings).resettable(true).group(
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
        ),
    )
}
