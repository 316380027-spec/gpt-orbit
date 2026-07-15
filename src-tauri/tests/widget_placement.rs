use std::fs;

use codex_orbit_lib::desktop::app_variant::AppVariant;
use codex_orbit_lib::desktop::preferences::{
    desktop_preferences_path, load_from_path, save_to_path, DesktopPreferences,
};

#[test]
fn desktop_preferences_use_camel_case_and_default_to_always_on_top() {
    let preferences = DesktopPreferences {
        always_on_top: true,
        monitor_name: Some("Primary".to_string()),
        offset_logical: Some((24.5, 32.0)),
        scale_factor: Some(1.25),
    };

    let serialized = serde_json::to_value(&preferences).expect("serialize preferences");

    assert_eq!(serialized["alwaysOnTop"], true);
    assert_eq!(serialized["monitorName"], "Primary");
    assert_eq!(serialized["offsetLogical"][0], 24.5);
    assert_eq!(serialized["scaleFactor"], 1.25);
    assert!(serialized.get("always_on_top").is_none());
    assert!(DesktopPreferences::default().always_on_top);
}

#[test]
fn save_and_load_desktop_preferences_round_trip_under_app_data() {
    let unique = format!(
        "gpt-orbit-widget-placement-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    let path = desktop_preferences_path(&dir);
    let preferences = DesktopPreferences {
        always_on_top: false,
        monitor_name: Some("Side".to_string()),
        offset_logical: Some((10.0, 20.0)),
        scale_factor: Some(2.0),
    };

    save_to_path(&path, &preferences).expect("save preferences");

    assert_eq!(
        load_from_path(&path).expect("load preferences"),
        preferences
    );
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("desktop-preferences-v1.json")
    );

    fs::remove_dir_all(dir).expect("remove temp preferences");
}

#[test]
fn restore_selects_collapsed_canvas_from_the_configured_identifier() {
    let source = include_str!("../src/desktop/preferences.rs");

    assert!(source.contains("AppVariant::from_identifier(&app.config().identifier)"));
    assert!(source.contains("canvas.collapsed_width"));
    assert!(source.contains("canvas.collapsed_height"));

    let variants = include_str!("../src/desktop/app_variant.rs");
    assert!(variants.contains("collapsed_width: 172.0"));
    assert!(variants.contains("collapsed_width: 104.0"));

    let standard = AppVariant::from_identifier("com.codex-orbit.app").widget_canvas();
    let weekly = AppVariant::from_identifier("com.codex-orbit.weekly").widget_canvas();
    assert_eq!(
        (standard.collapsed_width, standard.collapsed_height),
        (172.0, 172.0)
    );
    assert_eq!(
        (weekly.collapsed_width, weekly.collapsed_height),
        (104.0, 86.0)
    );
}

#[test]
fn restore_reapplies_compact_weekly_style_after_showing_the_window() {
    let source = include_str!("../src/desktop/preferences.rs");
    let show = source
        .find(".show()")
        .expect("restore should show the window");
    let compact = source
        .find("configure_compact_window(window, variant)")
        .expect("restore should reapply the compact weekly style");

    assert!(
        show < compact,
        "show must happen before compact style reapplication"
    );
}

#[test]
fn move_command_delegates_position_only_persistence_to_the_native_window() {
    let source = include_str!("../src/desktop/preferences.rs");
    let command = source
        .split("pub fn save_widget_placement")
        .nth(1)
        .expect("save widget placement command")
        .split("#[tauri::command]")
        .next()
        .expect("end of save widget placement command");
    let signature = command
        .split('{')
        .next()
        .expect("save widget placement signature");

    assert!(signature.contains("window: WebviewWindow<R>"));
    assert!(!signature.contains("DesktopPreferences"));
    assert!(command.contains("save_current_widget_placement(app, &window)"));
}
