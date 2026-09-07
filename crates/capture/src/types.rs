use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::CaptureError;

/// Rectangle in display points (not pixels), origin at top-left.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Overlapping area, or `None` when the rectangles do not overlap.
    /// Rectangles that only touch at an edge do not overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let candidate = Rect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        };
        (!candidate.is_empty()).then_some(candidate)
    }

    /// Returns the part of the rectangle that lies inside `bounds`, or an
    /// empty rect at the original origin when the two do not overlap. That
    /// origin is outside `bounds`, so callers must check `is_empty` before
    /// trusting `x` and `y`.
    pub fn clamp_to(&self, bounds: &Rect) -> Rect {
        self.intersect(bounds).unwrap_or(Rect {
            x: self.x,
            y: self.y,
            width: 0.0,
            height: 0.0,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PixelFormat {
    Rgba8,
    Bgra8,
}

/// A single captured image. Always carries its own scale factor so callers
/// never have to guess the pixel-to-point ratio.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub data: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row, including any padding.
    pub stride: usize,
    pub pixel_format: PixelFormat,
    /// Pixels per point, for example 2.0 on a Retina display.
    pub scale_factor: f32,
    /// When the frame was captured. Recording (v2) orders frames by this.
    pub captured_at: SystemTime,
}

impl Frame {
    /// Tightly packed RGBA8 copy with row padding removed.
    ///
    /// Fails when the frame's own fields contradict each other: a stride
    /// narrower than one row of pixels, or a buffer too short to hold
    /// `height` rows. Frames come from platform APIs, so these fields are
    /// validated rather than trusted. An unchecked stride either panics on
    /// the slice index or silently repeats a row.
    pub fn to_rgba8(&self) -> Result<Vec<u8>, CaptureError> {
        let row_bytes = self.width as usize * 4;
        let height = self.height as usize;
        if self.stride < row_bytes {
            return Err(CaptureError::Platform(format!(
                "frame stride {} is narrower than one row of {row_bytes} bytes",
                self.stride
            )));
        }
        let required = match height.checked_sub(1) {
            Some(rows_before_last) => self.stride * rows_before_last + row_bytes,
            None => 0,
        };
        if self.data.len() < required {
            return Err(CaptureError::Platform(format!(
                "frame buffer holds {} bytes, needs {required}",
                self.data.len()
            )));
        }
        let mut out = Vec::with_capacity(row_bytes * height);
        for row in 0..height {
            let start = row * self.stride;
            let row_slice = &self.data[start..start + row_bytes];
            match self.pixel_format {
                PixelFormat::Rgba8 => out.extend_from_slice(row_slice),
                PixelFormat::Bgra8 => {
                    for px in row_slice.chunks_exact(4) {
                        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                    }
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub id: u32,
    /// Bounds in the global point coordinate space.
    pub bounds: Rect,
    pub scale_factor: f32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: u32,
    pub title: Option<String>,
    pub app_name: Option<String>,
    pub bounds: Rect,
    /// Window layer; 0 is the normal application layer.
    pub layer: i32,
    pub is_on_screen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum CaptureTarget {
    Display(u32),
    Window(u32),
    Region(Rect),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn intersect_returns_overlap() {
        let a = rect(0.0, 0.0, 100.0, 100.0);
        let b = rect(50.0, 50.0, 100.0, 100.0);
        assert_eq!(a.intersect(&b), Some(rect(50.0, 50.0, 50.0, 50.0)));
    }

    #[test]
    fn intersect_returns_none_when_disjoint() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(20.0, 20.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), None);
    }

    #[test]
    fn intersect_returns_none_for_edge_touch() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(10.0, 0.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), None);
    }

    #[test]
    fn clamp_to_keeps_rect_inside_bounds() {
        let bounds = rect(0.0, 0.0, 100.0, 100.0);
        let outside = rect(90.0, 90.0, 50.0, 50.0);
        assert_eq!(outside.clamp_to(&bounds), rect(90.0, 90.0, 10.0, 10.0));
    }

    #[test]
    fn clamp_to_returns_empty_rect_when_disjoint() {
        let bounds = rect(0.0, 0.0, 100.0, 100.0);
        let elsewhere = rect(500.0, 500.0, 50.0, 50.0);
        let clamped = elsewhere.clamp_to(&bounds);
        assert!(clamped.is_empty());
        assert_eq!(clamped, rect(500.0, 500.0, 0.0, 0.0));
    }

    #[test]
    fn to_rgba8_drops_stride_padding() {
        // 2x2 image, 4 bytes of row padding per row.
        let frame = Frame {
            data: vec![
                1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, //
                9, 10, 11, 12, 13, 14, 15, 16, 0, 0, 0, 0,
            ],
            width: 2,
            height: 2,
            stride: 12,
            pixel_format: PixelFormat::Rgba8,
            scale_factor: 1.0,
            captured_at: SystemTime::now(),
        };
        assert_eq!(
            frame.to_rgba8().unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn to_rgba8_swaps_bgra_channels_across_padded_rows() {
        // 2x2 BGRA with 4 bytes of row padding: the combination
        // ScreenCaptureKit actually delivers.
        let frame = Frame {
            data: vec![
                10, 20, 30, 40, 50, 60, 70, 80, 0, 0, 0, 0, //
                90, 100, 110, 120, 130, 140, 150, 160, 0, 0, 0, 0,
            ],
            width: 2,
            height: 2,
            stride: 12,
            pixel_format: PixelFormat::Bgra8,
            scale_factor: 2.0,
            captured_at: SystemTime::now(),
        };
        // Each BGRA pixel becomes RGBA, and the padding is dropped.
        assert_eq!(
            frame.to_rgba8().unwrap(),
            vec![30, 20, 10, 40, 70, 60, 50, 80, 110, 100, 90, 120, 150, 140, 130, 160]
        );
    }

    #[test]
    fn to_rgba8_rejects_a_stride_narrower_than_one_row() {
        let frame = Frame {
            data: vec![0; 16],
            width: 2,
            height: 2,
            // A row of 2 pixels needs 8 bytes.
            stride: 4,
            pixel_format: PixelFormat::Rgba8,
            scale_factor: 1.0,
            captured_at: SystemTime::now(),
        };
        assert!(matches!(frame.to_rgba8(), Err(CaptureError::Platform(_))));
    }

    #[test]
    fn to_rgba8_rejects_a_buffer_too_short_for_its_rows() {
        let frame = Frame {
            data: vec![0; 8],
            width: 2,
            height: 2,
            stride: 8,
            pixel_format: PixelFormat::Rgba8,
            scale_factor: 1.0,
            captured_at: SystemTime::now(),
        };
        assert!(matches!(frame.to_rgba8(), Err(CaptureError::Platform(_))));
    }
}
