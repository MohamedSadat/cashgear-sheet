//! Colours as a file stores them, and what they look like on screen. Dark mode's swaps
//! happen here, on the way to the screen, and never touch the stored colour.

/// `0xRRGGBB`.
pub type Rgb = u32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorKind {
    /// Whatever the system would use: black text, or no fill.
    Auto,
    Rgb(Rgb),
    /// An index in file order: lt1, dk1, lt2, dk2, accent1 to 6, hlink, folHlink.
    Theme(u8),
    Indexed(u8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub kind: ColorKind,
    pub tint: f32,
}

impl Color {
    pub const AUTO: Self = Self {
        kind: ColorKind::Auto,
        tint: 0.0,
    };

    pub fn rgb(value: Rgb) -> Self {
        Self {
            kind: ColorKind::Rgb(value),
            tint: 0.0,
        }
    }
}

/// The workbook's theme colours, in the order `theme="n"` counts them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePalette(pub [Rgb; 12]);

impl Default for ThemePalette {
    /// The Office theme, for files with no theme part.
    fn default() -> Self {
        Self([
            0xFFFFFF, 0x000000, 0xE7E6E6, 0x44546A, 0x4472C4, 0xED7D31, 0xA5A5A5, 0xFFC000,
            0x5B9BD5, 0x70AD47, 0x0563C1, 0x954F72,
        ])
    }
}

/// The legacy palette a file can override with `<indexedColors>`.
pub const DEFAULT_INDEXED: [Rgb; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000,
    0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x800000, 0x008000,
    0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC,
    0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF,
    0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF,
    0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600,
    0x666699, 0x969696, 0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399,
    0x333333,
];

/// The screen's own colours, which dark mode's swaps fall back on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DarkColors {
    pub background: Rgb,
    pub text: Rgb,
}

/// Everything needed to turn a stored colour into a screen colour.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorSpace {
    pub theme: ThemePalette,
    pub indexed: Vec<Rgb>,
}

impl Default for ColorSpace {
    fn default() -> Self {
        Self {
            theme: ThemePalette::default(),
            indexed: DEFAULT_INDEXED.to_vec(),
        }
    }
}

impl ColorSpace {
    /// `None` means automatic. `dark` swaps `lt1` and `dk1` for the screen's own colours.
    pub fn resolve(&self, color: Color, dark: Option<DarkColors>) -> Option<Rgb> {
        let base = match color.kind {
            ColorKind::Auto => return None,
            ColorKind::Rgb(value) => value & 0xFF_FFFF,
            ColorKind::Theme(index) => match (index, dark) {
                (0, Some(colors)) => colors.background,
                (1, Some(colors)) => colors.text,
                _ => *self.theme.0.get(usize::from(index))?,
            },
            // 64 and 65 are the system's foreground and background.
            ColorKind::Indexed(64 | 65) => return None,
            ColorKind::Indexed(index) => *self.indexed.get(usize::from(index))?,
        };
        Some(apply_tint(base, color.tint))
    }
}

fn to_hls(rgb: Rgb) -> (f64, f64, f64) {
    let r = f64::from((rgb >> 16) & 0xFF) / 255.0;
    let g = f64::from((rgb >> 8) & 0xFF) / 255.0;
    let b = f64::from(rgb & 0xFF) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, l, 0.0);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < f64::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f64::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, l, s)
}

