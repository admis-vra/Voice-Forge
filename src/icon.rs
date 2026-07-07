//! Procedurally generated RGBA icons, so the app ships without binary image assets.
//!
//! Each icon is a simple filled rounded square badge with a microphone glyph, tinted
//! by status: blue when idle, red when listening.

/// A raw RGBA image: width, height, and `width*height*4` bytes.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

/// Idle badge (blue).
pub fn idle() -> Rgba {
    badge([0x2f, 0x6f, 0xed, 0xff])
}

/// Listening badge (red).
pub fn listening() -> Rgba {
    badge([0xe0, 0x3b, 0x3b, 0xff])
}

/// Renders a 32×32 rounded-square badge in `color` with a white microphone glyph.
fn badge(color: [u8; 4]) -> Rgba {
    const S: i32 = 32;
    let mut bytes = vec![0u8; (S * S * 4) as usize];

    let put = |bytes: &mut [u8], x: i32, y: i32, c: [u8; 4]| {
        if x < 0 || y < 0 || x >= S || y >= S {
            return;
        }
        let i = ((y * S + x) * 4) as usize;
        bytes[i..i + 4].copy_from_slice(&c);
    };

    // Rounded-square background.
    let radius = 7;
    for y in 0..S {
        for x in 0..S {
            if inside_rounded(x, y, S, radius) {
                put(&mut bytes, x, y, color);
            }
        }
    }

    // Microphone glyph (white): capsule body + stand.
    let white = [0xff, 0xff, 0xff, 0xff];
    // Capsule body: a vertical rounded bar.
    for y in 7..=18 {
        for x in 13..=18 {
            let near_edge = x == 13 || x == 18;
            // Round the top/bottom corners a touch.
            if (y == 7 || y == 18) && near_edge {
                continue;
            }
            put(&mut bytes, x, y, white);
        }
    }
    // Stand arc (approximate) + neck + base.
    for x in 10..=21 {
        put(&mut bytes, x, 20, white);
    }
    for y in 20..=24 {
        put(&mut bytes, 10, y, white);
        put(&mut bytes, 21, y, white);
    }
    for y in 20..=25 {
        put(&mut bytes, 15, y, white);
        put(&mut bytes, 16, y, white);
    }
    for x in 12..=19 {
        put(&mut bytes, x, 26, white);
    }

    Rgba {
        width: S as u32,
        height: S as u32,
        bytes,
    }
}

/// True if the pixel is inside a rounded square of side `s` with corner `radius`.
fn inside_rounded(x: i32, y: i32, s: i32, radius: i32) -> bool {
    let (mut cx, mut cy) = (x, y);
    // Reflect into the top-left corner region for a single distance test.
    if cx > s - 1 - radius {
        cx = s - 1 - cx;
    }
    if cy > s - 1 - radius {
        cy = s - 1 - cy;
    }
    if cx >= radius || cy >= radius {
        return true;
    }
    let dx = radius - cx;
    let dy = radius - cy;
    dx * dx + dy * dy <= radius * radius
}
