/// RGBA render buffer for subtitle compositing
pub struct RenderBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RenderBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height * 4) as usize],
        }
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels.resize((width * height * 4) as usize, 0);
        self.clear();
    }

    /// Get pixel at (x, y) as [R, G, B, A]
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let idx = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ]
    }

    /// Set pixel at (x, y) with RGBA values
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        self.pixels[idx] = r;
        self.pixels[idx + 1] = g;
        self.pixels[idx + 2] = b;
        self.pixels[idx + 3] = a;
    }

    /// Blend a pixel with alpha compositing (source-over)
    #[inline]
    #[allow(clippy::manual_checked_ops)]
    pub fn blend_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if x >= self.width || y >= self.height || a == 0 {
            return;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        let src_a = a as u32;
        let dst_a = self.pixels[idx + 3] as u32;
        let out_a = src_a + dst_a * (255 - src_a) / 255;

        if out_a > 0 {
            let inv_src = 255 - src_a;
            self.pixels[idx] = ((r as u32 * src_a
                + self.pixels[idx] as u32 * dst_a * inv_src / 255)
                / out_a) as u8;
            self.pixels[idx + 1] = ((g as u32 * src_a
                + self.pixels[idx + 1] as u32 * dst_a * inv_src / 255)
                / out_a) as u8;
            self.pixels[idx + 2] = ((b as u32 * src_a
                + self.pixels[idx + 2] as u32 * dst_a * inv_src / 255)
                / out_a) as u8;
            self.pixels[idx + 3] = ((out_a * 255 + 128) / 255) as u8;
        }
    }

    /// Blend premultiplied alpha pixel
    #[inline]
    pub fn blend_pixel_premul(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if x >= self.width || y >= self.height || a == 0 {
            return;
        }
        let idx = ((y * self.width + x) * 4) as usize;
        let inv = 255 - a as u32;

        self.pixels[idx] = (r as u32 + self.pixels[idx] as u32 * inv / 255) as u8;
        self.pixels[idx + 1] = (g as u32 + self.pixels[idx + 1] as u32 * inv / 255) as u8;
        self.pixels[idx + 2] = (b as u32 + self.pixels[idx + 2] as u32 * inv / 255) as u8;
        self.pixels[idx + 3] = (a as u32 + self.pixels[idx + 3] as u32 * inv / 255) as u8;
    }

    /// Fill a rectangle with color
    #[allow(clippy::too_many_arguments)]
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8, a: u8) {
        if a == 0 {
            return;
        }
        let x0 = x.max(0) as u32;
        let y0 = y.max(0) as u32;
        let x1 = (x + w).min(self.width as i32) as u32;
        let y1 = (y + h).min(self.height as i32) as u32;

        for py in y0..y1 {
            for px in x0..x1 {
                self.blend_pixel(px, py, r, g, b, a);
            }
        }
    }

    /// Get raw pixel slice for ImageData creation
    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    /// Apply box blur (3-pass for Gaussian approximation) with sliding window
    pub fn box_blur(&mut self, radius: u32) {
        if radius == 0 {
            return;
        }
        let r = radius as i32;
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

            for dx in -r..=r {
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
                let add_x = x + r;
                if add_x < w {
                    let idx = ((y * w + add_x) * 4) as usize;
                    sum_r += self.pixels[idx] as u32;
                    sum_g += self.pixels[idx + 1] as u32;
                    sum_b += self.pixels[idx + 2] as u32;
                    sum_a += self.pixels[idx + 3] as u32;
                    count += 1;
                }

                // Remove pixel leaving window (left side)
                let remove_x = x - r - 1;
                if remove_x >= 0 && remove_x < w {
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

            for dy in -r..=r {
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
                let add_y = y + r;
                if add_y < h {
                    let idx = ((add_y * w + x) * 4) as usize;
                    sum_r += out[idx] as u32;
                    sum_g += out[idx + 1] as u32;
                    sum_b += out[idx + 2] as u32;
                    sum_a += out[idx + 3] as u32;
                    count += 1;
                }

                // Remove pixel leaving window (top)
                let remove_y = y - r - 1;
                if remove_y >= 0 && remove_y < h {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buf = RenderBuffer::new(100, 100);
        assert_eq!(buf.width, 100);
        assert_eq!(buf.height, 100);
        assert_eq!(buf.pixels.len(), 40000);
    }

    #[test]
    fn test_set_get_pixel() {
        let mut buf = RenderBuffer::new(10, 10);
        buf.set_pixel(5, 5, 255, 128, 64, 200);
        let px = buf.get_pixel(5, 5);
        assert_eq!(px, [255, 128, 64, 200]);
    }

    #[test]
    fn test_blend_pixel() {
        let mut buf = RenderBuffer::new(10, 10);
        buf.set_pixel(5, 5, 0, 0, 0, 128);
        buf.blend_pixel(5, 5, 255, 255, 255, 128);
        let px = buf.get_pixel(5, 5);
        assert!(px[3] > 128);
    }
}
