pub mod backend;
pub mod desktop;

use backend::{
    resolve_codex_executable, BackendService, RateLimitCache, ResetCreditCache, ResetCreditClient,
    ResetCreditService,
};
use desktop::app_variant::AppVariant;
use desktop::session_unlock::{install_session_unlock_handler, SessionUnlockRegistration};
use desktop::{
    install_tray, load_desktop_preferences, restore_widget_window, save_widget_placement,
    set_always_on_top,
};
use std::path::PathBuf;
use tauri::{Emitter, Manager};

#[doc(hidden)]
pub fn start_reset_credit_for_variant<T, E>(
    identifier: &str,
    start: impl FnOnce() -> Result<T, E>,
) -> Option<T> {
    if AppVariant::from_identifier(identifier) == AppVariant::Weekly {
        match start() {
            Ok(service) => Some(service),
            Err(_) => {
                tracing::warn!(category = "reset_credit_setup_failed");
                None
            }
        }
    } else {
        None
    }
}

#[doc(hidden)]
pub fn restore_then_refresh_optional_service<T>(
    service: Option<&T>,
    restore: impl FnOnce(),
    refresh: impl FnOnce(&T),
) {
    restore();
    if let Some(service) = service {
        refresh(service);
    }
}

#[doc(hidden)]
pub fn request_service_refreshes<B, R>(
    reason: backend::RefreshReason,
    backend: B,
    reset_credit: Option<R>,
) where
    B: FnOnce(backend::RefreshReason),
    R: FnOnce(backend::RefreshReason),
{
    backend(reason);
    if let Some(reset_credit) = reset_credit {
        reset_credit(reason);
    }
}