fn from_hls(h: f64, l: f64, s: f64) -> Rgb {
    // Truncated, not rounded: that is what reproduces the colours Excel shows.
    let channel = |value: f64| ((value * 255.0 + 1e-9).floor() as u32).min(255);
    if s == 0.0 {
        let v = channel(l);
        return (v << 16) | (v << 8) | v;
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |t: f64| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (channel(hue(h + 1.0 / 3.0)) << 16) | (channel(hue(h)) << 8) | channel(hue(h - 1.0 / 3.0))
}

/// Excel's tint: below zero darkens by that share of the lightness, above zero lightens.
pub fn apply_tint(rgb: Rgb, tint: f32) -> Rgb {
    if tint == 0.0 {
        return rgb;
    }
    let tint = f64::from(tint).clamp(-1.0, 1.0);
    let (h, l, s) = to_hls(rgb);
    let l = if tint < 0.0 {
        l * (1.0 + tint)
    } else {
        l * (1.0 - tint) + tint
    };
    from_hls(h, l, s)
}

fn luminance(rgb: Rgb) -> f64 {
    let channel = |value: u32| {
        let v = f64::from(value & 0xFF) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb >> 16) + 0.7152 * channel(rgb >> 8) + 0.0722 * channel(rgb)
}

/// The WCAG contrast ratio, from 1 to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Dark mode leaves text alone unless its contrast with its background is under this.
pub const MIN_CONTRAST: f64 = 3.0;
/// Text that had to move goes this far, so that it reads comfortably, not barely.
const MOVED_CONTRAST: f64 = 4.5;

/// Leaves a colour with `MIN_CONTRAST` against `background` exactly as it is. Otherwise moves
/// it towards white or black, keeping its hue where it can, until it reads comfortably.
pub fn ensure_contrast(color: Rgb, background: Rgb) -> Rgb {
    if contrast(color, background) >= MIN_CONTRAST {
        return color;
    }
    let target: u32 = if luminance(background) < 0.18 {
        0xFF
    } else {
        0
    };
    let mix = |value: u32, share: u32| (value * (100 - share) + target * share + 50) / 100;
    for share in (10..=100).step_by(10) {
        let mixed = (mix((color >> 16) & 0xFF, share) << 16)
            | (mix((color >> 8) & 0xFF, share) << 8)
            | mix(color & 0xFF, share);
        if contrast(mixed, background) >= MOVED_CONTRAST {
            return mixed;
        }
    }
    (target << 16) | (target << 8) | target
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK: DarkColors = DarkColors {
        background: 0x1E1E1E,
        text: 0xE6E6E6,
    };

    #[test]
    fn tint_matches_excel_on_the_office_theme() {
        // Accent 1 "lighter 40%" and "darker 25%" as Excel shows them.
        assert_eq!(apply_tint(0x4472C4, 0.399_975_6), 0x8EAADB);
        assert_eq!(apply_tint(0x4472C4, -0.249_977_1), 0x2F5496);
        // White "darker 5%".
        assert_eq!(apply_tint(0xFFFFFF, -0.049_989_3), 0xF2F2F2);
    }

    #[test]
    fn automatic_and_system_colours_have_no_colour_of_their_own() {
        let space = ColorSpace::default();
        assert_eq!(space.resolve(Color::AUTO, None), None);
        let system = Color {
            kind: ColorKind::Indexed(64),
            tint: 0.0,
        };
        assert_eq!(space.resolve(system, None), None);
    }

    #[test]
    fn dark_mode_swaps_lt1_and_dk1_and_nothing_else() {
        let space = ColorSpace::default();
        let theme = |index| Color {
            kind: ColorKind::Theme(index),
            tint: 0.0,
        };
        assert_eq!(space.resolve(theme(1), None), Some(0x000000));
        assert_eq!(space.resolve(theme(1), Some(DARK)), Some(DARK.text));
        assert_eq!(space.resolve(theme(0), Some(DARK)), Some(DARK.background));
        assert_eq!(space.resolve(theme(4), Some(DARK)), Some(0x4472C4));
        assert_eq!(
            space.resolve(Color::rgb(0x000000), Some(DARK)),
            Some(0x000000)
        );
    }

    #[test]
    fn low_contrast_text_moves_and_readable_text_stays() {
        assert_eq!(ensure_contrast(0xFF0000, 0xFFFFFF), 0xFF0000);
        assert_eq!(ensure_contrast(0xFFFFFF, 0x1E1E1E), 0xFFFFFF);
        let moved = ensure_contrast(0x000000, 0x1E1E1E);
        assert!(contrast(moved, 0x1E1E1E) >= MIN_CONTRAST);
        // Dark blue keeps some of its blue on the way up.
        let blue = ensure_contrast(0x000080, 0x1E1E1E);
        assert!(contrast(blue, 0x1E1E1E) >= MIN_CONTRAST);
        assert!(blue & 0xFF > (blue >> 16) & 0xFF);
    }
}
