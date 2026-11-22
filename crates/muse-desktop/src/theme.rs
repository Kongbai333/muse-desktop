//! Shared visual tokens and the single source of truth for native-view geometry.

pub const TITLEBAR: f32 = 40.0;
pub const RAIL: f32 = 0.0;
pub const SIDEBAR: f32 = 380.0;
pub const INSPECTOR: f32 = 264.0;
/// Proxy pool side sheet. The page shrinks beside it instead of being hidden.
pub const DRAWER: f32 = 360.0;
// Tabs share the title bar row; kept as a named term of the geometry.
pub const TABS: f32 = 0.0;
pub const TOOLBAR: f32 = 44.0;
pub const STATUS: f32 = 24.0;
pub const MIN_BROWSER_WIDTH: f32 = 360.0;

use muse_core::ThemeMode;
use std::sync::atomic::{AtomicBool, Ordering};

// Rendering, hover callbacks and separately rendered input/tooltip entities all
// resolve the same palette. Media pixels and viewer overlays remain untouched.
static DARK: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: u32,
    pub panel: u32,
    pub surface: u32,
    pub line: u32,
    pub text: u32,
    pub secondary: u32,
    pub blue: u32,
    pub primary: u32,
    pub primary_hover: u32,
    pub on_primary: u32,
    pub fill: u32,
    pub fill_hover: u32,
    pub track: u32,
    pub muted: u32,
    pub danger: u32,
}

pub const fn palette_for(mode: ThemeMode) -> Palette {
    match mode {
        ThemeMode::Light => Palette {
            canvas: 0xFFFFFF,
            panel: 0xFFFFFF,
            surface: 0xF7F7F8,
            line: 0xECECEF,
            text: 0x18181B,
            secondary: 0x71717A,
            blue: 0x2F6FEB,
            primary: 0x18181B,
            primary_hover: 0x2E2E33,
            on_primary: 0xFFFFFF,
            fill: 0xF2F2F4,
            fill_hover: 0xEBEBEE,
            track: 0xE6E6EA,
            muted: 0xA1A1AA,
            danger: 0xD92D20,
        },
        ThemeMode::Dark => Palette {
            canvas: 0x18181B,
            panel: 0x222226,
            surface: 0x1D1D21,
            line: 0x36363E,
            text: 0xECECF1,
            secondary: 0xAAAAB5,
            blue: 0x82AAFF,
            primary: 0xE5E5EB,
            primary_hover: 0xFFFFFF,
            on_primary: 0x18181B,
            fill: 0x2B2B31,
            fill_hover: 0x35353D,
            track: 0x41414B,
            muted: 0x92929F,
            danger: 0xFF8A80,
        },
    }
}

pub fn set_mode(mode: ThemeMode) {
    DARK.store(mode == ThemeMode::Dark, Ordering::Relaxed);
}

pub fn colors() -> Palette {
    palette_for(if DARK.load(Ordering::Relaxed) {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    })
}

