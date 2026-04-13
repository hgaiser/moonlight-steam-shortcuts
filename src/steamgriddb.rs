use serde::Deserialize;
use std::io::Read;

const BASE_URL: &str = "https://www.steamgriddb.com/api/v2";

#[derive(Deserialize)]
struct SearchResponse {
	success: bool,
	data: Vec<Game>,
}

#[derive(Deserialize)]
struct Game {
	id: u64,
}

#[derive(Deserialize)]
struct ImageResponse {
	success: bool,
	data: Vec<Image>,
}

#[derive(Deserialize)]
struct Image {
	url: String,
}

/// Fetch the first matching wide grid image from SteamGridDB for the given game name.
///
/// Returns the raw image bytes (JPEG or PNG), or `None` if no match was found.
pub fn fetch_wide_grid(game_name: &str, api_key: &str) -> Result<Option<Vec<u8>>, String> {
	let game_id = match search_game(game_name, api_key)? {
		Some(id) => id,
		None => return Ok(None),
	};

	let url = format!("{BASE_URL}/grids/game/{game_id}?styles=alternate");
	let image_url = match first_image_url(&url, api_key)? {
		Some(u) => u,
		None => return Ok(None),
	};

	download_image(&image_url).map(Some)
}

/// Fetch the first available hero image from SteamGridDB for the given game name.
///
/// Returns the raw image bytes, or `None` if no match was found.
pub fn fetch_hero(game_name: &str, api_key: &str) -> Result<Option<Vec<u8>>, String> {
	let game_id = match search_game(game_name, api_key)? {
		Some(id) => id,
		None => return Ok(None),
	};

	let url = format!("{BASE_URL}/heroes/game/{game_id}");
	let image_url = match first_image_url(&url, api_key)? {
		Some(u) => u,
		None => return Ok(None),
	};

	download_image(&image_url).map(Some)
}

/// Search SteamGridDB for a game by name, returning its ID.
fn search_game(name: &str, api_key: &str) -> Result<Option<u64>, String> {
	let encoded = urlencoding::encode(name);
	let url = format!("{BASE_URL}/search/autocomplete/{encoded}");

	let response = match authorized_get(&url, api_key).map_err(|e| *e) {
		Ok(r) => r,
		Err(ureq::Error::Status(404, _)) => return Ok(None),
		Err(e) => return Err(format!("SteamGridDB search request failed: {e}")),
	};

	let parsed: SearchResponse = response
		.into_json()
		.map_err(|e| format!("SteamGridDB search response parse failed: {e}"))?;

	if !parsed.success || parsed.data.is_empty() {
		return Ok(None);
	}

	Ok(Some(parsed.data[0].id))
}

/// Fetch the URL of the first image returned by the given SteamGridDB endpoint.
fn first_image_url(url: &str, api_key: &str) -> Result<Option<String>, String> {
	let response = match authorized_get(url, api_key).map_err(|e| *e) {
		Ok(r) => r,
		Err(ureq::Error::Status(404, _)) => return Ok(None),
		Err(e) => return Err(format!("SteamGridDB image list request failed: {e}")),
	};

	let parsed: ImageResponse = response
		.into_json()
		.map_err(|e| format!("SteamGridDB image list parse failed: {e}"))?;

	if !parsed.success || parsed.data.is_empty() {
		return Ok(None);
	}

	Ok(Some(parsed.data[0].url.clone()))
}

/// Download raw bytes from a URL.
fn download_image(url: &str) -> Result<Vec<u8>, String> {
	let response = ureq::get(url)
		.call()
		.map_err(|e| format!("Image download failed for '{url}': {e}"))?;

	let mut bytes = Vec::new();
	response
		.into_reader()
		.read_to_end(&mut bytes)
		.map_err(|e| format!("Image read failed: {e}"))?;

	Ok(bytes)
}

/// Perform an authorized GET request against SteamGridDB.
fn authorized_get(url: &str, api_key: &str) -> Result<ureq::Response, Box<ureq::Error>> {
	ureq::get(url).set("Authorization", &format!("Bearer {api_key}")).call().map_err(Box::new)
}
