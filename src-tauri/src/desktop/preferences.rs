use std::path::{Path, PathBuf};

use crate::desktop::{app_variant::AppVariant, compact_window::configure_compact_window};
use tauri::{LogicalPosition, LogicalSize, Manager, Runtime, WebviewWindow};

const DEFAULT_RESTORE_MARGIN: f64 = 24.0;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPreferences {
    pub always_on_top: bool,
    pub monitor_name: Option<String>,
    pub offset_logical: Option<(f64, f64)>,
    pub scale_factor: Option<f64>,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            always_on_top: true,
            monitor_name: None,
            offset_logical: None,
            scale_factor: None,
        }
    }
}

pub fn desktop_preferences_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("desktop-preferences-v1.json")
}

pub fn load_from_path(path: &Path) -> Result<DesktopPreferences, String> {
    if !path.exists() {
        return Ok(DesktopPreferences::default());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read desktop preferences: {error}"))?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse desktop preferences: {error}"))
}

pub fn save_to_path(path: &Path, preferences: &DesktopPreferences) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create desktop preference directory: {error}"))?;
    }
    let raw = serde_json::to_string_pretty(preferences)
        .map_err(|error| format!("failed to serialize desktop preferences: {error}"))?;
    std::fs::write(path, raw)
        .map_err(|error| format!("failed to write desktop preferences: {error}"))
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

pub fn persist_always_on_top<R: Runtime>(
    app: tauri::AppHandle<R>,
    enabled: bool,
) -> Result<(), String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let path = desktop_preferences_path(&app_data);
    let mut preferences = load_from_path(&path)?;
    preferences.always_on_top = enabled;
    save_to_path(&path, &preferences)
}

pub fn save_current_widget_placement<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: &WebviewWindow<R>,
) -> Result<(), String> {
    let monitor = window
        .current_monitor()
        .map_err(|error| format!("failed to read current monitor: {error}"))?
        .ok_or_else(|| "failed to resolve current monitor".to_string())?;
    let scale_factor = monitor.scale_factor();
    let position = window
        .outer_position()
        .map_err(|error| format!("failed to read window position: {error}"))?
        .to_logical::<f64>(scale_factor);
    let work_area = monitor.work_area();
    let work_position = work_area.position.to_logical::<f64>(scale_factor);

    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    let path = desktop_preferences_path(&app_data);
    let mut preferences = load_from_path(&path)?;
    preferences.monitor_name = monitor.name().cloned();
    preferences.offset_logical = Some((position.x - work_position.x, position.y - work_position.y));
    preferences.scale_factor = Some(scale_factor);
    save_to_path(&path, &preferences)
}

pub fn restore_widget_window<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: &WebviewWindow<R>,
) -> Result<(), String> {
    let variant = AppVariant::from_identifier(&app.config().identifier);
    let canvas = variant.widget_canvas();
    let preferences = load_desktop_preferences(app)?;
    let monitors = window
        .available_monitors()
        .map_err(|error| format!("failed to read available monitors: {error}"))?;
    let primary = window
        .primary_monitor()
        .map_err(|error| format!("failed to read primary monitor: {error}"))?;
    let selected = monitors
        .iter()
        .find(|monitor| monitor.name() == preferences.monitor_name.as_ref())
        .or(primary.as_ref())
        .or_else(|| monitors.first())
        .ok_or_else(|| "failed to resolve restore monitor".to_string())?;
    let scale_factor = selected.scale_factor();
    let work_area = selected.work_area();
    let work_position = work_area.position.to_logical::<f64>(scale_factor);
    let work_size = work_area.size.to_logical::<f64>(scale_factor);
    let (offset_x, offset_y) = preferences.offset_logical.unwrap_or((
        work_size.width - canvas.collapsed_width - DEFAULT_RESTORE_MARGIN,
        DEFAULT_RESTORE_MARGIN,
    ));
    let x = clamp(
        work_position.x + offset_x,
        work_position.x,
        work_position.x + work_size.width - canvas.collapsed_width,
    );
    let y = clamp(
        work_position.y + offset_y,
        work_position.y,
        work_position.y + work_size.height - canvas.collapsed_height,
    );

    window
        .set_always_on_top(preferences.always_on_top)
        .map_err(|error| format!("failed to restore always-on-top: {error}"))?;
    window
        .set_size(LogicalSize::new(
            canvas.collapsed_width,
            canvas.collapsed_height,
        ))
        .map_err(|error| format!("failed to restore widget size: {error}"))?;
    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|error| format!("failed to restore widget position: {error}"))?;
    window
        .show()
        .map_err(|error| format!("failed to show widget window: {error}"))?;
    configure_compact_window(window, variant)
        .map_err(|error| format!("failed to configure compact widget: {error}"))?;
    window
        .set_size(LogicalSize::new(
            canvas.collapsed_width,
            canvas.collapsed_height,
        ))
        .map_err(|error| format!("failed to reapply widget size: {error}"))?;
    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|error| format!("failed to reapply widget position: {error}"))
}

#[tauri::command]
pub fn load_desktop_preferences<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<DesktopPreferences, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?;
    load_from_path(&desktop_preferences_path(&app_data))
}

#[tauri::command]
pub fn save_widget_placement<R: Runtime>(
    app: tauri::AppHandle<R>,
    window: WebviewWindow<R>,
) -> Result<(), String> {
    save_current_widget_placement(app, &window)
}

#[tauri::command]
pub fn set_always_on_top<R: Runtime>(
    window: tauri::Window<R>,
    enabled: bool,
) -> Result<(), String> {
    window
        .set_always_on_top(enabled)
        .map_err(|error| format!("failed to set always-on-top: {error}"))
}
