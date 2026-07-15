use codex_orbit_lib::backend::RefreshReason;
use codex_orbit_lib::desktop::tray::{
    action_for_tray_id, tray_menu_spec, TrayAction, ALWAYS_ON_TOP_ID, QUIT_ID, REFRESH_ID,
    SHOW_HIDE_ID,
};

#[test]
fn tray_menu_uses_exact_ids_and_labels_once() {
    let items = tray_menu_spec();

    assert_eq!(items.len(), 4);
    assert_eq!(items[0].id, SHOW_HIDE_ID);
    assert_eq!(items[0].label, "显示/隐藏");
    assert_eq!(items[1].id, REFRESH_ID);
    assert_eq!(items[1].label, "刷新额度");
    assert_eq!(items[2].id, ALWAYS_ON_TOP_ID);
    assert_eq!(items[2].label, "始终置顶");
    assert!(items[2].checked);
    assert_eq!(items[3].id, QUIT_ID);
    assert_eq!(items[3].label, "退出");

    let unique_ids = items
        .iter()
        .map(|item| item.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique_ids.len(), 4, "tray items must not be duplicated");
}

#[test]
fn tray_ids_map_to_exact_actions() {
    assert_eq!(action_for_tray_id(SHOW_HIDE_ID), Some(TrayAction::ShowHide));
    assert_eq!(
        action_for_tray_id(REFRESH_ID),
        Some(TrayAction::Refresh(RefreshReason::Tray))
    );
    assert_eq!(
        action_for_tray_id(ALWAYS_ON_TOP_ID),
        Some(TrayAction::ToggleAlwaysOnTop)
    );
    assert_eq!(action_for_tray_id(QUIT_ID), Some(TrayAction::Quit));
    assert_eq!(action_for_tray_id("unknown"), None);
}

#[test]
fn weekly_tray_refresh_and_window_show_request_both_services() {
    let source = include_str!("../src/desktop/tray.rs");

    assert!(
        source
            .matches("try_state::<crate::backend::ResetCreditService>()")
            .count()
            >= 3
    );
    assert!(source.matches("crate::request_service_refreshes(").count() >= 2);
    assert!(source.contains("RefreshReason::WindowShown,"));
    assert!(source.contains("reset_service.shutdown().await"));
}

#[test]
fn tray_tooltip_uses_configured_product_name_with_standard_fallback() {
    let source = include_str!("../src/desktop/tray.rs");

    assert!(source.contains(".product_name"));
    assert!(source.contains("unwrap_or(\"Gpt Orbit\")"));
    assert!(source.contains(".tooltip(tooltip)"));
    assert!(!source.contains(".tooltip(\"Gpt Orbit\")"));
}
