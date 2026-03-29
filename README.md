# moonlight-steam-shortcuts

Automatically create Steam shortcuts for all your [Moonlight](https://moonlight-stream.org/) game streaming apps, complete with boxart and a Moonlight logo overlay.

Every game launch triggers a background sync, so your shortcuts stay up-to-date as you add or remove apps on your host.

## Features

- **Auto-detection** — finds Moonlight (PATH binary or Flatpak) and all known hosts automatically
- **Boxart with overlay** — installs cover art with a Moonlight logo badge in Steam's grid view
- **Background sync on launch** — each game launch refreshes shortcuts in the background with zero added latency
- **Multi-host** — sync apps from multiple Moonlight hosts into one Steam library
- **Idempotent** — run sync repeatedly without creating duplicates

## Quick Start (Steam Deck)

1. Install Moonlight via Flatpak and pair with your host(s) through the Moonlight GUI.
2. Open a terminal (Konsole) in Desktop Mode and run:

```sh
curl -sL https://raw.githubusercontent.com/hgaiser/moonlight-steam-shortcuts/main/scripts/install | sh
```

3. Restart Steam. Your Moonlight apps appear as shortcuts in the library.

## Usage

After install, the binary is at `~/.local/bin/moonlight-steam-shortcuts`.

### Sync shortcuts

Sync all known hosts (auto-detected from Moonlight's config):

```sh
moonlight-steam-shortcuts sync
```

Sync specific hosts:

```sh
moonlight-steam-shortcuts sync 192.168.1.10 192.168.1.20
```

Options:
- `--dry-run` — show what would change without modifying anything
- `--no-overlay` — skip the Moonlight logo overlay on boxart

### List shortcuts

```sh
moonlight-steam-shortcuts list
```

### Remove all shortcuts

```sh
moonlight-steam-shortcuts remove
```

### Launch a game

This is what each Steam shortcut invokes internally — you don't normally run this manually:

```sh
moonlight-steam-shortcuts launch <host> "<app>"
```

Options:
- `--no-sync` — skip the background sync before launching

### Global options

```
-m, --moonlight <PATH>        Path to Moonlight executable (auto-detected if omitted)
-s, --steam-userdata <PATH>   Path to Steam userdata directory (auto-detected if omitted)
    --flatpak                 Force Flatpak Moonlight (auto-detected if omitted)
-v, --verbose                 Enable verbose output
```

## How It Works

1. **Sync** queries each Moonlight host for its app list via `moonlight list --csv`.
2. For each app, it creates a Steam non-Steam shortcut tagged `"moonlight"` where the `exe` points back to `moonlight-steam-shortcuts launch`.
3. Boxart from Moonlight's local cache is composited with a small Moonlight logo and installed as a Steam grid cover image (`<app_id>p.png`).
4. When a game is launched from Steam, the `launch` subcommand forks a child process for background sync, then immediately `exec()`s into `moonlight stream` — so the game starts with zero delay.
5. Stale shortcuts (apps removed from the host) are automatically cleaned up during sync.

## Building

```sh
cargo build --release
```

For a fully static binary (recommended for Steam Deck):

```sh
cargo build --release --target x86_64-unknown-linux-musl
```

## License

See [LICENSE](LICENSE).
