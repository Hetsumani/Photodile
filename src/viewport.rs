//! Viewport Pan & Zoom Transform Engine
//!
//! Handles coordinate transformations, aspect-preserving fitting, cursor-centered
//! mouse-wheel zooming, and Photoshop/Lightroom-style scrubby zoom anchored at the initial click.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragMode {
    None,
    Pan {
        start_x: f32,
        start_y: f32,
        start_offset_x: f32,
        start_offset_y: f32,
    },
    ScrubbyZoom {
        anchor_x: f32,
        anchor_y: f32,
        start_x: f32,
        start_scale: f32,
        start_offset_x: f32,
        start_offset_y: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub img_w: f32,
    pub img_h: f32,
    pub vp_w: f32,
    pub vp_h: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub scale: f32,
    pub drag_mode: DragMode,
}

pub const MIN_SCALE: f32 = 0.02;  // 2%
pub const MAX_SCALE: f32 = 64.0;  // 6400%
const SCRUBBY_SENSITIVITY: f32 = 0.007;

impl Default for Viewport {
    fn default() -> Self {
        Self {
            img_w: 0.0,
            img_h: 0.0,
            vp_w: 800.0,
            vp_h: 600.0,
            offset_x: 0.0,
            offset_y: 0.0,
            scale: 1.0,
            drag_mode: DragMode::None,
        }
    }
}

