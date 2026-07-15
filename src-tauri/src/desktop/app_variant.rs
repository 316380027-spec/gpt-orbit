#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppVariant {
    Standard,
    Weekly,
}

impl AppVariant {
    pub fn from_identifier(identifier: &str) -> Self {
        if identifier == "com.codex-orbit.weekly" {
            Self::Weekly
        } else {
            Self::Standard
        }
    }

    pub fn widget_canvas(self) -> WidgetCanvas {
        match self {
            Self::Standard => STANDARD_CANVAS,
            Self::Weekly => WEEKLY_CANVAS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetCanvas {
    pub collapsed_width: f64,
    pub collapsed_height: f64,
    pub expanded_width: f64,
    pub expanded_height: f64,
}

pub const STANDARD_CANVAS: WidgetCanvas = WidgetCanvas {
    collapsed_width: 172.0,
    collapsed_height: 172.0,
    expanded_width: 269.0,
    expanded_height: 136.0,
};

pub const WEEKLY_CANVAS: WidgetCanvas = WidgetCanvas {
    collapsed_width: 104.0,
    collapsed_height: 86.0,
    expanded_width: 153.0,
    expanded_height: 68.0,
};
