mod boxart;
mod moonlight;
mod steam;
mod steamcdn;
mod steamstore;

use clap::{Parser, Subcommand};
use std::{io::Write, path::PathBuf, process::Command};
use steam_shortcuts_util::Shortcut;

struct SyncOptions {
	dry_run: bool,
	no_overlay: bool,
	force_download_images: bool,
	/// App name prefixes (case-insensitive) for which image downloading is skipped.
	skip_images: Vec<String>,
	no_sync_shortcut: bool,
	verbose: bool,
}

#[derive(Parser)]
#[clap(version)]
struct Cli {
	/// Path to the Moonlight executable.
	#[clap(short, long, global = true)]
	moonlight: Option<PathBuf>,

	/// Path to the Steam userdata directory.
	#[clap(short, long, global = true)]
	steam_userdata: Option<PathBuf>,

	/// Use Flatpak Moonlight (com.moonlight_stream.Moonlight).
	#[clap(long, global = true)]
	flatpak: bool,

	/// Do not add a "Sync Moonlight Shortcuts" shortcut to Steam.
	#[clap(long, global = true)]
	no_sync_shortcut: bool,

	/// Enable verbose output.
	#[clap(short, long, global = true)]
	verbose: bool,

	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand)]
enum Commands {
	/// Sync Moonlight apps to Steam shortcuts (add new, remove stale).
	Sync {
		/// Moonlight host addresses to sync. If omitted, all known hosts from
		/// Moonlight's config are used.
		hosts: Vec<String>,

		/// Show what would change without modifying anything.
		#[clap(long)]
		dry_run: bool,

		/// Skip adding the Moonlight logo overlay to boxart.
		#[clap(long)]
		no_overlay: bool,

		/// Re-download and overwrite grid/hero images even if they already exist.
		#[clap(long)]
		force_download_images: bool,

		/// Skip image downloads for apps whose names start with this prefix (case-insensitive).
		/// Can be specified multiple times.
		#[clap(long)]
		skip_images: Vec<String>,
	},
	/// Remove all Moonlight-managed shortcuts and grid images.
	Remove {
		/// Show what would be removed without modifying anything.
		#[clap(long)]
		dry_run: bool,
	},
	/// List currently managed Moonlight shortcuts in Steam.
	List,
	/// Launch a game via Moonlight, triggering a background sync first.
	Launch {
		/// Moonlight host address.
		host: String,

		/// Application name to launch.
		app: String,

		/// Skip the background sync before launching.
		#[clap(long)]
		no_sync: bool,
	},
}

fn main() -> Result<(), String> {
	let cli = Cli::parse();
	let backend = moonlight::resolve_backend(cli.moonlight.as_deref(), cli.flatpak)?;

	match cli.command {
		Commands::Sync {
			hosts,
			dry_run,
			no_overlay,
			force_download_images,
			skip_images,
		} => cmd_sync(&backend, &hosts, cli.steam_userdata.as_deref(), &SyncOptions {
			dry_run,
			no_overlay,
			force_download_images,
			skip_images,
			no_sync_shortcut: cli.no_sync_shortcut,
			verbose: cli.verbose,
		}),
		Commands::Remove { dry_run } => cmd_remove(cli.steam_userdata.as_deref(), dry_run, cli.verbose),
		Commands::List => cmd_list(cli.steam_userdata.as_deref()),
		Commands::Launch { host, app, no_sync } => cmd_launch(
			&backend,
			&host,
			&app,
			no_sync,
			cli.steam_userdata.as_deref(),
			cli.verbose,
		),
	}
}