impl Viewport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_image_dimensions(&mut self, w: f32, h: f32) {
        self.img_w = w.max(1.0);
        self.img_h = h.max(1.0);
        self.fit_to_viewport();
    }

    pub fn set_viewport_dimensions(&mut self, w: f32, h: f32) {
        self.vp_w = w.max(10.0);
        self.vp_h = h.max(10.0);
        if self.scale == 1.0 && self.offset_x == 0.0 && self.offset_y == 0.0 && self.img_w > 0.0 {
            self.fit_to_viewport();
        }
    }

    /// Fits the image to the canvas viewport, preserving aspect ratio.
    pub fn fit_to_viewport(&mut self) {
        if self.img_w <= 0.0 || self.img_h <= 0.0 || self.vp_w <= 0.0 || self.vp_h <= 0.0 {
            return;
        }

        // Leave a 12px aesthetic padding around the image if viewport is large enough
        let pad_w = (self.vp_w - 24.0).max(10.0);
        let pad_h = (self.vp_h - 24.0).max(10.0);

        let scale_w = pad_w / self.img_w;
        let scale_h = pad_h / self.img_h;
        let target_scale = scale_w.min(scale_h).min(1.0).clamp(MIN_SCALE, MAX_SCALE);

        self.scale = target_scale;
        self.offset_x = (self.vp_w - self.img_w * self.scale) / 2.0;
        self.offset_y = (self.vp_h - self.img_h * self.scale) / 2.0;
    }

    /// Sets zoom to 100% (1:1 pixel scale), keeping the center of the viewport centered.
    pub fn set_zoom_100(&mut self) {
        let center_x = self.vp_w / 2.0;
        let center_y = self.vp_h / 2.0;
        self.zoom_at_point(center_x, center_y, 1.0);
    }

    /// Zooms keeping the given canvas coordinate (anchor_x, anchor_y) over the exact same
    /// relative pixel position on the image.
    pub fn zoom_at_point(&mut self, anchor_x: f32, anchor_y: f32, target_scale: f32) {
        if self.img_w <= 0.0 || self.img_h <= 0.0 {
            return;
        }

        let new_scale = target_scale.clamp(MIN_SCALE, MAX_SCALE);
        if (new_scale - self.scale).abs() < f32::EPSILON {
            return;
        }

        // Image coordinate under the anchor point before zoom
        let img_px = (anchor_x - self.offset_x) / self.scale;
        let img_py = (anchor_y - self.offset_y) / self.scale;

        // Reposition offset so that img_px, img_py stays at anchor_x, anchor_y
        self.scale = new_scale;
        self.offset_x = anchor_x - img_px * self.scale;
        self.offset_y = anchor_y - img_py * self.scale;
    }

    /// Handles mouse-wheel zooming centered at the live mouse cursor position.
    pub fn wheel_zoom(&mut self, delta_y: f32, cursor_x: f32, cursor_y: f32) {
        if delta_y == 0.0 {
            return;
        }

        let factor = if delta_y > 0.0 {
            1.18
        } else {
            1.0 / 1.18
        };

        self.zoom_at_point(cursor_x, cursor_y, self.scale * factor);
    }

    /// Multiplies or divides zoom by a fixed step (for toolbar +/- buttons), centered on viewport.
    pub fn step_zoom(&mut self, zoom_in: bool) {
        let factor = if zoom_in { 1.25 } else { 1.0 / 1.25 };
        let center_x = self.vp_w / 2.0;
        let center_y = self.vp_h / 2.0;
        self.zoom_at_point(center_x, center_y, self.scale * factor);
    }

    /// Begins a pointer drag interaction.
    /// - Space + Ctrl: Scrubby Zoom (Photoshop/Lightroom style, anchored at initial click)
    /// - Space: Pan tool
    pub fn begin_pointer_drag(&mut self, x: f32, y: f32, space_held: bool, ctrl_held: bool) -> bool {
        if space_held && ctrl_held {
            self.drag_mode = DragMode::ScrubbyZoom {
                anchor_x: x,
                anchor_y: y,
                start_x: x,
                start_scale: self.scale,
                start_offset_x: self.offset_x,
                start_offset_y: self.offset_y,
            };
            true
        } else if space_held {
            self.drag_mode = DragMode::Pan {
                start_x: x,
                start_y: y,
                start_offset_x: self.offset_x,
                start_offset_y: self.offset_y,
            };
            true
        } else {
            self.drag_mode = DragMode::None;
            false
        }
    }

    /// Updates the ongoing pointer drag.
    pub fn update_pointer_drag(&mut self, x: f32, y: f32) -> bool {
        match self.drag_mode {
            DragMode::Pan {
                start_x,
                start_y,
                start_offset_x,
                start_offset_y,
            } => {
                let dx = x - start_x;
                let dy = y - start_y;
                self.offset_x = start_offset_x + dx;
                self.offset_y = start_offset_y + dy;
                true
            }
            DragMode::ScrubbyZoom {
                anchor_x,
                anchor_y,
                start_x,
                start_scale,
                start_offset_x,
                start_offset_y,
            } => {
                let dx = x - start_x;
                // Exponential scrubby zoom: dragging right zooms in, dragging left zooms out
                let new_scale = (start_scale * (dx * SCRUBBY_SENSITIVITY).exp()).clamp(MIN_SCALE, MAX_SCALE);

                // Keep the initial click point (anchor_x, anchor_y) anchored at the exact same screen position
                let img_px = (anchor_x - start_offset_x) / start_scale;
                let img_py = (anchor_y - start_offset_y) / start_scale;

                self.scale = new_scale;
                self.offset_x = anchor_x - img_px * self.scale;
                self.offset_y = anchor_y - img_py * self.scale;
                true
            }
            DragMode::None => false,
        }
    }

    /// Ends the current pointer drag.
    pub fn end_pointer_drag(&mut self) {
        self.drag_mode = DragMode::None;
    }

    pub fn display_width(&self) -> f32 {
        self.img_w * self.scale
    }

    pub fn display_height(&self) -> f32 {
        self.img_h * self.scale
    }

    pub fn zoom_percentage(&self) -> u32 {
        (self.scale * 100.0).round() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fit_to_viewport_centers_image() {
        let mut vp = Viewport::new();
        vp.vp_w = 1000.0;
        vp.vp_h = 800.0;
        vp.set_image_dimensions(2000.0, 1000.0);

        // Aspect ratio is 2:1.
        // Viewport padding is 24px, so available width is 976, available height is 776.
        // Scale is limited by width: 976 / 2000 = 0.488.
        assert!(vp.scale > 0.0 && vp.scale < 1.0);
        // Should be horizontally and vertically centered:
        let expected_x = (vp.vp_w - vp.img_w * vp.scale) / 2.0;
        let expected_y = (vp.vp_h - vp.img_h * vp.scale) / 2.0;
        assert!((vp.offset_x - expected_x).abs() < 1e-3);
        assert!((vp.offset_y - expected_y).abs() < 1e-3);
    }

    #[test]
    fn test_pan_translates_offset_by_delta() {
        let mut vp = Viewport::new();
        vp.set_image_dimensions(1000.0, 1000.0);
        let initial_x = vp.offset_x;
        let initial_y = vp.offset_y;

        assert!(vp.begin_pointer_drag(100.0, 100.0, true, false));
        assert!(vp.update_pointer_drag(140.0, 160.0));
        assert_eq!(vp.offset_x, initial_x + 40.0);
        assert_eq!(vp.offset_y, initial_y + 60.0);

        vp.end_pointer_drag();
        assert_eq!(vp.drag_mode, DragMode::None);
    }

    #[test]
    fn test_scrubby_zoom_in_and_out_preserves_anchor() {
        let mut vp = Viewport::new();
        vp.vp_w = 800.0;
        vp.vp_h = 600.0;
        vp.set_image_dimensions(800.0, 600.0);

        let anchor_x = 350.0;
        let anchor_y = 220.0;
        let initial_scale = vp.scale;

        // Image point under anchor before zoom
        let img_x = (anchor_x - vp.offset_x) / vp.scale;
        let img_y = (anchor_y - vp.offset_y) / vp.scale;

        // Start scrubby zoom (Space + Ctrl + Left Click)
        assert!(vp.begin_pointer_drag(anchor_x, anchor_y, true, true));

        // Drag right 100px -> Zoom in
        assert!(vp.update_pointer_drag(anchor_x + 100.0, anchor_y));
        assert!(vp.scale > initial_scale, "Dragging right must zoom in");

        // The image point under anchor must STILL be at (anchor_x, anchor_y)
        let new_canvas_x = vp.offset_x + img_x * vp.scale;
        let new_canvas_y = vp.offset_y + img_y * vp.scale;
        assert!((new_canvas_x - anchor_x).abs() < 1e-3);
        assert!((new_canvas_y - anchor_y).abs() < 1e-3);

        // Drag left 100px -> Zoom out
        assert!(vp.update_pointer_drag(anchor_x - 100.0, anchor_y));
        assert!(vp.scale < initial_scale, "Dragging left must zoom out");

        let zoom_out_canvas_x = vp.offset_x + img_x * vp.scale;
        let zoom_out_canvas_y = vp.offset_y + img_y * vp.scale;
        assert!((zoom_out_canvas_x - anchor_x).abs() < 1e-3);
        assert!((zoom_out_canvas_y - anchor_y).abs() < 1e-3);
    }

    #[test]
    fn test_wheel_zoom_preserves_cursor_position() {
        let mut vp = Viewport::new();
        vp.vp_w = 1200.0;
        vp.vp_h = 900.0;
        vp.set_image_dimensions(1920.0, 1080.0);

        let cursor_x = 450.0;
        let cursor_y = 320.0;

        let img_px = (cursor_x - vp.offset_x) / vp.scale;
        let img_py = (cursor_y - vp.offset_y) / vp.scale;

        // Zoom in via wheel
        vp.wheel_zoom(1.0, cursor_x, cursor_y);

        let canvas_x_after = vp.offset_x + img_px * vp.scale;
        let canvas_y_after = vp.offset_y + img_py * vp.scale;
        assert!((canvas_x_after - cursor_x).abs() < 1e-3);
        assert!((canvas_y_after - cursor_y).abs() < 1e-3);
    }
}

