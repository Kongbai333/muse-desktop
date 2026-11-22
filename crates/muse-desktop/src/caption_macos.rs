//! Native movement is delegated to AppKit rather than Win32 non-client messages.
pub struct CaptionBridge;
impl CaptionBridge {
    pub fn install(window: &gpui::Window) -> Result<Self, String> {
        crate::windows_host::hide_standard_controls(window);
        Ok(Self)
    }
}
pub fn set_drag_start(_: f32) {}
pub fn set_drag_end(_: f32) {}
pub fn set_tabs_start(_: f32) {}
pub fn set_overlay(_: bool) {}
