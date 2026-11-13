use gpui::*;

/// Twitter / X Obsidian Color Palette
pub fn color_bg_base() -> Rgba {
    rgb(0x000000)
}
pub fn color_surface() -> Rgba {
    rgb(0x16181c)
}
pub fn color_surface_hover() -> Rgba {
    rgb(0x1e2024)
}
pub fn color_border() -> Rgba {
    rgb(0x2f3336)
}
pub fn color_border_subtle() -> Rgba {
    rgb(0x202327)
}
pub fn color_text_primary() -> Rgba {
    rgb(0xe7e9ea)
}
pub fn color_text_muted() -> Rgba {
    rgb(0x71767b)
}
pub fn color_accent_blue() -> Rgba {
    rgb(0x1d9bf0)
}
pub fn color_success_green() -> Rgba {
    rgb(0x00ba7c)
}
pub fn color_danger_red() -> Rgba {
    rgb(0xf4212e)
}
pub fn color_warning_amber() -> Rgba {
    rgb(0xffd400)
}
pub fn color_review_purple() -> Rgba {
    rgb(0xa855f7)
}
pub fn color_review_cyan() -> Rgba {
    rgb(0x06b6d4)
}

/// Swiss Grid System Dimensions
pub const GRID_COLUMNS: usize = 12;
pub const SIDEBAR_COLS: usize = 2; // 2 / 12 = 16.67%
pub const FEED_COLS: usize = 6; // 6 / 12 = 50.00%
pub const INSPECTOR_COLS: usize = 4; // 4 / 12 = 33.33%

/// Single sans-serif font family enforced across the entire interface.
pub fn font_family_sans() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Segoe UI"
    }
    #[cfg(target_os = "macos")]
    {
        "Helvetica Neue"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "Arial"
    }
}

pub fn spacing_unit() -> Pixels {
    px(4.0)
}
pub fn padding_xs() -> Pixels {
    px(4.0)
}
pub fn padding_sm() -> Pixels {
    px(8.0)
}
pub fn padding_md() -> Pixels {
    px(12.0)
}
pub fn padding_lg() -> Pixels {
    px(16.0)
}
pub fn padding_xl() -> Pixels {
    px(24.0)
}

pub fn gap_xs() -> Pixels {
    px(4.0)
}
pub fn gap_sm() -> Pixels {
    px(8.0)
}
pub fn gap_md() -> Pixels {
    px(12.0)
}
pub fn gap_lg() -> Pixels {
    px(16.0)
}
pub fn gap_xl() -> Pixels {
    px(24.0)
}

pub fn radius_none() -> Pixels {
    px(0.0)
}
pub fn radius_xs() -> Pixels {
    px(2.0)
}
pub fn radius_sm() -> Pixels {
    px(4.0)
}
pub fn radius_md() -> Pixels {
    px(6.0)
}
pub fn radius_lg() -> Pixels {
    px(8.0)
}
pub fn radius_xl() -> Pixels {
    px(12.0)
}

pub fn font_size_caption() -> Pixels {
    px(11.0)
}
pub fn font_size_body() -> Pixels {
    px(13.0)
}
pub fn font_size_title() -> Pixels {
    px(15.0)
}
pub fn font_size_header() -> Pixels {
    px(18.0)
}

/// Status pill color resolver
pub fn status_color(status: &str) -> Rgba {
    match status.to_uppercase().as_str() {
        "ONLINE" | "DONE" => color_success_green(),
        "RUNNING" | "THINKING" => color_warning_amber(),
        "ERROR" | "FAILED" => color_danger_red(),
        _ => color_text_muted(),
    }
}
