pub mod app_variant;
pub mod compact_window;
pub mod lifecycle;
pub mod preferences;
pub mod session_unlock;
pub mod tray;

pub use preferences::{
    load_desktop_preferences, persist_always_on_top, restore_widget_window,
    save_current_widget_placement, save_widget_placement, set_always_on_top, DesktopPreferences,
};
pub use tray::install_tray;