fn cmd_sync(
	backend: &moonlight::MoonlightBackend,
	hosts: &[String],
	steam_userdata: Option<&std::path::Path>,
	opts: &SyncOptions,
) -> Result<(), String> {
	// If no hosts specified, auto-discover from Moonlight's config.
	let hosts: Vec<String> = if hosts.is_empty() {
		let known = moonlight::known_hosts(backend);
		if known.is_empty() {
			return Err("No hosts specified and no known hosts found in Moonlight config. \
				 Provide host addresses as arguments or pair with a host in Moonlight first."
				.to_string());
		}
		println!(
			"Auto-detected {} known host(s): {}",
			known.len(),
			known
				.iter()
				.map(|h| format!("{} ({})", h.name, h.address))
				.collect::<Vec<_>>()
				.join(", ")
		);
		known.into_iter().map(|h| h.name).collect()
	} else {
		hosts.to_vec()
	};

	let user_dir = steam::find_user_dir(steam_userdata)?;
	let existing = steam::load_shortcuts(&user_dir)?;

	let non_moonlight: Vec<_> = existing
		.iter()
		.filter(|s| !s.tags.contains(&"moonlight".to_string()))
		.cloned()
		.collect();
	let moonlight_existing: Vec<_> = existing
		.iter()
		.filter(|s| s.tags.contains(&"moonlight".to_string()))
		.cloned()
		.collect();

	// Collect desired apps from all hosts.
	let mut desired: Vec<(String, moonlight::MoonlightApp)> = Vec::new();
	for host in &hosts {
		println!("Retrieving apps from '{host}' ...");
		match moonlight::list_apps(backend, host) {
			Ok(apps) => {
				println!("Found {} apps on '{host}'.", apps.len());
				for app in apps {
					desired.push((host.clone(), app));
				}
			},
			Err(e) => {
				eprintln!("Warning: failed to list apps from '{host}': {e}");
			},
		}
	}

	// Determine the exe path (our own binary).
	let self_path = std::env::current_exe()
		.map_err(|e| format!("Failed to determine own executable path: {e}"))?
		.to_string_lossy()
		.to_string();

	// Build set of desired launch_options for identity matching.
	let mut desired_launch_opts: std::collections::HashSet<String> = desired
		.iter()
		.map(|(host, app)| build_launch_options(backend, steam_userdata, host, &app.name))
		.collect();

	// Always keep the sync shortcut if present.
	if !opts.no_sync_shortcut {
		desired_launch_opts.insert(build_sync_launch_options(backend, steam_userdata, &opts.skip_images));
	}

	// Find stale moonlight shortcuts (not in desired set).
	let stale: Vec<_> = moonlight_existing
		.iter()
		.filter(|s| !desired_launch_opts.contains(&s.launch_options))
		.collect();

	if !stale.is_empty() {
		println!("Removing {} stale shortcuts.", stale.len());
		for s in &stale {
			if opts.verbose {
				println!("  - {}", s.app_name);
			}
			if !opts.dry_run {
				steam::remove_grid_images(&user_dir, s.app_id)?;
			}
		}
	}

	// Build new shortcut list.
	let fetch_images = !opts.dry_run;
	let mut new_shortcuts = Vec::new();
	if fetch_images {
		println!("Fetching grid/hero images for {} app(s)...", desired.len());
	}
	for (i, (host, app)) in desired.iter().enumerate() {
		let launch_options = build_launch_options(backend, steam_userdata, host, &app.name);

		// Check if shortcut already exists.
		let existing_shortcut = moonlight_existing.iter().find(|s| s.launch_options == launch_options);

		let display_name = format!("{} 🌙", app.name);
		let shortcut = if let Some(existing) = existing_shortcut {
			let mut s = existing.clone();
			s.app_name = display_name.clone();
			s
		} else {
			let mut s = Shortcut::new("", &display_name, &self_path, "", "", "", &launch_options).to_owned();
			s.tags.push("moonlight".to_string());
			if opts.verbose {
				println!("  + {} (host: {host})", app.name);
			}
			s
		};

		// Determine which image slots need updating.
		let need_portrait =
			!opts.dry_run && (opts.force_download_images || !steam::grid_image_exists(&user_dir, shortcut.app_id));
		let need_wide =
			fetch_images && (opts.force_download_images || !steam::wide_grid_image_exists(&user_dir, shortcut.app_id));
		let need_hero =
			fetch_images && (opts.force_download_images || !steam::hero_image_exists(&user_dir, shortcut.app_id));

		if need_portrait || need_wide || need_hero {
			// Look up Steam app ID once (used for CDN portrait fallback, wide grid, and hero).
			let skip_images = opts
				.skip_images
				.iter()
				.any(|prefix| app.name.to_lowercase().starts_with(&prefix.to_lowercase()));
			let steam_app_id = if skip_images || (!need_wide && !need_hero && app.boxart_path.is_some()) {
				None
			} else {
				steamstore::find_app_id(&app.name)
			};

			// Install portrait boxart: prefer Moonlight's local cache, fall back to Steam CDN.
			if need_portrait {
				let portrait_data = match boxart::process_boxart(app.boxart_path.as_deref(), opts.no_overlay) {
					Ok(Some(data)) => Some(data),
					Ok(None) => steam_app_id.and_then(steamcdn::fetch_portrait).map(|d| {
						if opts.no_overlay {
							d
						} else {
							boxart::apply_overlay_to_bytes(d)
						}
					}),
					Err(e) => {
						eprintln!("Warning: boxart processing failed for '{}': {e}", app.name);
						None
					},
				};
				if let Some(data) = portrait_data {
					steam::install_grid_image(&user_dir, shortcut.app_id, &data)?;
				}
			}

			if need_wide || need_hero {
				print!("  [{}/{}] '{}':", i + 1, desired.len(), app.name);
				let _ = std::io::stdout().flush();

				// Wide grid.
				if need_wide {
					print!(" wide grid");
					let _ = std::io::stdout().flush();
					let wide_data = steam_app_id.and_then(steamcdn::fetch_wide_grid).map(|d| {
						if opts.no_overlay {
							d
						} else {
							boxart::apply_overlay_to_bytes(d)
						}
					});
					match wide_data {
						Some(data) => match steam::install_wide_grid_image(&user_dir, shortcut.app_id, &data) {
							Ok(()) => print!(" ok,"),
							Err(e) => print!(" install failed ({e}),"),
						},
						None => print!(" not found,"),
					}
					let _ = std::io::stdout().flush();
				}

				// Hero image.
				if need_hero {
					print!(" hero");
					let _ = std::io::stdout().flush();
					let hero_data = steam_app_id.and_then(steamcdn::fetch_hero).map(|d| {
						if opts.no_overlay {
							d
						} else {
							boxart::apply_overlay_to_bytes(d)
						}
					});
					match hero_data {
						Some(data) => match steam::install_hero_image(&user_dir, shortcut.app_id, &data) {
							Ok(()) => print!(" ok"),
							Err(e) => print!(" install failed ({e})"),
						},
						None => print!(" not found"),
					}
					let _ = std::io::stdout().flush();
				}

				println!();
			}
		}

		new_shortcuts.push(shortcut);
	}

	// Create or update the sync shortcut.
	if !opts.no_sync_shortcut {
		let sync_opts = build_sync_launch_options(backend, steam_userdata, &opts.skip_images);
		let sync_shortcut = moonlight_existing
			.iter()
			.find(|s| s.launch_options == sync_opts)
			.cloned()
			.unwrap_or_else(|| {
				let mut s =
					Shortcut::new("", "Sync Moonlight Shortcuts 🌙", &self_path, "", "", "", &sync_opts).to_owned();
				s.tags.push("moonlight".to_string());
				s
			});
		new_shortcuts.push(sync_shortcut);
	}

	if opts.dry_run {
		println!(
			"Dry run: would write {} shortcuts ({} moonlight).",
			non_moonlight.len() + new_shortcuts.len(),
			new_shortcuts.len()
		);
	} else {
		let mut final_shortcuts = non_moonlight;
		final_shortcuts.extend(new_shortcuts);
		steam::save_shortcuts(&user_dir, &final_shortcuts)?;
		let ml_count = final_shortcuts
			.iter()
			.filter(|s| s.tags.contains(&"moonlight".to_string()))
			.count();
		println!("Saved {} shortcuts ({} moonlight).", final_shortcuts.len(), ml_count);
	}

	Ok(())
}

