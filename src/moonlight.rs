use std::{
	collections::HashMap,
	io::Cursor,
	path::{Path, PathBuf},
	process::Command,
};

/// How Moonlight should be invoked.
#[derive(Clone, Debug)]
pub enum MoonlightBackend {
	/// Direct binary invocation.
	Binary(PathBuf),
	/// Invoke via `flatpak run com.moonlight_stream.Moonlight`.
	Flatpak,
}

/// An application available on a Moonlight host.
#[allow(dead_code)]
pub struct MoonlightApp {
	pub name: String,
	pub id: u32,
	/// Local file path to the boxart, or None if no boxart is available.
	pub boxart_path: Option<PathBuf>,
}

/// Resolve the Moonlight backend from CLI arguments.
///
/// Priority: --flatpak > --moonlight <path> > search PATH for `moonlight` > detect Flatpak.
pub fn resolve_backend(moonlight_path: Option<&Path>, flatpak: bool) -> Result<MoonlightBackend, String> {
	if flatpak {
		return Ok(MoonlightBackend::Flatpak);
	}

	if let Some(p) = moonlight_path {
		let path = p
			.canonicalize()
			.map_err(|e| format!("Failed to resolve Moonlight path '{}': {e}", p.display()))?;
		if !path.is_file() {
			return Err(format!("Moonlight at '{}' is not a file.", path.display()));
		}
		return Ok(MoonlightBackend::Binary(path));
	}

	// Auto-detect: try PATH first, then Flatpak.
	if let Ok(path) = which::which("moonlight") {
		return Ok(MoonlightBackend::Binary(path));
	}

	if is_flatpak_installed() {
		return Ok(MoonlightBackend::Flatpak);
	}

	Err("Moonlight not found. Install it or provide --moonlight <path> or --flatpak.".to_string())
}

/// Check if Moonlight is installed as a Flatpak.
fn is_flatpak_installed() -> bool {
	Command::new("flatpak")
		.args(["info", "com.moonlight_stream.Moonlight"])
		.stdout(std::process::Stdio::null())
		.stderr(std::process::Stdio::null())
		.status()
		.map(|s| s.success())
		.unwrap_or(false)
}

impl MoonlightBackend {
	/// Build a base `Command` for invoking Moonlight with the given arguments.
	fn command(&self, args: &[&str]) -> Command {
		match self {
			MoonlightBackend::Binary(path) => {
				let mut cmd = Command::new(path);
				cmd.args(args);
				cmd
			},
			MoonlightBackend::Flatpak => {
				let mut cmd = Command::new("flatpak");
				cmd.arg("run");
				cmd.arg("com.moonlight_stream.Moonlight");
				cmd.args(args);
				cmd
			},
		}
	}

	/// Return a string representation of the backend for use in launch_options.
	pub fn launch_flags(&self) -> String {
		match self {
			MoonlightBackend::Binary(path) => format!("--moonlight {}", path.display()),
			MoonlightBackend::Flatpak => "--flatpak".to_string(),
		}
	}
}

/// Query a Moonlight host for its available applications.
pub fn list_apps(backend: &MoonlightBackend, host: &str) -> Result<Vec<MoonlightApp>, String> {
	let output = backend
		.command(&["list", host, "--csv"])
		.output()
		.map_err(|e| format!("Failed to run Moonlight: {e}"))?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		return Err(format!("Moonlight list failed for host '{host}': {stderr}"));
	}

	let cursor = Cursor::new(output.stdout);
	let mut reader = csv::Reader::from_reader(cursor);
	let mut apps = Vec::new();

	for record in reader.records() {
		let record = record.map_err(|e| format!("Failed to parse CSV: {e}"))?;
		if record.len() != 7 {
			return Err(format!("Expected 7 CSV columns, got {}: {:?}", record.len(), record));
		}

		let name = record[0].to_string();
		let id: u32 = record[1]
			.parse()
			.map_err(|e| format!("Invalid app ID '{}': {e}", &record[1]))?;
		let hidden: bool = record[4].parse().unwrap_or(false);
		let is_app_collector: bool = record[3].parse().unwrap_or(false);

		// Skip hidden apps and app collector games.
		if hidden || is_app_collector {
			continue;
		}

		let boxart_path = if record[6].contains("no_app_image") {
			None
		} else {
			Some(PathBuf::from(record[6].strip_prefix("file://").unwrap_or(&record[6])))
		};

		apps.push(MoonlightApp { name, id, boxart_path });
	}

	Ok(apps)
}

