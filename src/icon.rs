//! App branding: the VoiceForge logo (mic + anvil), rendered as RGBA for the tray icon,
//! window icon, and any other native surface that needs raw pixels.
//!
//! The tray icon is the logo with a small status dot baked into the corner (blue while
//! idle, red while listening) so the tray still communicates state at a glance.

/// A raw RGBA image: width, height, and `width*height*4` bytes.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

/// The source logo, decoded fresh each call (cheap: a few hundred KB, done a handful of
/// times at startup).
fn logo() -> image::RgbaImage {
    let bytes = include_bytes!("../assets/logo.png");
    image::load_from_memory(bytes)
        .expect("bundled logo.png must decode")
        .to_rgba8()
}

/// The plain logo at `size`×`size`, no status dot — used for the window icon.
pub fn app_icon(size: u32) -> Rgba {
    let resized = image::imageops::resize(&logo(), size, size, image::imageops::FilterType::Lanczos3);
    Rgba {
        width: size,
        height: size,
        bytes: resized.into_raw(),
    }
}

/// Idle tray badge: logo with a blue status dot.
pub fn idle() -> Rgba {
    badge([0x2f, 0x6f, 0xed, 0xff])
}

/// Listening tray badge: logo with a red status dot.
pub fn listening() -> Rgba {
    badge([0xe0, 0x3b, 0x3b, 0xff])
}

/// Renders the logo at tray size with a filled status-color dot in the bottom-right corner.
fn badge(dot_color: [u8; 4]) -> Rgba {
    const S: u32 = 32;
    let mut img = image::imageops::resize(&logo(), S, S, image::imageops::FilterType::Lanczos3);

    let cx = S as i32 - 8;
    let cy = S as i32 - 8;
    let r = 7;
    // White ring so the dot reads clearly against any background pixel underneath it.
    for y in -r - 1..=r + 1 {
        for x in -r - 1..=r + 1 {
            let px = cx + x;
            let py = cy + y;
            if px < 0 || py < 0 || px >= S as i32 || py >= S as i32 {
                continue;
            }
            let dist2 = x * x + y * y;
            let color = if dist2 <= r * r {
                dot_color
            } else if dist2 <= (r + 1) * (r + 1) {
                [0xff, 0xff, 0xff, 0xff]
            } else {
                continue;
            };
            img.put_pixel(px as u32, py as u32, image::Rgba(color));
        }
    }

    Rgba {
        width: S,
        height: S,
        bytes: img.into_raw(),
    }
}
