use clap::{ArgAction, Parser};
use dialoguer::{theme::ColorfulTheme, Select};
use steam_shortcuts_util::{parse_shortcuts, shortcut::ShortcutOwned, shortcuts_to_bytes, Shortcut};
use std::{io::Cursor, path::PathBuf, process::Command};

#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
	/// Host to retrieve apps from.
	host: String,

	/// Path to the Moonlight executable.
	#[clap(short, long)]
	moonlight: Option<PathBuf>,

	/// Path to the userdata directory of Steam.
	#[clap(short, long)]
	steam_userdata: Option<PathBuf>,

	/// The used Moonlight is installed through Flatpak.
	#[clap(short, long)]
	flatpak: bool,

	/// Don't remove existing games tagged as "moonlight".
	#[clap(long = "no-sync", action = ArgAction::SetFalse)]
	sync: bool,

	/// Don't override the shortcuts file, just print the Moonlight apps that were found.
	#[clap(long)]
	dry_run: bool,
}

fn main() -> Result<(), String> {
	let args = Args::parse();

	let moonlight_path = match args.moonlight {
		Some(path) => path.canonicalize().map_err(|e| format!("Failed to find absolute path of moonlight ('{}'): {e}", path.display()))?,
		None => {
			which::which("moonlight")
				.map_err(|_| "Failed to find Moonlight executable. Make sure it is in your PATH environment variable.".to_string())?
		},
	};

	if !moonlight_path.exists() || !moonlight_path.is_file() {
		return Err(format!("Moonlight at '{:?}' does not exist or is not a file.", moonlight_path));
	}

	println!("Found Moonlight at '{moonlight_path:?}'.");

	let userdata_dir = match args.steam_userdata {
		Some(path) => {
			if path.ends_with("userdata") {
				// Assume we got the `userdata` directory.
				choose_user_dir(path)?
			} else {
				// Assume we got the full user directory.
				path
			}
		},
		None => {
			let steam_users_dir = xdg::BaseDirectories::new()
				.map_err(|_| "Failed to retrieve Steam userdata directory. Please provide a directory using --steam-userdata <steam_dir>.".to_string())?
				.get_data_home().join("Steam/userdata");

			choose_user_dir(steam_users_dir)?
		},
	};

	let shortcuts_path = userdata_dir.join("config/shortcuts.vdf");

	let mut shortcuts = if !shortcuts_path.exists() {
		println!("Creating shortcuts file at {}.", shortcuts_path.display());
		Vec::new()
	} else {
		let shortcuts_file = std::fs::read(&shortcuts_path)
			.map_err(|e| format!("Failed to read existing shortcuts file: {e}"))?;
		parse_shortcuts(&shortcuts_file)
			.map_err(|e| format!("Failed to parse shortcuts: {e}"))?
			.into_iter()
			.map(|s| s.to_owned())
			.collect()
	};

	if args.sync {
		// Remove all games that are "moonlight" games.
		shortcuts.retain(|s| !s.tags.contains(&"moonlight".to_string()));
	}

	println!("Retrieving apps from Moonlight ...");
	let moonlight_apps = Command::new(&moonlight_path)
		.args([
			"list",
			&args.host,
			"--csv"
		])
		.output()
		.map_err(|e| format!("Failed to request apps from moonlight: {e}"))?;
	println!("Finished retrieving apps from Moonlight.");

	if !moonlight_apps.status.success() {
		println!("Output from Moonlight: {moonlight_apps:?}");
		return Err("Failed to get apps from Moonlight.".to_string());
	}

	let cursor = Cursor::new(moonlight_apps.stdout);
	let mut reader = csv::Reader::from_reader(cursor);

	let mut new_shortcuts = Vec::new();
	for record in reader.records() {
		match record {
			Ok(record) => {
				if record.len() != 7 {
					return Err(format!("Expected exactly 7 entries in record, but got {}: {:?}", record.len(), record));
				}

				let title = &record[0];
				let launch_options = format!("stream {} \"{title}\"", args.host);

				let icon = if record[6].contains("no_app_image") { "" } else { record[6].strip_prefix("file://").unwrap() };
				let mut shortcut = Shortcut::new(
					"",
					title,
					&moonlight_path.to_string_lossy(),
					"",
					icon,
					"",
					&launch_options,
				).to_owned();
				shortcut.tags.push("moonlight".to_string());

				println!("{title} => '{} {launch_options}' (icon: '{icon}')", moonlight_path.display());
				new_shortcuts.push(shortcut);
			},
			Err(e) => {
				return Err(format!("Failed to parse CSV from Moonlight: {e}"));
			},
		}
	}

	if !args.dry_run {
		shortcuts.extend(new_shortcuts);
		let serialized = shortcuts_to_bytes(&shortcuts.iter().map(ShortcutOwned::borrow).collect());
		println!("Shortcuts file: {shortcuts_path:?}");
		std::fs::write(&shortcuts_path, serialized)
			.map_err(|e| format!("Failed to write shortcuts to file: {e}"))?;
	}

	Ok(())
}

fn choose_user_dir(steam_users_dir: PathBuf) -> Result<PathBuf, String> {
	let user_dirs: Vec<PathBuf> = std::fs::read_dir(steam_users_dir)
		.map_err(|e| format!("Failed to read Steam user dir: {e}"))?
		.filter_map(Result::ok)
		.map(|d| d.path())
		.filter(|d| d.is_dir())
		.collect();

	if user_dirs.len() == 1 {
		return Ok(user_dirs[0].clone());
	}

	let usernames = user_dirs_to_usernames(&user_dirs);

	let options = user_dirs.iter().zip(usernames);

	let selection = Select::with_theme(&ColorfulTheme::default())
		.with_prompt("Pick your userdir (or run using --steam-userdata):")
		.default(0)
		.items(&options.map(|(dir, name)| format!("{} ({})", dir.display(), name)).collect::<Vec<String>>())
		.interact()
		.map_err(|e| format!("Failed to select userdir: {e}"))?;

	Ok(user_dirs[selection].clone())
}

fn user_dirs_to_usernames(userdirs: &[PathBuf]) -> Vec<String> {
	let mut usernames = Vec::new();

	for userdir in userdirs {
		let localconf = userdir.join("config/localconfig.vdf");
		let lines: Vec<String> = match std::fs::read_to_string(localconf) {
			Ok(s) => s.lines().filter(|l| l.contains("PersonaName")).map(String::from).collect(),
			Err(_) => {
				usernames.push("UNKNOWN".to_string());
				continue;
			},
		};

		if lines.len() != 1 {
			usernames.push("UNKNOWN".to_string());
			continue;
		}

		match lines[0].trim().split('"').nth(3) {
			Some(name) => usernames.push(name.to_string()),
			None => {
				usernames.push("UNKNOWN".to_string());
				continue;
			}
		}
	}

	usernames
}