/// Build a `Command` that will stream the given app.
pub fn stream_command(backend: &MoonlightBackend, host: &str, app: &str) -> Command {
	backend.command(&["stream", host, app])
}

/// A known host from Moonlight's configuration.
pub struct KnownHost {
	pub name: String,
	pub address: String,
}

/// Discover known hosts from Moonlight's config file.
///
/// Reads the QSettings INI file (`Moonlight Game Streaming Project/Moonlight.conf`)
/// and extracts the hostname and best address for each computer.
/// Checks both the standard and Flatpak config paths.
pub fn known_hosts(backend: &MoonlightBackend) -> Vec<KnownHost> {
	let config_paths = match backend {
		MoonlightBackend::Flatpak => vec![flatpak_config_path()],
		MoonlightBackend::Binary(_) => vec![standard_config_path(), flatpak_config_path()],
	};

	for path in config_paths.into_iter().flatten() {
		if path.is_file() {
			if let Ok(hosts) = parse_moonlight_config(&path) {
				if !hosts.is_empty() {
					return hosts;
				}
			}
		}
	}

	Vec::new()
}

fn standard_config_path() -> Option<PathBuf> {
	xdg::BaseDirectories::new().ok().map(|dirs| {
		dirs.get_config_home()
			.join("Moonlight Game Streaming Project/Moonlight.conf")
	})
}

fn flatpak_config_path() -> Option<PathBuf> {
	std::env::var_os("HOME").map(|home| {
		PathBuf::from(home)
			.join(".var/app/com.moonlight_stream.Moonlight/config/Moonlight Game Streaming Project/Moonlight.conf")
	})
}

/// Parse Moonlight's QSettings INI config to extract known hosts.
fn parse_moonlight_config(path: &Path) -> Result<Vec<KnownHost>, String> {
	let content = std::fs::read_to_string(path).map_err(|e| format!("Failed to read config: {e}"))?;

	// Find the [hosts] section and parse entries.
	// QSettings format: <index>\hostname=..., <index>\localaddress=..., <index>\manualaddress=...
	let mut in_hosts = false;
	let mut hosts_data: HashMap<String, HashMap<String, String>> = HashMap::new();

	for line in content.lines() {
		let line = line.trim();
		if line.starts_with('[') {
			in_hosts = line == "[hosts]";
			continue;
		}
		if !in_hosts {
			continue;
		}

		if let Some((key, value)) = line.split_once('=') {
			// Keys look like "1\hostname", "1\localaddress", "2\hostname", etc.
			if let Some((index, field)) = key.split_once('\\') {
				hosts_data
					.entry(index.to_string())
					.or_default()
					.insert(field.to_string(), value.to_string());
			}
		}
	}

	let mut hosts: Vec<KnownHost> = Vec::new();
	let mut indices: Vec<String> = hosts_data.keys().cloned().collect();
	indices.sort_by_key(|a| a.parse::<u32>().unwrap_or(0));

	for index in indices {
		let data = &hosts_data[&index];
		let name = match data.get("hostname") {
			Some(n) => n.clone(),
			None => continue,
		};

		// Prefer manual address (user-configured), then local address, then remote.
		let address = data
			.get("manualaddress")
			.filter(|a| !a.is_empty())
			.or_else(|| data.get("localaddress").filter(|a| !a.is_empty()))
			.or_else(|| data.get("remoteaddress").filter(|a| !a.is_empty()));

		if let Some(addr) = address {
			hosts.push(KnownHost {
				name: name.clone(),
				address: addr.clone(),
			});
		}
	}

	Ok(hosts)
}
