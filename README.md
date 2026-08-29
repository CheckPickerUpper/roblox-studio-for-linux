# Roblox Studio Linux Launcher

An unofficial, experimental launcher for attempting to run the Windows build of Roblox Studio on Linux through Wine.

Roblox officially supports Studio on Windows and macOS. This project does not provide a native Linux build, bypass Roblox authentication, or modify Roblox binaries. Studio updates may stop working until the compatibility code is adjusted.

## What this first version does

- Stores a per-user Wine prefix and launcher configuration.
- Checks whether Wine is available.
- Installs the current official Windows Studio deployment directly into that prefix.
- Keeps the official bootstrapper available as an explicit `--installer` fallback.
- Finds the newest installed `RobloxStudioBeta.exe` on every launch.
- Configures the Wine prefix for Studio, installs WebView2 when needed, and repairs the
  WebView2 registry entry that Wine's installer can leave missing.
- Registers the `roblox-studio-auth:` browser callback with the Linux desktop.
- Launches Studio through Wine and forwards Studio command-line arguments.
- Connects AI clients to Roblox Studio's built-in MCP process without replacing it.
- Verifies the matching Studio version, MCP executable, Wine prefix, and live Studio session.
- Includes a desktop-entry template for a clickable launcher.
- Includes a graphical launcher with install, update, launch, MCP connection checks, diagnostics, and settings actions.

## Dependencies

Required system software:

- Linux
- Rust stable and Cargo
- Wine, installed through your Linux distribution
- Winetricks with the `corefonts`, `vcrun2019`, and `dxvk` verbs available

These dependencies describe the native Cargo installation. The Flatpak build
contains one managed Kombucha Wine runtime and keeps its prefix inside the
Flatpak data directory. It does not also include the `org.winehq.Wine` base.

The launcher downloads and verifies the current Studio packages from Roblox's official deployment endpoints. It does not require a manually downloaded installer for the normal path.

## Install from a checkout

Install the launcher command:

```bash
cargo install --path .
roblox-studio-linux-launcher doctor
```

Open the graphical launcher with:

```bash
roblox-studio-linux-launcher gui
```

During development, run it without installing:

```bash
cargo run -- doctor
```

## First run

1. Install the Windows compatibility components into the launcher's prefix:

   ```bash
   WINEPREFIX="$HOME/.local/share/roblox-studio-linux-launcher/wine" \
     winetricks --unattended corefonts vcrun2019 dxvk
   ```

2. Install the current Studio deployment directly:

   ```bash
   roblox-studio-linux-launcher install
   ```
3. Register the browser callback (the install command also does this):

   ```bash
   roblox-studio-linux-launcher register
   ```

4. Check what was installed:

   ```bash
   roblox-studio-linux-launcher doctor
   ```

5. Try launching Studio:

   ```bash
   roblox-studio-linux-launcher launch
   ```

The launcher uses Studio's own sign-in window by default. Its managed WebView2
setup uses WebView2's built-in SwiftShader renderer instead of Wine's hanging
D3D11 software path. If that window still cannot render on a particular system,
use the Linux-browser backup with `roblox-studio-linux-launcher browser-login`,
or save that choice with `roblox-studio-linux-launcher configure --browser-login`.

On a Wayland desktop with XWayland available, Wine keeps its mature X11 window
driver. The native Wine Wayland path currently lacks desktop icon and clipboard
features and did not prevent Studio's plugin-window swapchain failure in live
testing. The launcher saves that driver order in the Wine prefix before Studio
starts and restarts a stale Wine session once when the saved choice changes.

Rerun `install` to check for and install a newer Studio deployment. Normal launches discover the newest installed Studio executable instead of pinning launches to an older version directory. The default data directory is `~/.local/share/roblox-studio-linux-launcher`. Use `--config` to keep the configuration somewhere else.

## Connect an AI client through Studio MCP

Roblox Studio already contains the MCP server. This launcher only supplies the
Linux/Wine process bridge that AI clients need; it does not create a replacement
server or use the old standalone MCP project.

1. Launch Studio and open a place.
2. In Studio, open `Assistant` → `…` → `Manage MCP Servers` and enable
   `Enable Studio as MCP server`.
3. Add the launcher to an MCP client's JSON configuration. To merge it into an
   existing JSON file while preserving other servers and creating a backup:

   ```bash
   roblox-studio-linux-launcher mcp setup \
     --client-config ~/.config/your-client/mcp.json
   ```

   To print a configuration without editing a file:

   ```bash
   roblox-studio-linux-launcher mcp setup --print
   ```

4. Restart the AI client, then verify the live connection:

   ```bash
   roblox-studio-linux-launcher mcp doctor
   ```

`mcp doctor` checks `list_roblox_studios`, `get_studio_state`, and
`search_game_tree`. It distinguishes a missing Wine installation, missing
Studio/MCP files, Studio not running, Studio waiting for sign-in, Studio
running without MCP enabled, multiple Studio sessions, and a verified
connection. `mcp serve` is the command
an AI client invokes; it passes stdin/stdout directly to the exact
`StudioMCP.exe` beside the selected `RobloxStudioBeta.exe`, with diagnostics on
stderr so protocol output stays clean.

If Roblox changes the direct deployment service, a manually downloaded bootstrapper can still be run explicitly:

```bash
roblox-studio-linux-launcher install --installer ~/Downloads/RobloxStudioLauncherBeta.exe
```

If Studio is installed outside the launcher's Wine prefix, you can configure it as a launch-only fallback:

```bash
roblox-studio-linux-launcher configure --studio-executable /path/to/RobloxStudioBeta.exe
```

