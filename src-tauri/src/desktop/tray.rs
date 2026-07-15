use crate::backend::RefreshReason;
use crate::desktop::{persist_always_on_top, restore_widget_window, save_current_widget_placement};
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, Menu, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const SHOW_HIDE_ID: &str = "show-hide";
pub const REFRESH_ID: &str = "refresh";
pub const ALWAYS_ON_TOP_ID: &str = "always-on-top";
pub const QUIT_ID: &str = "quit";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayMenuItemSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub checked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    ShowHide,
    Refresh(RefreshReason),
    ToggleAlwaysOnTop,
    Quit,
}

struct DesktopTrayState<R: Runtime> {
    always_on_top: CheckMenuItem<R>,
}

pub fn tray_menu_spec() -> Vec<TrayMenuItemSpec> {
    vec![
        TrayMenuItemSpec {
            id: SHOW_HIDE_ID,
            label: "显示/隐藏",
            checked: false,
        },
        TrayMenuItemSpec {
            id: REFRESH_ID,
            label: "刷新额度",
            checked: false,
        },
        TrayMenuItemSpec {
            id: ALWAYS_ON_TOP_ID,
            label: "始终置顶",
            checked: true,
        },
        TrayMenuItemSpec {
            id: QUIT_ID,
            label: "退出",
            checked: false,
        },
    ]
}

pub fn action_for_tray_id(id: &str) -> Option<TrayAction> {
    match id {
        SHOW_HIDE_ID => Some(TrayAction::ShowHide),
        REFRESH_ID => Some(TrayAction::Refresh(RefreshReason::Tray)),
        ALWAYS_ON_TOP_ID => Some(TrayAction::ToggleAlwaysOnTop),
        QUIT_ID => Some(TrayAction::Quit),
        _ => None,
    }
}

pub fn install_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show_hide = MenuItemBuilder::with_id(SHOW_HIDE_ID, "显示/隐藏").build(app)?;
    let refresh = MenuItemBuilder::with_id(REFRESH_ID, "刷新额度").build(app)?;
    let always_on_top = CheckMenuItemBuilder::with_id(ALWAYS_ON_TOP_ID, "始终置顶")
        .checked(true)
        .build(app)?;
    let quit = MenuItemBuilder::with_id(QUIT_ID, "退出").build(app)?;
    let menu = Menu::with_items(app, &[&show_hide, &refresh, &always_on_top, &quit])?;
    app.manage(DesktopTrayState {
        always_on_top: always_on_top.clone(),
    });
    let tooltip = app.config().product_name.as_deref().unwrap_or("Gpt Orbit");

    let mut builder = TrayIconBuilder::with_id("gpt-orbit")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip(tooltip)
        .on_menu_event(|app, event| {
            if let Some(action) = action_for_tray_id(&event.id().0) {
                handle_tray_action(app, action);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn handle_tray_action<R: Runtime>(app: &AppHandle<R>, action: TrayAction) {
    match action {
        TrayAction::ShowHide => {
            if let Some(window) = app.get_webview_window("main") {
                let visible = window.is_visible().unwrap_or(false);
                if visible {
                    let _ = window.hide();
                    let _ = app.emit(
                        "desktop://visibility",
                        serde_json::json!({ "visible": false }),
                    );
                } else {
                    let _ = restore_widget_window(app.clone(), &window);
                    let _ = app.emit(
                        "desktop://visibility",
                        serde_json::json!({ "visible": true }),
                    );
                    if let Some(service) = app.try_state::<crate::backend::BackendService>() {
                        let reset_service = app.try_state::<crate::backend::ResetCreditService>();
                        crate::request_service_refreshes(
                            RefreshReason::WindowShown,
                            |reason| {
                                let _ = service.refresh_now(reason);
                            },
                            reset_service.as_ref().map(|reset_service| {
                                |reason| {
                                    let _ = reset_service.refresh_now(reason);
                                }
                            }),
                        );
                    }
                }
            }
        }
        TrayAction::Refresh(reason) => {
            if let Some(service) = app.try_state::<crate::backend::BackendService>() {
                let reset_service = app.try_state::<crate::backend::ResetCreditService>();
                crate::request_service_refreshes(
                    reason,
                    |reason| {
                        let _ = service.refresh_now(reason);
                    },
                    reset_service.as_ref().map(|reset_service| {
                        |reason| {
                            let _ = reset_service.refresh_now(reason);
                        }
                    }),
                );
            }
        }
        TrayAction::ToggleAlwaysOnTop => {
            if let Some(window) = app.get_webview_window("main") {
                let enabled = !window.is_always_on_top().unwrap_or(true);
                let _ = window.set_always_on_top(enabled);
                let _ = persist_always_on_top(app.clone(), enabled);
                if let Some(tray_state) = app.try_state::<DesktopTrayState<R>>() {
                    let _ = tray_state.always_on_top.set_checked(enabled);
                }
                let _ = app.emit(
                    "desktop://always-on-top",
                    serde_json::json!({ "enabled": enabled }),
                );
            }
        }
        TrayAction::Quit => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = save_current_widget_placement(app.clone(), &window);
            }
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(service) = handle.try_state::<crate::backend::BackendService>() {
                    let _ = service.shutdown().await;
                }
                if let Some(reset_service) =
                    handle.try_state::<crate::backend::ResetCreditService>()
                {
                    let _ = reset_service.shutdown().await;
                }
                handle.exit(0);
            });
        }
    }
}
