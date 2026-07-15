use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Runtime, WebviewWindow};

pub const WM_WTSSESSION_CHANGE_MESSAGE: u32 = 0x02B1;
pub const WTS_SESSION_UNLOCK_CODE: usize = 0x8;

pub fn route_session_change_message(
    message: u32,
    event_code: usize,
    on_unlock: &mut impl FnMut(),
) -> bool {
    if message != WM_WTSSESSION_CHANGE_MESSAGE || event_code != WTS_SESSION_UNLOCK_CODE {
        return false;
    }
    on_unlock();
    true
}

pub struct SessionUnlockRegistration {
    active: AtomicBool,
    #[cfg(windows)]
    hwnd: isize,
    #[cfg(windows)]
    callback: usize,
}

impl SessionUnlockRegistration {
    pub fn unregister(&self) {
        if !self.active.swap(false, Ordering::AcqRel) {
            return;
        }
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::RemoteDesktop::WTSUnRegisterSessionNotification;
            use windows_sys::Win32::UI::Shell::RemoveWindowSubclass;

            let hwnd = self.hwnd as windows_sys::Win32::Foundation::HWND;
            let _ = WTSUnRegisterSessionNotification(hwnd);
            let _ = RemoveWindowSubclass(hwnd, Some(session_unlock_subclass), SUBCLASS_ID);
            drop(Box::from_raw(self.callback as *mut UnlockCallback));
        }
    }
}

impl Drop for SessionUnlockRegistration {
    fn drop(&mut self) {
        self.unregister();
    }
}

#[cfg(windows)]
const SUBCLASS_ID: usize = 0x4750_544f;

#[cfg(windows)]
struct UnlockCallback {
    on_unlock: Box<dyn Fn() + Send + Sync>,
}

#[cfg(windows)]
unsafe extern "system" fn session_unlock_subclass(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _subclass_id: usize,
    callback: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;

    let mut on_unlock = || {
        let callback = &*(callback as *const UnlockCallback);
        (callback.on_unlock)();
    };
    let _ = route_session_change_message(message, wparam, &mut on_unlock);
    DefSubclassProc(hwnd, message, wparam, lparam)
}

pub fn install_session_unlock_handler<R: Runtime>(
    window: &WebviewWindow<R>,
    on_unlock: impl Fn() + Send + Sync + 'static,
) -> Result<Option<SessionUnlockRegistration>, &'static str> {
    #[cfg(not(windows))]
    {
        let _ = (window, on_unlock);
        Ok(None)
    }

    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::RemoteDesktop::{
            WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
        };
        use windows_sys::Win32::UI::Shell::{RemoveWindowSubclass, SetWindowSubclass};

        let hwnd = window.hwnd().map_err(|_| "session_unlock_window_handle")?.0
            as windows_sys::Win32::Foundation::HWND;
        let callback = Box::into_raw(Box::new(UnlockCallback {
            on_unlock: Box::new(on_unlock),
        }));

        if SetWindowSubclass(
            hwnd,
            Some(session_unlock_subclass),
            SUBCLASS_ID,
            callback as usize,
        ) == 0
        {
            drop(Box::from_raw(callback));
            return Err("session_unlock_subclass_registration");
        }
        if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) == 0 {
            let _ = RemoveWindowSubclass(hwnd, Some(session_unlock_subclass), SUBCLASS_ID);
            drop(Box::from_raw(callback));
            return Err("session_unlock_notification_registration");
        }

        Ok(Some(SessionUnlockRegistration {
            active: AtomicBool::new(true),
            hwnd: hwnd as isize,
            callback: callback as usize,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{WM_WTSSESSION_CHANGE_MESSAGE, WTS_SESSION_UNLOCK_CODE};

    #[cfg(windows)]
    #[test]
    fn portable_message_constants_match_windows_bindings() {
        assert_eq!(
            WM_WTSSESSION_CHANGE_MESSAGE,
            windows_sys::Win32::UI::WindowsAndMessaging::WM_WTSSESSION_CHANGE
        );
        assert_eq!(
            WTS_SESSION_UNLOCK_CODE,
            windows_sys::Win32::UI::WindowsAndMessaging::WTS_SESSION_UNLOCK as usize
        );
    }
}
