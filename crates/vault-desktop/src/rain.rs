//! Decorative digital rain for the welcome/locked screen only.
//!
//! Glyphs are drawn from a fixed decorative alphabet with a non-secret PRNG;
//! document content is never used. Animation runs at a capped frame rate,
//! only while the locked screen is visible, and is disabled entirely by
//! "Reduce motion" or "Visual effects: off".

use std::time::Instant;

use egui::{Color32, Pos2, Rect};

use crate::theme;

const GLYPHS: &[char] = &[
    'λ', 'Σ', 'Ω', 'π', 'Δ', '≡', '±', 'ж', 'я', 'ψ', 'φ', '0', '1', '2', '3', '4', '5', '6', '7',
    '8', '9', '<', '>', '{', '}', '[', ']', '/', '\\', '*', '+', '=', ':', ';', '¦', '§', 'µ', 'ß',
    'Ξ', 'Γ', 'Θ', 'Л', 'Ф', 'Ц',
];

struct Drop {
    x: f32,
    y: f32,
    speed: f32,
    len: usize,
    glyphs: Vec<char>,
}

pub struct Rain {
    drops: Vec<Drop>,
    rng: u64,
    last: Option<Instant>,
    area: Rect,
}

impl Default for Rain {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self {
            drops: Vec::new(),
            rng: seed | 1,
            last: None,
            area: Rect::NOTHING,
        }
    }
}

impl Rain {
    fn next(&mut self) -> u64 {
        // xorshift64* — decorative only.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn rand_f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn glyph(&mut self) -> char {
        GLYPHS[(self.next() % GLYPHS.len() as u64) as usize]
    }

    fn reset(&mut self, area: Rect, col_w: f32) {
        self.area = area;
        self.drops.clear();
        let cols = (area.width() / col_w).max(1.0) as usize;
        for c in 0..cols {
            if self.rand_f() < 0.55 {
                continue;
            }
            let len = 6 + (self.next() % 18) as usize;
            let glyphs = (0..len).map(|_| self.glyph()).collect();
            let y = area.top() - self.rand_f() * area.height();
            let speed = 40.0 + self.rand_f() * 110.0;
            self.drops.push(Drop {
                x: area.left() + c as f32 * col_w + col_w * 0.5,
                y,
                speed,
                len,
                glyphs,
            });
        }
    }

    /// Paints one frame. Returns true if another frame should be scheduled.
    pub fn paint(&mut self, painter: &egui::Painter, area: Rect, animate: bool) -> bool {
        let size = 15.0;
        let col_w = size * 1.15;
        if self.area != area || self.drops.is_empty() {
            self.reset(area, col_w);
        }
        let now = Instant::now();
        let dt = self
            .last
            .map(|l| now.duration_since(l).as_secs_f32())
            .unwrap_or(0.0)
            .min(0.1);
        self.last = Some(now);
        let font = theme::mono(size);
        let n = self.drops.len();
        for i in 0..n {
            if animate {
                self.drops[i].y += self.drops[i].speed * dt;
                if self.rand_f() < 0.04 {
                    let g = self.glyph();
                    let k = (self.next() % self.drops[i].len as u64) as usize;
                    self.drops[i].glyphs[k] = g;
                }
                let tail = self.drops[i].len as f32 * size;
                if self.drops[i].y - tail > area.bottom() {
                    self.drops[i].y = area.top() - self.rand_f() * area.height() * 0.3;
                    self.drops[i].speed = 40.0 + self.rand_f() * 110.0;
                }
            }
            let d = &self.drops[i];
            for (k, g) in d.glyphs.iter().enumerate() {
                let y = d.y - k as f32 * size;
                if y < area.top() - size || y > area.bottom() + size {
                    continue;
                }
                let fade = 1.0 - k as f32 / d.len as f32;
                let color = if k == 0 {
                    Color32::from_rgb(0xB8, 0xFF, 0xD6).gamma_multiply(0.55)
                } else {
                    theme::GREEN.gamma_multiply(0.22 * fade)
                };
                painter.text(
                    Pos2::new(d.x, y),
                    egui::Align2::CENTER_CENTER,
                    g.to_string(),
                    font.clone(),
                    color,
                );
            }
        }
        animate
    }
}
