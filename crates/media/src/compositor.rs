//! Compositor CPU de fotogramas RGBA. Lo usan por igual el visor y la exportación.

use std::sync::Arc;
use tv2_domain::timeline::{FitMode, Transform};

#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA 8 bits, `width * height * 4`.
    pub rgba: Arc<Vec<u8>>,
}

impl Frame {
    pub fn black(width: u32, height: u32) -> Frame {
        let mut v = vec![0u8; (width * height * 4) as usize];
        for px in v.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        Frame { width, height, rgba: Arc::new(v) }
    }

    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Frame {
        debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
        Frame { width, height, rgba: Arc::new(rgba) }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Escalado por vecino más cercano.
    pub fn resized(&self, w: u32, h: u32) -> Frame {
        if w == self.width && h == self.height {
            return self.clone();
        }
        let mut out = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            let sy = (y as u64 * self.height as u64 / h as u64) as u32;
            for x in 0..w {
                let sx = (x as u64 * self.width as u64 / w as u64) as u32;
                let si = ((sy * self.width + sx) * 4) as usize;
                let di = ((y * w + x) * 4) as usize;
                out[di..di + 4].copy_from_slice(&self.rgba[si..si + 4]);
            }
        }
        Frame { width: w, height: h, rgba: Arc::new(out) }
    }
}

/// Rectángulo destino (px) de una fuente `sw×sh` sobre un lienzo `cw×ch` con la
/// transformación dada. Devuelve `(x, y, w, h)` (puede salirse del lienzo).
pub fn dest_rect(sw: u32, sh: u32, cw: u32, ch: u32, t: &Transform) -> (i64, i64, u32, u32) {
    if sw == 0 || sh == 0 || cw == 0 || ch == 0 {
        return (0, 0, 0, 0);
    }
    let (mut w, mut h) = match t.fit {
        FitMode::Stretch => (cw as f64, ch as f64),
        FitMode::Native => (sw as f64, sh as f64),
        FitMode::Fit => {
            let s = (cw as f64 / sw as f64).min(ch as f64 / sh as f64);
            (sw as f64 * s, sh as f64 * s)
        }
        FitMode::Fill => {
            let s = (cw as f64 / sw as f64).max(ch as f64 / sh as f64);
            (sw as f64 * s, sh as f64 * s)
        }
    };
    w *= t.scale as f64;
    h *= t.scale as f64;
    let cx = cw as f64 / 2.0 + t.x as f64 * cw as f64;
    let cy = ch as f64 / 2.0 + t.y as f64 * ch as f64;
    let x = (cx - w / 2.0).round() as i64;
    let y = (cy - h / 2.0).round() as i64;
    (x, y, w.round().max(1.0) as u32, h.round().max(1.0) as u32)
}

/// Lienzo de composición.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Canvas {
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for px in rgba.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        Canvas { width, height, rgba }
    }

    pub fn clear(&mut self) {
        for px in self.rgba.as_chunks_mut::<4>().0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
            px[3] = 255;
        }
    }

    /// Dibuja `src` con la transformación (vecino más cercano, mezcla alfa
    /// «source over», opacidad global). `src` se asume alfa no premultiplicado.
    pub fn draw(&mut self, src: &Frame, t: &Transform) {
        let (dx, dy, dw, dh) = dest_rect(src.width, src.height, self.width, self.height, t);
        if dw == 0 || dh == 0 {
            return;
        }
        let opacity = t.opacity.clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return;
        }
        let op = (opacity * 256.0).round() as u32;
        let x0 = dx.max(0);
        let y0 = dy.max(0);
        let x1 = (dx + dw as i64).min(self.width as i64);
        let y1 = (dy + dh as i64).min(self.height as i64);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let exact = dw == src.width && dh == src.height;
        for y in y0..y1 {
            let sy = if exact { (y - dy) as u32 } else { ((y - dy) as u64 * src.height as u64 / dh as u64) as u32 };
            let src_row = (sy * src.width * 4) as usize;
            let dst_row = (y as u32 * self.width * 4) as usize;
            for x in x0..x1 {
                let sx = if exact { (x - dx) as u32 } else { ((x - dx) as u64 * src.width as u64 / dw as u64) as u32 };
                let si = src_row + (sx * 4) as usize;
                let di = dst_row + (x as u32 * 4) as usize;
                let a = (src.rgba[si + 3] as u32 * op) >> 8; // 0..256
                if a >= 255 {
                    self.rgba[di] = src.rgba[si];
                    self.rgba[di + 1] = src.rgba[si + 1];
                    self.rgba[di + 2] = src.rgba[si + 2];
                    self.rgba[di + 3] = 255;
                } else if a > 0 {
                    let inv = 256 - a;
                    for k in 0..3 {
                        let s = src.rgba[si + k] as u32;
                        let d = self.rgba[di + k] as u32;
                        self.rgba[di + k] = ((s * a + d * inv) >> 8) as u8;
                    }
                    self.rgba[di + 3] = 255;
                }
            }
        }
    }

    pub fn to_frame(&self) -> Frame {
        Frame { width: self.width, height: self.height, rgba: Arc::new(self.rgba.clone()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_letterboxes_and_alpha_blends() {
        let mut c = Canvas::new(100, 50);
        // fuente cuadrada roja opaca → ajustada 50×50 centrada
        let red = Frame::from_rgba(2, 2, [255, 0, 0, 255].repeat(4));
        c.draw(&red, &Transform::default());
        assert_eq!(&c.rgba[((25 * 100 + 50) * 4) as usize..((25 * 100 + 50) * 4 + 4) as usize], &[255, 0, 0, 255]);
        assert_eq!(&c.rgba[((25 * 100 + 10) * 4) as usize..((25 * 100 + 10) * 4 + 4) as usize], &[0, 0, 0, 255]);
        // overlay verde 50 % sobre rojo → mezcla
        let green = Frame::from_rgba(1, 1, vec![0, 255, 0, 128]);
        c.draw(&green, &Transform { fit: FitMode::Stretch, ..Default::default() });
        let px = &c.rgba[((25 * 100 + 50) * 4) as usize..((25 * 100 + 50) * 4 + 4) as usize];
        assert!(px[0] > 120 && px[0] < 135 && px[1] > 120 && px[1] < 135, "{px:?}");
        assert_eq!(dest_rect(1920, 1080, 1280, 720, &Transform::default()), (0, 0, 1280, 720));
        assert_eq!(dest_rect(1080, 1920, 1920, 1080, &Transform::default()).2, 608);
    }
}
