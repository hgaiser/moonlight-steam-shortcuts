use serde::Deserialize;

const SEARCH_URL: &str = "https://store.steampowered.com/api/storesearch/";

#[derive(Deserialize)]
struct SearchResponse {
	items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
	id: u32,
	name: String,
	#[serde(rename = "type")]
	item_type: String,
}

/// Look up the Steam App ID for a game by name using the Steam Store search API.
///
/// Returns the ID of the first `app` result whose name matches exactly (case-insensitive),
/// or the first `app` result if no exact match is found.
/// Returns `None` if no app results are returned.
pub fn find_app_id(name: &str) -> Option<u32> {
	let encoded = urlencoding::encode(name);
	let url = format!("{SEARCH_URL}?term={encoded}&cc=US&l=en");

	let response = ureq::get(&url).call().ok()?;
	let parsed: SearchResponse = response.into_json().ok()?;

	let apps: Vec<&Item> = parsed.items.iter().filter(|i| i.item_type == "app").collect();
	if apps.is_empty() {
		return None;
	}

	// Prefer an exact name match (case-insensitive), fall back to the first result.
	apps.iter()
		.find(|i| i.name.eq_ignore_ascii_case(name))
		.or(Some(&apps[0]))
		.map(|i| i.id)
}
