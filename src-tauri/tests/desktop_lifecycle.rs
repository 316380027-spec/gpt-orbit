use codex_orbit_lib::backend::RefreshReason;
use codex_orbit_lib::desktop::lifecycle::{
    coalesced_refresh_reasons, effects_for_event, DesktopLifecycleEvent, LifecycleEffect,
};
use codex_orbit_lib::desktop::session_unlock::{
    route_session_change_message, WM_WTSSESSION_CHANGE_MESSAGE, WTS_SESSION_UNLOCK_CODE,
};
use codex_orbit_lib::{
    request_service_refreshes, restore_then_refresh_optional_service,
    start_reset_credit_for_variant,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[test]
fn close_hides_instead_of_exiting_and_emits_visibility() {
    assert_eq!(
        effects_for_event(DesktopLifecycleEvent::CloseRequested),
        vec![
            LifecycleEffect::HideWindow,
            LifecycleEffect::EmitVisibility(false)
        ]
    );
}

#[test]
fn show_resume_and_unlock_request_coalesced_refreshes() {
    assert_eq!(
        effects_for_event(DesktopLifecycleEvent::WindowShown),
        vec![
            LifecycleEffect::ShowWindow,
            LifecycleEffect::EmitVisibility(true),
            LifecycleEffect::Refresh(RefreshReason::WindowShown)
        ]
    );
    assert_eq!(
        effects_for_event(DesktopLifecycleEvent::Resume),
        vec![LifecycleEffect::Refresh(RefreshReason::Resume)]
    );
    assert_eq!(
        effects_for_event(DesktopLifecycleEvent::SessionUnlocked),
        vec![LifecycleEffect::Refresh(RefreshReason::SessionUnlocked)]
    );

    assert_eq!(
        coalesced_refresh_reasons([
            RefreshReason::WindowShown,
            RefreshReason::WindowShown,
            RefreshReason::Resume,
            RefreshReason::SessionUnlocked,
            RefreshReason::Resume,
        ]),
        vec![
            RefreshReason::WindowShown,
            RefreshReason::Resume,
            RefreshReason::SessionUnlocked,
        ]
    );
}

#[test]
fn quit_waits_for_shutdown_before_exit() {
    assert_eq!(
        effects_for_event(DesktopLifecycleEvent::Quit),
        vec![
            LifecycleEffect::ShutdownBackend,
            LifecycleEffect::ExitProcess
        ]
    );
}

#[test]
fn weekly_lifecycle_wires_reset_credit_refresh_and_shutdown_without_required_state() {
    let source = include_str!("../src/lib.rs");

    assert!(source.contains("AppVariant::Weekly"));
    assert!(source.contains("ResetCreditService::start("));
    assert!(source.contains("try_state::<ResetCreditService>()"));
    assert!(source.contains("backend::RefreshReason::Resume"));
    assert!(source.contains("backend::RefreshReason::SessionUnlocked"));
    assert!(source.contains("install_session_unlock_handler"));
    assert!(source.contains("registration.unregister()"));
    assert!(source.contains("reset_service.shutdown()"));
}

#[test]
fn resumed_dispatches_resume_once_and_never_impersonates_unlock() {
    let source = include_str!("../src/lib.rs");
    let resumed = source
        .split("tauri::RunEvent::Resumed =>")
        .nth(1)
        .expect("resumed run-event arm")
        .split("_ =>")
        .next()
        .expect("end of resumed arm");

    assert_eq!(resumed.matches("RefreshReason::Resume").count(), 1);
    assert!(!resumed.contains("SessionUnlocked"));
}

#[test]
fn native_session_router_requests_exactly_one_refresh_per_unlock() {
    let calls = AtomicUsize::new(0);
    let mut refresh = || {
        calls.fetch_add(1, Ordering::SeqCst);
    };

    assert!(!route_session_change_message(
        WM_WTSSESSION_CHANGE_MESSAGE - 1,
        WTS_SESSION_UNLOCK_CODE,
        &mut refresh,
    ));
    assert!(!route_session_change_message(
        WM_WTSSESSION_CHANGE_MESSAGE,
        WTS_SESSION_UNLOCK_CODE - 1,
        &mut refresh,
    ));
    assert!(route_session_change_message(
        WM_WTSSESSION_CHANGE_MESSAGE,
        WTS_SESSION_UNLOCK_CODE,
        &mut refresh,
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn reset_credit_commands_are_exported_and_registered() {
    let source = include_str!("../src/lib.rs");

    for command in ["get_reset_credits", "refresh_reset_credits"] {
        assert!(source.contains(&format!("pub fn {command}(")));
        assert!(source.contains(&format!("            {command},")));
    }
}

#[test]
fn standard_skips_reset_credit_factory_and_weekly_runs_it_once() {
    let calls = AtomicUsize::new(0);
    let standard = start_reset_credit_for_variant("com.codex-orbit", || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok::<_, ()>(7)
    });
    assert_eq!(standard, None);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let weekly = start_reset_credit_for_variant("com.codex-orbit.weekly", || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok::<_, ()>(7)
    });
    assert_eq!(weekly, Some(7));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let failed = start_reset_credit_for_variant("com.codex-orbit.weekly", || {
        calls.fetch_add(1, Ordering::SeqCst);
        Err::<usize, _>("synthetic-sensitive-detail")
    });
    assert_eq!(failed, None);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn restore_always_completes_before_optional_initial_reset_refresh() {
    let order = Mutex::new(Vec::new());
    let service = 7;

    restore_then_refresh_optional_service(
        Some(&service),
        || order.lock().unwrap().push("restore"),
        |_| order.lock().unwrap().push("refresh"),
    );
    assert_eq!(*order.lock().unwrap(), vec!["restore", "refresh"]);

    order.lock().unwrap().clear();
    restore_then_refresh_optional_service(
        None::<&usize>,
        || order.lock().unwrap().push("restore"),
        |_| order.lock().unwrap().push("refresh"),
    );
    assert_eq!(*order.lock().unwrap(), vec!["restore"]);
}

#[test]
fn refresh_dispatch_reaches_both_services_only_when_weekly_service_exists() {
    let backend_calls = AtomicUsize::new(0);
    let reset_calls = AtomicUsize::new(0);

    for reason in [
        RefreshReason::Tray,
        RefreshReason::WindowShown,
        RefreshReason::Resume,
        RefreshReason::SessionUnlocked,
    ] {
        request_service_refreshes(
            reason,
            |_| {
                backend_calls.fetch_add(1, Ordering::SeqCst);
            },
            Some(|_| {
                reset_calls.fetch_add(1, Ordering::SeqCst);
            }),
        );
    }
    assert_eq!(backend_calls.load(Ordering::SeqCst), 4);
    assert_eq!(reset_calls.load(Ordering::SeqCst), 4);

    request_service_refreshes(
        RefreshReason::Tray,
        |_| {
            backend_calls.fetch_add(1, Ordering::SeqCst);
        },
        None::<fn(RefreshReason)>,
    );
    assert_eq!(backend_calls.load(Ordering::SeqCst), 5);
    assert_eq!(reset_calls.load(Ordering::SeqCst), 4);
}
