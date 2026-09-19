//! Bounded box-blur operations for render buffers.

use super::MAX_BLUR_RADIUS;

impl super::RenderBuffer {
    /// Apply box blur (single pass; callers repeat for Gaussian approximation).
    /// The radius is clamped to [`MAX_BLUR_RADIUS`].
    pub fn box_blur(&mut self, radius: u32) {
        self.box_blur_xy(radius, radius);
    }

    /// Apply an axis-specific box blur. libass scales blur horizontally and
    /// vertically from the active layout resolution, which can differ from
    /// PlayRes for anamorphic scripts.
    pub fn box_blur_xy(&mut self, radius_x: u32, radius_y: u32) {
        let radius_x = radius_x.min(MAX_BLUR_RADIUS);
        let radius_y = radius_y.min(MAX_BLUR_RADIUS);
        if (radius_x == 0 && radius_y == 0) || self.width == 0 || self.height == 0 {
            return;
        }
        let rx = radius_x as i32;
        let ry = radius_y as i32;
        let w = self.width as i32;
        let h = self.height as i32;
        let mut out = vec![0u8; self.pixels.len()];

        // Horizontal pass with sliding window
        for y in 0..h {
            // Initialize window sums for x=0
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            let mut sum_a = 0u32;
            let mut count = 0u32;

            for dx in -rx..=rx {
                let sx = dx;
                if sx >= 0 && sx < w {
                    let idx = ((y * w + sx) * 4) as usize;
                    sum_r += self.pixels[idx] as u32;
                    sum_g += self.pixels[idx + 1] as u32;
                    sum_b += self.pixels[idx + 2] as u32;
                    sum_a += self.pixels[idx + 3] as u32;
                    count += 1;
                }
            }

            let idx = ((y * w) * 4) as usize;
            out[idx] = (sum_r / count) as u8;
            out[idx + 1] = (sum_g / count) as u8;
            out[idx + 2] = (sum_b / count) as u8;
            out[idx + 3] = (sum_a / count) as u8;

            // Slide window: for each subsequent x, add right edge, remove left edge
            for x in 1..w {
                // Add new pixel entering window (right side)
                let add_x = x + rx;
                if add_x < w {
                    let idx = ((y * w + add_x) * 4) as usize;
                    sum_r += self.pixels[idx] as u32;
                    sum_g += self.pixels[idx + 1] as u32;
                    sum_b += self.pixels[idx + 2] as u32;
                    sum_a += self.pixels[idx + 3] as u32;
                    count += 1;
                }

                // Remove pixel leaving window (left side)
                let remove_x = x - rx - 1;
                if (0..w).contains(&remove_x) {
                    let idx = ((y * w + remove_x) * 4) as usize;
                    sum_r -= self.pixels[idx] as u32;
                    sum_g -= self.pixels[idx + 1] as u32;
                    sum_b -= self.pixels[idx + 2] as u32;
                    sum_a -= self.pixels[idx + 3] as u32;
                    count -= 1;
                }

                let idx = ((y * w + x) * 4) as usize;
                out[idx] = (sum_r / count) as u8;
                out[idx + 1] = (sum_g / count) as u8;
                out[idx + 2] = (sum_b / count) as u8;
                out[idx + 3] = (sum_a / count) as u8;
            }
        }

        // Vertical pass with sliding window
        self.pixels.fill(0);
        for x in 0..w {
            // Initialize window sums for y=0
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;
            let mut sum_a = 0u32;
            let mut count = 0u32;

            for dy in -ry..=ry {
                let sy = dy;
                if sy >= 0 && sy < h {
                    let idx = ((sy * w + x) * 4) as usize;
                    sum_r += out[idx] as u32;
                    sum_g += out[idx + 1] as u32;
                    sum_b += out[idx + 2] as u32;
                    sum_a += out[idx + 3] as u32;
                    count += 1;
                }
            }

            let idx = (x * 4) as usize;
            self.pixels[idx] = (sum_r / count) as u8;
            self.pixels[idx + 1] = (sum_g / count) as u8;
            self.pixels[idx + 2] = (sum_b / count) as u8;
            self.pixels[idx + 3] = (sum_a / count) as u8;

            // Slide window: for each subsequent y, add bottom edge, remove top edge
            for y in 1..h {
                // Add new pixel entering window (bottom)
                let add_y = y + ry;
                if add_y < h {
                    let idx = ((add_y * w + x) * 4) as usize;
                    sum_r += out[idx] as u32;
                    sum_g += out[idx + 1] as u32;
                    sum_b += out[idx + 2] as u32;
                    sum_a += out[idx + 3] as u32;
                    count += 1;
                }

                // Remove pixel leaving window (top)
                let remove_y = y - ry - 1;
                if (0..h).contains(&remove_y) {
                    let idx = ((remove_y * w + x) * 4) as usize;
                    sum_r -= out[idx] as u32;
                    sum_g -= out[idx + 1] as u32;
                    sum_b -= out[idx + 2] as u32;
                    sum_a -= out[idx + 3] as u32;
                    count -= 1;
                }

                let idx = ((y * w + x) * 4) as usize;
                self.pixels[idx] = (sum_r / count) as u8;
                self.pixels[idx + 1] = (sum_g / count) as u8;
                self.pixels[idx + 2] = (sum_b / count) as u8;
                self.pixels[idx + 3] = (sum_a / count) as u8;
            }
        }
    }
}
