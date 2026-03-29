use image::{imageops, DynamicImage, GenericImageView, ImageFormat, RgbaImage};
use std::{io::Cursor, path::Path};

const MOONLIGHT_LOGO: &[u8] = include_bytes!("assets/moonlight_logo.png");

/// Load a boxart image from a local file path.
pub fn load_boxart(path: &Path) -> Result<DynamicImage, String> {
	if !path.is_file() {
		return Err(format!("Boxart file '{}' does not exist.", path.display()));
	}
	image::open(path).map_err(|e| format!("Failed to load boxart '{}': {e}", path.display()))
}

/// Composite the Moonlight logo overlay onto the bottom-right corner of the boxart.
///
/// Returns the composited image as PNG bytes.
pub fn apply_overlay(boxart: &DynamicImage) -> Result<Vec<u8>, String> {
	let logo = image::load_from_memory_with_format(MOONLIGHT_LOGO, ImageFormat::Png)
		.map_err(|e| format!("Failed to load embedded Moonlight logo: {e}"))?;

	let (bw, bh) = boxart.dimensions();
	let shorter = bw.min(bh);
	let logo_size = (shorter as f32 * 0.15).max(16.0) as u32;

	let resized_logo = logo.resize(logo_size, logo_size, imageops::FilterType::Lanczos3);
	let (lw, lh) = resized_logo.dimensions();

	let margin_x = (bw as f32 * 0.03) as u32;
	let margin_y = (bh as f32 * 0.03) as u32;
	let x = bw.saturating_sub(lw + margin_x);
	let y = bh.saturating_sub(lh + margin_y);

	let mut canvas = boxart.to_rgba8();
	overlay_rgba(&mut canvas, &resized_logo.to_rgba8(), x, y);

	let mut buf = Cursor::new(Vec::new());
	canvas
		.write_to(&mut buf, ImageFormat::Png)
		.map_err(|e| format!("Failed to encode composited image: {e}"))?;

	Ok(buf.into_inner())
}

/// Load boxart from path, apply overlay, and return PNG bytes.
/// If no boxart is available, returns None.
pub fn process_boxart(boxart_path: Option<&Path>, no_overlay: bool) -> Result<Option<Vec<u8>>, String> {
	let path = match boxart_path {
		Some(p) => p,
		None => return Ok(None),
	};

	let boxart = match load_boxart(path) {
		Ok(img) => img,
		Err(e) => {
			eprintln!("Warning: {e}");
			return Ok(None);
		},
	};

	if no_overlay {
		let mut buf = Cursor::new(Vec::new());
		boxart
			.write_to(&mut buf, ImageFormat::Png)
			.map_err(|e| format!("Failed to encode boxart: {e}"))?;
		return Ok(Some(buf.into_inner()));
	}

	match apply_overlay(&boxart) {
		Ok(data) => Ok(Some(data)),
		Err(e) => {
			eprintln!("Warning: overlay failed, using original boxart: {e}");
			let mut buf = Cursor::new(Vec::new());
			boxart
				.write_to(&mut buf, ImageFormat::Png)
				.map_err(|e| format!("Failed to encode boxart: {e}"))?;
			Ok(Some(buf.into_inner()))
		},
	}
}

/// Alpha-composite `top` onto `bottom` at offset (x, y).
fn overlay_rgba(bottom: &mut RgbaImage, top: &RgbaImage, ox: u32, oy: u32) {
	for (x, y, pixel) in top.enumerate_pixels() {
		let bx = ox + x;
		let by = oy + y;
		if bx < bottom.width() && by < bottom.height() {
			let bg = bottom.get_pixel(bx, by);
			let blended = alpha_blend(*bg, *pixel);
			bottom.put_pixel(bx, by, blended);
		}
	}
}

fn alpha_blend(bg: image::Rgba<u8>, fg: image::Rgba<u8>) -> image::Rgba<u8> {
	let fa = fg[3] as f32 / 255.0;
	let ba = bg[3] as f32 / 255.0;
	let out_a = fa + ba * (1.0 - fa);
	if out_a == 0.0 {
		return image::Rgba([0, 0, 0, 0]);
	}
	let r = (fg[0] as f32 * fa + bg[0] as f32 * ba * (1.0 - fa)) / out_a;
	let g = (fg[1] as f32 * fa + bg[1] as f32 * ba * (1.0 - fa)) / out_a;
	let b = (fg[2] as f32 * fa + bg[2] as f32 * ba * (1.0 - fa)) / out_a;
	image::Rgba([r as u8, g as u8, b as u8, (out_a * 255.0) as u8])
}
