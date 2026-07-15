use super::app_variant::AppVariant;
use tauri::{Runtime, WebviewWindow};

const NATIVE_CHROME_MASK: u32 = 0x00CF_0000;
const WS_POPUP_STYLE: u32 = 0x8000_0000;

fn compact_popup_style(style: isize) -> isize {
    (((style as u32 & !NATIVE_CHROME_MASK) | WS_POPUP_STYLE) as i32) as isize
}

pub fn configure_compact_window<R: Runtime>(
    window: &WebviewWindow<R>,
    variant: AppVariant,
) -> Result<(), &'static str> {
    if variant != AppVariant::Weekly {
        return Ok(());
    }

    window.set_title("").map_err(|_| "compact_window_title")?;

    #[cfg(not(windows))]
    {
        let _ = window;
        Ok(())
    }

    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{GetLastError, SetLastError, ERROR_SUCCESS};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        };

        let hwnd = window.hwnd().map_err(|_| "compact_window_handle")?.0
            as windows_sys::Win32::Foundation::HWND;
        let current = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let compact = compact_popup_style(current);
        SetLastError(ERROR_SUCCESS);
        if SetWindowLongPtrW(hwnd, GWL_STYLE, compact) == 0 && GetLastError() != ERROR_SUCCESS {
            return Err("compact_window_style");
        }
        if SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        ) == 0
        {
            return Err("compact_window_frame");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::compact_popup_style;

    #[test]
    fn removes_native_chrome_that_enforces_the_windows_minimum_width() {
        let tauri_style = 0x14CB_0000_u32 as i32 as isize;

        let compact = compact_popup_style(tauri_style) as u32;

        assert_eq!(compact, 0x9400_0000);
        assert_eq!(compact & 0x00CF_0000, 0);
        assert_ne!(compact & 0x8000_0000, 0);
        assert_ne!(compact & 0x1000_0000, 0);
        assert_ne!(compact & 0x0400_0000, 0);
    }
}