The MCP commands do not use that outside-prefix fallback. They only connect to
the `StudioMCP.exe` beside the selected Studio version in the configured prefix,
so Studio and MCP cannot accidentally come from different installations.

Additional arguments after `launch` are passed to Studio.

## Studio login

The launcher installs the matching WebView2 runtime and uses Studio's own sign-in
window. Browser mode is the backup: it opens Studio's one-time authorization URL
in the Linux browser, then returns through the `roblox-studio-auth:` URI. The
launcher's registered desktop entry forwards that callback to the already-running
Flatpak Studio sandbox. Browser mode verifies that handler and waits until the
authorization page actually opens before reporting success.

Use `configure --embedded-webview` to return to the normal in-Studio page after
testing browser mode.

On WSL, use a Linux browser inside WSL for this callback path. A Windows browser uses Windows' protocol registry and cannot invoke the WSL desktop entry.

If the browser was already open before registration, restart it once so it reloads the desktop application database.

## Versioning

`Cargo.toml` is the source of truth for the launcher version. Versions follow SemVer 2.0, including optional prerelease and build metadata identifiers.

Release tags use the matching `v`-prefixed version, for example `v0.1.0` for package version `0.1.0`. CI checks both the package version and the exact tag match, so a release tag cannot point at a different version than the build metadata.

## Reference implementation

This repository includes Vinegar as a pinned, source-only Git submodule at references/vinegar. It is the working reference for Roblox Studio installation, Wine setup, version discovery, and launching:

- references/vinegar/cmd/vinegar: command entry point
- references/vinegar/internal: configuration and platform behavior
- references/vinegar/layer: Wine and runtime layers

Vinegar is GPL-3.0 licensed. We study its behavior and reimplement the needed ideas in Rust; we do not compile or copy its code into this project.

## Documentation and resources

Roblox:

- [Studio setup](https://create.roblox.com/docs/studio/setup): official supported platforms and system requirements.
- [Studio MCP](https://create.roblox.com/docs/studio/mcp): official built-in MCP server, tools, Studio toggle, and client setup.
- [Studio command-line interface](https://create.roblox.com/docs/studio/command-line-interface): official launch arguments and executable locations.

Linux and Wine:

- [Vinegar installation guide](https://vinegarhq.org/Vinegar/Installation.html): Linux requirements and installation options.
- [Vinegar FAQ](https://vinegarhq.org/Vinegar/FAQ/index.html): compatibility, rendering, and configuration guidance.
- [Vinegar troubleshooting](https://vinegarhq.org/Vinegar/Troubleshooting.html): common login, graphics, prefix, and desktop-environment issues.
- [WineHQ help](https://www.winehq.org/help): Wine documentation, FAQ, wiki, and application support resources.

Rust:

- [The Rust Programming Language](https://doc.rust-lang.org/book/): language guide for contributors learning Rust.
- [The Cargo Book](https://doc.rust-lang.org/cargo/): build, run, and package this launcher.

## Desktop launcher

The GUI desktop entry is available at
`assets/io.github.checkpickerupper.RobloxStudioLinuxLauncher.desktop`.
The existing registered desktop entry remains dedicated to the `roblox-studio-auth:`
browser callback, so browser login continues to work while the GUI is open from the
desktop menu.

`install` and `launch` register the browser callback automatically. To register it without launching Studio:

```bash
roblox-studio-linux-launcher register
```

Registration writes a per-user desktop entry and refreshes the user MIME cache for `roblox-studio-auth:`. The committed file at `assets/roblox-studio-linux-launcher.desktop` is a template for desktop-menu integration; the generated entry contains the installed launcher's absolute path.

## Development checks

```bash
cargo fmt --all
cargo check --all-targets
cargo run -- --help
```

## Flatpak

Install the published app from its signed Flatpak repository:

```bash
flatpak remote-add --user --if-not-exists roblox-studio-linux-launcher \
  https://checkpickerupper.github.io/roblox-studio-for-linux/RobloxStudioLinuxLauncher.flatpakrepo
flatpak install --user roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher
```

After installation, normal `flatpak update` commands deliver launcher updates.
The one-click installer is
[RobloxStudioLinuxLauncher.flatpakref](https://checkpickerupper.github.io/roblox-studio-for-linux/RobloxStudioLinuxLauncher.flatpakref),
and the release page retains the standalone `.flatpak` bundle as a fallback.

The manifest at
`flatpak/io.github.checkpickerupper.RobloxStudioLinuxLauncher.yml` bundles one
managed Kombucha Wine build and DXVK graphics layer, so Studio and
`StudioMCP.exe` run inside the same sandbox and prefix. See `flatpak/README.md`
for source builds and MCP invocation. For Flatpak MCP, keep the launcher GUI open
while Studio and the AI client are connected; the external command enters that
running app sandbox so the official MCP process can see the open Studio place.

## Current limits

- Linux is not an officially supported Roblox platform.
- Wine compatibility can change after any Roblox Studio update.
- Embedded WebView2 login depends on the current Wine/Studio build. The managed
  compatibility settings use WebView2's SwiftShader renderer to avoid Wine's
  hanging D3D11 WARP path; browser sign-in remains available as a backup.
- Browser callback delivery depends on the Linux desktop handler. Windows Chrome cannot invoke a WSL `.desktop` entry.
- Roblox Studio plugin windows depend on Wine, Vulkan, and display-driver
  compatibility. The launcher GUI can use native Wayland, while Studio prefers
  Wine's X11 driver through XWayland and uses native Wine Wayland only when X11
  is unavailable.
- Windows dual boot remains the reliable fallback for Studio work.