fn refresh_managed_services(app: &tauri::AppHandle, reason: backend::RefreshReason) {
    if let Some(service) = app.try_state::<BackendService>() {
        let reset_service = app.try_state::<ResetCreditService>();
        request_service_refreshes(
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

mod commands {
    use super::{backend, BackendService, ResetCreditService};
    use tauri::Manager;

    pub fn refresh_reason_from_frontend(reason: &str) -> backend::RefreshReason {
        match reason {
            "resetExpired" => backend::RefreshReason::ResetExpired,
            "tray" => backend::RefreshReason::Tray,
            "windowShown" => backend::RefreshReason::WindowShown,
            "resume" => backend::RefreshReason::Resume,
            "sessionUnlocked" => backend::RefreshReason::SessionUnlocked,
            _ => backend::RefreshReason::Manual,
        }
    }

    #[tauri::command]
    pub fn get_rate_limits(
        service: tauri::State<'_, BackendService>,
    ) -> Option<backend::RateLimitState> {
        service.current_rate_limits()
    }

    #[tauri::command]
    pub fn get_quota_bridge_state(
        service: tauri::State<'_, BackendService>,
    ) -> backend::QuotaBridgeState {
        service.current_bridge_state()
    }

    #[tauri::command]
    pub fn refresh_rate_limits(
        service: tauri::State<'_, BackendService>,
        reason: String,
    ) -> Result<(), String> {
        service
            .refresh_now(refresh_reason_from_frontend(&reason))
            .map_err(|error| format!("failed to refresh rate limits: {error}"))
    }

    #[tauri::command]
    pub fn get_reset_credits(app: tauri::AppHandle) -> Option<backend::ResetCreditState> {
        app.try_state::<ResetCreditService>()
            .and_then(|service| service.current())
    }

    #[tauri::command]
    pub fn refresh_reset_credits(app: tauri::AppHandle, reason: String) -> Result<(), String> {
        if let Some(service) = app.try_state::<ResetCreditService>() {
            service
                .refresh_now(refresh_reason_from_frontend(&reason))
                .map_err(|error| format!("failed to refresh reset credits: {error}"))?;
        }
        Ok(())
    }
}

pub use commands::{
    get_quota_bridge_state, get_rate_limits, get_reset_credits, refresh_rate_limits,
    refresh_reset_credits,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_rate_limits,
            get_quota_bridge_state,
            refresh_rate_limits,
            get_reset_credits,
            refresh_reset_credits,
            load_desktop_preferences,
            save_widget_placement,
            set_always_on_top
        ])
        .setup(|app| {
            let cache_path = app.path().app_data_dir()?.join("rate-limits-v1.json");
            let service = tauri::async_runtime::block_on(BackendService::start(
                app.handle().clone(),
                resolve_codex_executable(),
                RateLimitCache::new(cache_path),
            ))?;
            app.manage(service);
            let reset_service = start_reset_credit_for_variant(
                &app.config().identifier,
                || -> Result<ResetCreditService, Box<dyn std::error::Error>> {
                    let reset_cache_path = app.path().app_data_dir()?.join("reset-credits-v1.json");
                    let codex_home = std::env::var_os("CODEX_HOME")
                        .map(PathBuf::from)
                        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
                        .ok_or_else(|| std::io::Error::other("unable to resolve Codex home"))?;
                    let reset_client = ResetCreditClient::production(codex_home.join("auth.json"))?;
                    let reset_service = tauri::async_runtime::block_on(ResetCreditService::start(
                        app.handle().clone(),
                        reset_client,
                        ResetCreditCache::new(reset_cache_path),
                    ))?;
                    Ok(reset_service)
                },
            );
            if let Some(reset_service) = &reset_service {
                app.manage(reset_service.clone());
            }
            if let Some(window) = app.get_webview_window("main") {
                let unlock_app = app.handle().clone();
                match install_session_unlock_handler(&window, move || {
                    refresh_managed_services(&unlock_app, backend::RefreshReason::SessionUnlocked);
                }) {
                    Ok(Some(registration)) => {
                        app.manage(registration);
                    }
                    Ok(None) => {}
                    Err(category) => tracing::warn!(category = category),
                }
            }
            install_tray(app.handle())?;
            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.hide();
                        }
                        let _ = app_handle.emit(
                            "desktop://visibility",
                            serde_json::json!({ "visible": false }),
                        );
                    }
                });
            }
            restore_then_refresh_optional_service(
                reset_service.as_ref(),
                || {
                    if let Some(window) = app.get_webview_window("main") {
                        if restore_widget_window(app.handle().clone(), &window).is_err() {
                            tracing::warn!(category = "desktop_restore_failed");
                        }
                    }
                },
                |reset_service| {
                    if reset_service
                        .refresh_now(backend::RefreshReason::Startup)
                        .is_err()
                    {
                        tracing::warn!(category = "reset_credit_initial_refresh_failed");
                    }
                },
            );
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Codex Orbit");

    app.run(|app, event| match event {
        tauri::RunEvent::ExitRequested { .. } => {
            if let Some(registration) = app.try_state::<SessionUnlockRegistration>() {
                registration.unregister();
            }
        }
        tauri::RunEvent::Exit => {
            if let Some(registration) = app.try_state::<SessionUnlockRegistration>() {
                registration.unregister();
            }
            if let Some(reset_service) = app.try_state::<ResetCreditService>() {
                let _ = tauri::async_runtime::block_on(reset_service.shutdown());
            }
            let service = app.state::<BackendService>();
            let _ = tauri::async_runtime::block_on(service.shutdown());
        }
        tauri::RunEvent::Resumed => {
            refresh_managed_services(app, backend::RefreshReason::Resume);
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::commands::refresh_reason_from_frontend;
    use crate::backend::RefreshReason;

    #[test]
    fn frozen_rate_limit_command_name_is_exported_and_registered() {
        let source = include_str!("lib.rs");

        assert!(source.contains("pub fn get_rate_limits("));
        assert!(source.contains("pub fn get_quota_bridge_state("));
        assert!(source.contains("pub fn refresh_rate_limits("));
        assert!(source.contains("get_rate_limits"));
        assert!(source.contains("refresh_rate_limits"));
        assert!(!source.contains(concat!("get_rate_limit_", "state")));
    }

    #[test]
    fn frontend_refresh_reasons_map_to_backend_reasons() {
        assert_eq!(
            refresh_reason_from_frontend("manual"),
            RefreshReason::Manual
        );
        assert_eq!(
            refresh_reason_from_frontend("resetExpired"),
            RefreshReason::ResetExpired
        );
        assert_eq!(refresh_reason_from_frontend("tray"), RefreshReason::Tray);
        assert_eq!(
            refresh_reason_from_frontend("windowShown"),
            RefreshReason::WindowShown
        );
        assert_eq!(
            refresh_reason_from_frontend("resume"),
            RefreshReason::Resume
        );
        assert_eq!(
            refresh_reason_from_frontend("sessionUnlocked"),
            RefreshReason::SessionUnlocked
        );
        assert_eq!(
            refresh_reason_from_frontend("unexpected"),
            RefreshReason::Manual
        );
    }
}
