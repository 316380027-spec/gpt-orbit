use crate::backend::RefreshReason;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopLifecycleEvent {
    CloseRequested,
    WindowShown,
    Resume,
    SessionUnlocked,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEffect {
    HideWindow,
    ShowWindow,
    EmitVisibility(bool),
    Refresh(RefreshReason),
    ShutdownBackend,
    ExitProcess,
}

pub fn effects_for_event(event: DesktopLifecycleEvent) -> Vec<LifecycleEffect> {
    match event {
        DesktopLifecycleEvent::CloseRequested => {
            vec![
                LifecycleEffect::HideWindow,
                LifecycleEffect::EmitVisibility(false),
            ]
        }
        DesktopLifecycleEvent::WindowShown => vec![
            LifecycleEffect::ShowWindow,
            LifecycleEffect::EmitVisibility(true),
            LifecycleEffect::Refresh(RefreshReason::WindowShown),
        ],
        DesktopLifecycleEvent::Resume => vec![LifecycleEffect::Refresh(RefreshReason::Resume)],
        DesktopLifecycleEvent::SessionUnlocked => {
            vec![LifecycleEffect::Refresh(RefreshReason::SessionUnlocked)]
        }
        DesktopLifecycleEvent::Quit => {
            vec![
                LifecycleEffect::ShutdownBackend,
                LifecycleEffect::ExitProcess,
            ]
        }
    }
}

pub fn coalesced_refresh_reasons(
    reasons: impl IntoIterator<Item = RefreshReason>,
) -> Vec<RefreshReason> {
    let mut coalesced = Vec::new();
    for reason in reasons {
        if !coalesced.contains(&reason) {
            coalesced.push(reason);
        }
    }
    coalesced
}
