use dialoguer::{theme::ColorfulTheme, Select};
use std::path::{Path, PathBuf};
use steam_shortcuts_util::{parse_shortcuts, shortcut::ShortcutOwned, shortcuts_to_bytes};

/// Resolved Steam user directory.
pub struct SteamUserDir {
	pub path: PathBuf,
}

/// Find and select a Steam user directory.
pub fn find_user_dir(steam_userdata: Option<&Path>) -> Result<SteamUserDir, String> {
	let user_dir = match steam_userdata {
		Some(path) => {
			if path.ends_with("userdata") || has_userdata_children(path) {
				choose_user_dir(path.to_path_buf())?
			} else {
				// Assume the full user directory was given.
				path.to_path_buf()
			}
		},
		None => {
			let userdata_dir = xdg::BaseDirectories::new()
				.map_err(|_| "Failed to find Steam userdata directory. Provide --steam-userdata.".to_string())?
				.get_data_home()
				.join("Steam/userdata");
			choose_user_dir(userdata_dir)?
		},
	};

	Ok(SteamUserDir { path: user_dir })
}

/// Load existing shortcuts from shortcuts.vdf.
pub fn load_shortcuts(user_dir: &SteamUserDir) -> Result<Vec<ShortcutOwned>, String> {
	let path = user_dir.path.join("config/shortcuts.vdf");
	if !path.exists() {
		return Ok(Vec::new());
	}
	let data = std::fs::read(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
	let shortcuts = parse_shortcuts(&data).map_err(|e| format!("Failed to parse shortcuts: {e}"))?;
	Ok(shortcuts.into_iter().map(|s| s.to_owned()).collect())
}

/// Save shortcuts to shortcuts.vdf.
pub fn save_shortcuts(user_dir: &SteamUserDir, shortcuts: &[ShortcutOwned]) -> Result<(), String> {
	let path = user_dir.path.join("config/shortcuts.vdf");
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
	}
	let borrowed: Vec<_> = shortcuts.iter().map(ShortcutOwned::borrow).collect();
	let data = shortcuts_to_bytes(&borrowed);
	std::fs::write(&path, data).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
	Ok(())
}

/// Return the grid directory path for a user.
pub fn grid_dir(user_dir: &SteamUserDir) -> PathBuf {
	user_dir.path.join("config/grid")
}

/// Install a grid image (cover art) for a shortcut.
pub fn install_grid_image(user_dir: &SteamUserDir, app_id: u32, image_data: &[u8]) -> Result<(), String> {
	let dir = grid_dir(user_dir);
	std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create grid dir: {e}"))?;
	let path = dir.join(format!("{app_id}p.png"));
	std::fs::write(&path, image_data).map_err(|e| format!("Failed to write grid image: {e}"))?;
	Ok(())
}

/// Remove all grid images associated with a given app_id.
pub fn remove_grid_images(user_dir: &SteamUserDir, app_id: u32) -> Result<(), String> {
	let dir = grid_dir(user_dir);
	let suffixes = ["", "p", "_hero", "_logo"];
	let extensions = ["png", "jpg"];
	for suffix in &suffixes {
		for ext in &extensions {
			let path = dir.join(format!("{app_id}{suffix}.{ext}"));
			if path.exists() {
				std::fs::remove_file(&path).map_err(|e| format!("Failed to remove {}: {e}", path.display()))?;
			}
		}
	}
	Ok(())
}

/// Filter shortcuts to only those tagged "moonlight".
pub fn moonlight_shortcuts(shortcuts: &[ShortcutOwned]) -> Vec<&ShortcutOwned> {
	shortcuts
		.iter()
		.filter(|s| s.tags.contains(&"moonlight".to_string()))
		.collect()
}

/// Check if a directory looks like the userdata root (contains numeric subdirectories).
fn has_userdata_children(path: &Path) -> bool {
	std::fs::read_dir(path)
		.map(|entries| entries.filter_map(Result::ok).any(|e| e.path().is_dir()))
		.unwrap_or(false)
}

fn choose_user_dir(steam_users_dir: PathBuf) -> Result<PathBuf, String> {
	let user_dirs: Vec<PathBuf> = std::fs::read_dir(&steam_users_dir)
		.map_err(|e| format!("Failed to read Steam userdata at '{}': {e}", steam_users_dir.display()))?
		.filter_map(Result::ok)
		.map(|d| d.path())
		.filter(|d| d.is_dir())
		.collect();

	if user_dirs.is_empty() {
		return Err(format!("No user directories found in '{}'.", steam_users_dir.display()));
	}

	if user_dirs.len() == 1 {
		return Ok(user_dirs[0].clone());
	}

	let usernames = user_dirs_to_usernames(&user_dirs);
	let options: Vec<String> = user_dirs
		.iter()
		.zip(&usernames)
		.map(|(dir, name)| format!("{} ({})", dir.display(), name))
		.collect();

	let selection = Select::with_theme(&ColorfulTheme::default())
		.with_prompt("Pick your userdir (or run using --steam-userdata):")
		.default(0)
		.items(&options)
		.interact()
		.map_err(|e| format!("Failed to select userdir: {e}"))?;

	Ok(user_dirs[selection].clone())
}

fn user_dirs_to_usernames(userdirs: &[PathBuf]) -> Vec<String> {
	userdirs
		.iter()
		.map(|userdir| {
			let localconf = userdir.join("config/localconfig.vdf");
			let content = match std::fs::read_to_string(localconf) {
				Ok(s) => s,
				Err(_) => return "UNKNOWN".to_string(),
			};

			let lines: Vec<&str> = content.lines().filter(|l| l.contains("PersonaName")).collect();
			if lines.len() != 1 {
				return "UNKNOWN".to_string();
			}

			lines[0].trim().split('"').nth(3).unwrap_or("UNKNOWN").to_string()
		})
		.collect()
}