/// Preserve a component's light-mode refinement while supplying its dark role.
pub fn tone(light: u32, dark: u32) -> u32 {
    if DARK.load(Ordering::Relaxed) {
        dark
    } else {
        light
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ChromeLayout {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub inspector_width: f32,
}

impl ChromeLayout {
    #[cfg(test)]
    pub fn new(width: f32, height: f32, sidebar_extent: f32, inspector: bool) -> Self {
        Self::with_panel_extents(
            width,
            height,
            sidebar_extent,
            if inspector { INSPECTOR } else { 0. },
        )
    }

    pub fn with_panel_extents(
        width: f32,
        height: f32,
        sidebar_extent: f32,
        inspector_extent: f32,
    ) -> Self {
        let max_sidebar = (width - RAIL - inspector_extent - MIN_BROWSER_WIDTH)
            .max(crate::sidebar_state::COMPACT);
        let left = RAIL + sidebar_extent.min(max_sidebar);
        let top = TITLEBAR + TABS + TOOLBAR;
        Self {
            left,
            top,
            width: (width - left - inspector_extent).max(1.0),
            height: (height - top - STATUS).max(1.0),
            inspector_width: inspector_extent,
        }
    }

    pub fn sidebar_extent(self) -> f32 {
        self.left - RAIL
    }

    pub fn at_motion_frame(
        width: f32,
        height: f32,
        sidebar: &crate::motion::MotionValue,
        inspector: &crate::motion::MotionValue,
        now: std::time::Instant,
    ) -> Self {
        Self::with_panel_extents(
            width,
            height,
            sidebar.value_at(now),
            inspector.value_at(now),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn both_themes_keep_body_secondary_and_primary_button_text_readable() {
        fn luminance(color: u32) -> f64 {
            let channel = |shift: u32| {
                let value = ((color >> shift) & 255u32) as f64 / 255.;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
        }
        for mode in [muse_core::ThemeMode::Light, muse_core::ThemeMode::Dark] {
            let palette = palette_for(mode);
            for (foreground, background) in [
                (palette.text, palette.canvas),
                (palette.text, palette.panel),
                (palette.secondary, palette.panel),
                (palette.on_primary, palette.primary),
            ] {
                let a = luminance(foreground);
                let b = luminance(background);
                let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(contrast >= 4.5, "{mode:?}: contrast was {contrast}");
            }
        }
    }

    #[test]
    fn night_mode_uses_dark_surfaces_without_recoloring_media() {
        let dark = palette_for(muse_core::ThemeMode::Dark);
        assert!(dark.canvas < 0x303030);
        assert!(dark.panel < 0x303030);
        assert!(dark.surface < 0x303030);
        assert!(dark.text > 0xCCCCCC);
    }

    #[test]
    fn browser_geometry_tracks_sidebar_and_inspector_midway_through_transition() {
        let start = Instant::now();
        let mut sidebar = crate::motion::MotionValue::new(
            SIDEBAR + crate::sidebar_state::DIVIDER,
            crate::sidebar_state::COMPACT,
            start,
            Some(Duration::from_millis(240)),
        );
        let inspector =
            crate::motion::MotionValue::new(0., INSPECTOR, start, Some(Duration::from_millis(240)));
        let midpoint = start + Duration::from_millis(120);

        let layout = ChromeLayout::with_panel_extents(
            1360.,
            880.,
            sidebar.value_at(midpoint),
            inspector.value_at(midpoint),
        );
        assert_eq!(layout.left, 111.25);
        assert_eq!(layout.width, 1017.75);

        sidebar.retarget(
            SIDEBAR + crate::sidebar_state::DIVIDER,
            midpoint,
            Some(Duration::from_millis(240)),
        );
        assert_eq!(sidebar.value_at(midpoint), 111.25);
        assert_eq!(
            sidebar.value_at(midpoint + Duration::from_millis(240)),
            SIDEBAR + crate::sidebar_state::DIVIDER
        );
    }

    #[test]
    fn browser_geometry_reaches_the_latest_panel_targets_after_a_rapid_retarget() {
        let start = Instant::now();
        let midpoint = start + Duration::from_millis(120);
        let mut sidebar = crate::motion::MotionValue::new(
            SIDEBAR + crate::sidebar_state::DIVIDER,
            crate::sidebar_state::COMPACT,
            start,
            Some(Duration::from_millis(240)),
        );
        let mut inspector =
            crate::motion::MotionValue::new(0., INSPECTOR, start, Some(Duration::from_millis(240)));

        sidebar.retarget(
            SIDEBAR + crate::sidebar_state::DIVIDER,
            midpoint,
            Some(Duration::from_millis(240)),
        );
        inspector.retarget(0., midpoint, Some(Duration::from_millis(240)));

        let final_frame = ChromeLayout::at_motion_frame(
            1360.,
            880.,
            &sidebar,
            &inspector,
            midpoint + Duration::from_millis(240),
        );
        assert_eq!(final_frame.left, SIDEBAR + crate::sidebar_state::DIVIDER);
        assert_eq!(final_frame.width, 974.);
        assert_eq!(final_frame.top, TITLEBAR + TABS + TOOLBAR);
        assert_eq!(
            final_frame.height,
            880. - TITLEBAR - TABS - TOOLBAR - STATUS
        );
    }

    #[test]
    fn browser_geometry_uses_interpolated_extents_for_each_panel_at_the_same_frame() {
        let start = Instant::now();
        let sidebar = crate::motion::MotionValue::new(
            SIDEBAR + crate::sidebar_state::DIVIDER,
            crate::sidebar_state::COMPACT,
            start,
            Some(Duration::from_millis(240)),
        );
        let inspector =
            crate::motion::MotionValue::new(0., INSPECTOR, start, Some(Duration::from_millis(240)));
        let frame = ChromeLayout::at_motion_frame(
            1360.,
            880.,
            &sidebar,
            &inspector,
            start + Duration::from_millis(120),
        );

        assert_eq!(frame.left, 111.25);
        assert_eq!(frame.width, 1017.75);
        assert_eq!(frame.inspector_width, 231.);
    }

    #[test]
    fn browser_does_not_overlap_navigation_or_side_panels() {
        for (sidebar, inspector) in [(false, false), (true, false), (false, true), (true, true)] {
            let rect = ChromeLayout::new(
                1360.0,
                880.0,
                if sidebar { SIDEBAR } else { 72. },
                inspector,
            );
            let left = RAIL + if sidebar { SIDEBAR } else { 0.0 };
            let right = if inspector { INSPECTOR } else { 0.0 };
            assert!(rect.left >= left);
            assert!(rect.top >= TITLEBAR + TABS + TOOLBAR);
            assert_eq!(rect.left + rect.width + right, 1360.0);
            assert_eq!(rect.top + rect.height + STATUS, 880.0);
            assert!(rect.width > 0.0 && rect.height > 0.0);
        }
    }

    #[test]
    fn final_browser_rectangle_is_correct_for_each_sidebar_and_inspector_state() {
        let expected = [
            (72., 0., 72., 1288.),
            (458., 0., 458., 902.),
            (72., 264., 72., 1024.),
            (458., 264., 458., 638.),
        ];
        for (sidebar, inspector, left, width) in expected {
            let layout = ChromeLayout::with_panel_extents(1360., 880., sidebar, inspector);
            assert_eq!(layout.left, left);
            assert_eq!(layout.width, width);
            assert_eq!(layout.left + layout.width + inspector, 1360.);
        }
    }

    #[test]
    fn resizing_with_the_inspector_open_reserves_browser_space_without_losing_sidebar_width() {
        let now = Instant::now();
        let sidebar = crate::motion::MotionValue::new(566., 566., now, None);
        let inspector = crate::motion::MotionValue::new(264., 264., now, None);

        let narrow = ChromeLayout::at_motion_frame(1030., 680., &sidebar, &inspector, now);
        let wide = ChromeLayout::at_motion_frame(1360., 880., &sidebar, &inspector, now);

        assert_eq!((narrow.left, narrow.width), (406., 360.));
        assert_eq!((wide.left, wide.width), (566., 530.));
    }

    #[test]
    fn collapsing_sidebar_reclaims_space_without_moving_browser_vertically() {
        let expanded = ChromeLayout::new(1360.0, 880.0, 458., false);
        let collapsed = ChromeLayout::new(1360.0, 880.0, 72., false);
        assert!(collapsed.left < expanded.left);
        assert!(collapsed.width > expanded.width);
        assert_eq!(collapsed.top, expanded.top);
    }

    #[test]
    fn settings_inspector_uses_its_own_region_without_covering_the_page() {
        let normal = ChromeLayout::new(1030.0, 680.0, 386., false);
        let settings = ChromeLayout::new(1030.0, 680.0, 386., true);
        assert_eq!(normal.left, settings.left);
        assert!(settings.width < normal.width);
        assert!(settings.width >= 360.0);
    }
}
