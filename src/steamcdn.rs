use std::io::Read;

const STEAM_CDN: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";

/// Fetch the hero image (library_hero.jpg, 1920x620) for a Steam app from Steam's CDN.
///
/// Returns `None` if the app ID is zero or Steam returns a non-200 response.
pub fn fetch_hero(app_id: u32) -> Option<Vec<u8>> {
	if app_id == 0 {
		return None;
	}
	let url = format!("{STEAM_CDN}/{app_id}/library_hero.jpg");
	download_image(&url)
}

/// Fetch the landscape wide grid image (header.jpg, 460x215) for a Steam app from Steam's CDN.
///
/// Returns `None` if the app ID is zero or Steam returns a non-200 response.
pub fn fetch_wide_grid(app_id: u32) -> Option<Vec<u8>> {
	if app_id == 0 {
		return None;
	}
	let url = format!("{STEAM_CDN}/{app_id}/header.jpg");
	download_image(&url)
}

/// Download raw bytes from a URL, returning `None` on any error or non-200 response.
fn download_image(url: &str) -> Option<Vec<u8>> {
	let response = match ureq::get(url).call() {
		Ok(r) if r.status() == 200 => r,
		_ => return None,
	};

	let mut bytes = Vec::new();
	if response.into_reader().read_to_end(&mut bytes).is_err() {
		return None;
	}

	Some(bytes)
}
