//! Colour arithmetic for the roles of a palette: hex in and out, mixing, contrast (WCAG), the turn of a hue, and the pass that pulls a
//! colour toward its text until it can be read. Nothing here knows a role.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`.
    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#').filter(|h| h.len() == 6 && h.is_ascii())?;
        let part = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(part(0)?, part(2)?, part(4)?))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// `self` moved `k` of the way (0 to 1) toward `to`, channel by channel.
    pub fn mix(self, to: Rgb, k: f64) -> Rgb {
        let ch = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * k).round().clamp(0.0, 255.0) as u8;
        Rgb(ch(self.0, to.0), ch(self.1, to.1), ch(self.2, to.2))
    }

    /// Relative luminance, 0 (black) to 1 (white).
    pub fn luminance(self) -> f64 {
        let lin = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// The contrast ratio of two colours, 1 to 21.
    pub fn contrast(self, other: Rgb) -> f64 {
        let (x, y) = (self.luminance(), other.luminance());
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// The hue turned by `degrees`, lightness and saturation kept.
    pub fn rotate(self, degrees: f64) -> Rgb {
        let (r, g, b) = (f64::from(self.0) / 255.0, f64::from(self.1) / 255.0, f64::from(self.2) / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            return self;
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r { ((g - b) / d).rem_euclid(6.0) } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 } * 60.0;
        let h = (h + degrees).rem_euclid(360.0);
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
        let (r1, g1, b1) = match (h / 60.0) as u8 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = l - c / 2.0;
        let ch = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb(ch(r1), ch(g1), ch(b1))
    }
}

/// `fg` moved `k` toward `bg`, and then back toward `fg` a hundredth at a time until it has `floor` contrast on every one of `backs`
/// (or is `fg` itself).
pub fn pass(fg: Rgb, backs: &[Rgb], k: f64, floor: f64) -> Rgb {
    let Some(&to) = backs.first() else { return fg };
    let mut k = k;
    while k > 0.0 && backs.iter().any(|b| fg.mix(to, k).contrast(*b) < floor) {
        k -= 0.01;
    }
    fg.mix(to, k.max(0.0))
}

/// `fg` moved toward `text` a hundredth at a time until it has `floor` contrast on every one of `backs` (or is `text`).
pub fn reach(fg: Rgb, text: Rgb, backs: &[Rgb], floor: f64) -> Rgb {
    (0..=100).map(|n| fg.mix(text, f64::from(n) / 100.0)).find(|c| backs.iter().all(|b| c.contrast(*b) >= floor)).unwrap_or(text)
}