fn cmd_remove(steam_userdata: Option<&std::path::Path>, dry_run: bool, verbose: bool) -> Result<(), String> {
	let user_dir = steam::find_user_dir(steam_userdata)?;
	let existing = steam::load_shortcuts(&user_dir)?;
	let moonlight = steam::moonlight_shortcuts(&existing);

	if moonlight.is_empty() {
		println!("No Moonlight shortcuts to remove.");
		return Ok(());
	}

	println!("Removing {} Moonlight shortcuts.", moonlight.len());
	for s in &moonlight {
		if verbose {
			println!("  - {}", s.app_name);
		}
		if !dry_run {
			steam::remove_grid_images(&user_dir, s.app_id)?;
		}
	}

	if !dry_run {
		let remaining: Vec<_> = existing
			.into_iter()
			.filter(|s| !s.tags.contains(&"moonlight".to_string()))
			.collect();
		steam::save_shortcuts(&user_dir, &remaining)?;
	}

	Ok(())
}

fn cmd_list(steam_userdata: Option<&std::path::Path>) -> Result<(), String> {
	let user_dir = steam::find_user_dir(steam_userdata)?;
	let existing = steam::load_shortcuts(&user_dir)?;
	let moonlight = steam::moonlight_shortcuts(&existing);

	if moonlight.is_empty() {
		println!("No Moonlight shortcuts found.");
		return Ok(());
	}

	println!("{:<40} {:>10}  Launch Options", "Name", "App ID");
	println!("{}", "-".repeat(80));
	for s in &moonlight {
		println!("{:<40} {:>10}  {}", s.app_name, s.app_id, s.launch_options);
	}

	Ok(())
}

fn cmd_launch(
	backend: &moonlight::MoonlightBackend,
	host: &str,
	app: &str,
	no_sync: bool,
	steam_userdata: Option<&std::path::Path>,
	verbose: bool,
) -> Result<(), String> {
	// Fork a background sync if enabled.
	if !no_sync {
		match libc_fork() {
			ForkResult::Child => {
				// Child process: run sync silently, then exit.
				let hosts = vec![host.to_string()];
				let _ = cmd_sync(backend, &hosts, steam_userdata, &SyncOptions {
					dry_run: false,
					no_overlay: false,
					force_download_images: false,
					skip_images: Vec::new(),
					no_sync_shortcut: true,
					verbose,
				});
				std::process::exit(0);
			},
			ForkResult::Parent => {
				// Parent continues to launch the game.
			},
			ForkResult::Error(e) => {
				eprintln!("Warning: fork failed ({e}), skipping background sync.");
			},
		}
	}

	// Exec into moonlight stream.
	let mut cmd = moonlight::stream_command(backend, host, app);
	let err = exec_command(&mut cmd);
	Err(format!("Failed to exec Moonlight: {err}"))
}

/// Build the launch_options string for the self-sync shortcut.
fn build_sync_launch_options(
	backend: &moonlight::MoonlightBackend,
	steam_userdata: Option<&std::path::Path>,
	skip_images: &[String],
) -> String {
	let mut parts = vec!["sync".to_string()];
	parts.push(backend.launch_flags());
	if let Some(path) = steam_userdata {
		parts.push(format!("-s {}", path.display()));
	}
	for name in skip_images {
		parts.push(format!("--skip-images \"{name}\""));
	}
	parts.join(" ")
}

/// Build the launch_options string for a shortcut.
fn build_launch_options(
	backend: &moonlight::MoonlightBackend,
	steam_userdata: Option<&std::path::Path>,
	host: &str,
	app_name: &str,
) -> String {
	let mut parts = vec!["launch".to_string()];
	parts.push(backend.launch_flags());
	if let Some(path) = steam_userdata {
		parts.push(format!("-s {}", path.display()));
	}
	parts.push(host.to_string());
	parts.push(format!("\"{}\"", app_name));
	parts.join(" ")
}

enum ForkResult {
	Child,
	Parent,
	Error(std::io::Error),
}

/// Fork the current process using libc.
///
/// # Safety
/// This calls libc::fork() which is unsafe. We only use it to spawn a simple
/// background sync that does not share mutable state with the parent.
fn libc_fork() -> ForkResult {
	// SAFETY: We call fork() at a point where only the main thread is running
	// and the child immediately performs independent work then exits.
	let pid = unsafe { libc::fork() };
	match pid {
		-1 => ForkResult::Error(std::io::Error::last_os_error()),
		0 => ForkResult::Child,
		_ => ForkResult::Parent,
	}
}

/// Replace the current process with the given command (unix exec).
fn exec_command(cmd: &mut Command) -> std::io::Error {
	use std::os::unix::process::CommandExt;
	cmd.exec()
}
