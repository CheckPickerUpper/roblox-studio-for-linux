# Roblox Studio Linux Launcher

An unofficial, experimental launcher for attempting to run the Windows build of Roblox Studio on Linux through Wine.

Roblox officially supports Studio on Windows and macOS. This project does not provide a native Linux build, bypass Roblox authentication, or modify Roblox binaries. Studio updates may stop working until the compatibility code is adjusted.

## What this first version does

- Stores a per-user Wine prefix and launcher configuration.
- Checks whether Wine is available.
- Installs the current official Windows Studio deployment directly into that prefix.
- Keeps the official bootstrapper available as an explicit `--installer` fallback.
- Checks Roblox's current deployment on every launch and installs it before
  Studio starts when the local prefix is missing that version.
- Configures the Wine prefix for Studio, installs WebView2 when needed, and repairs the
  WebView2 registry entry that Wine's installer can leave missing.
- Registers the `roblox-studio-auth:` browser callback with the Linux desktop.
- Launches Studio through Wine and forwards Studio command-line arguments.
- Connects AI clients to Roblox Studio's built-in MCP process without replacing it.
- Verifies the matching Studio version, MCP executable, Wine prefix, and live Studio session.
- Includes a desktop entry for the Flatpak app.
- Includes a graphical launcher with install, update, launch, MCP connection checks, diagnostics, and settings actions.

## Official installation

The signed Flatpak repository is this project's official and only supported
end-user installation. It includes the graphical launcher, the launcher's CLI,
one managed Wine runtime, and the update path for the app and its compatibility
components.

Install it from the published repository:

```bash
flatpak remote-add --user --if-not-exists roblox-studio-linux-launcher \
  https://checkpickerupper.github.io/roblox-studio-for-linux/RobloxStudioLinuxLauncher.flatpakrepo
flatpak install --user roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher
flatpak run io.github.checkpickerupper.RobloxStudioLinuxLauncher
```

The one-click installer is
[RobloxStudioLinuxLauncher.flatpakref](https://checkpickerupper.github.io/roblox-studio-for-linux/RobloxStudioLinuxLauncher.flatpakref).
Update the installed app with:

```bash
flatpak update --user io.github.checkpickerupper.RobloxStudioLinuxLauncher
```

Do not install the launcher with `cargo install`, host Wine, or host
Winetricks. Those commands are contributor/development tools, not another
supported installation path.

## First run

1. Start the installed Flatpak from your desktop menu or with:

   ```bash
   flatpak run io.github.checkpickerupper.RobloxStudioLinuxLauncher
   ```

2. Use **Install / update** in the GUI. The Flatpak manages the Wine
   prefix and compatibility components for you.

3. Sign in through Studio and use **Launch Studio**. Normal launches check
   Roblox's current deployment before opening Studio.

The launcher uses Studio's own sign-in window by default. Its managed WebView2
setup uses WebView2's built-in SwiftShader renderer instead of Wine's hanging
D3D11 software path. If that window still cannot render on a particular system,
use the Linux-browser backup with:

```bash
flatpak run --command=roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher browser-login
```

or save that choice with:

```bash
flatpak run --command=roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher configure --browser-login
```

On a Wayland desktop with XWayland available, Wine keeps its mature X11 window
driver. The native Wine Wayland path currently lacks desktop icon and clipboard
features and did not prevent Studio's plugin-window swapchain failure in live
testing. The launcher saves that driver order in the Wine prefix before Studio
starts and restarts a stale Wine session once when the saved choice changes.

Normal launches check Roblox's deployment endpoint and update the managed
version directory automatically before Studio starts. The Flatpak's Wine
prefix, including Studio sign-in data, is reused across versions. If the
deployment endpoint or package downloads are unavailable, launch fails with
the reason instead of opening an outdated Studio build. The Flatpak keeps its
data under `~/.var/app/io.github.checkpickerupper.RobloxStudioLinuxLauncher/`.
Advanced CLI commands are documented in [Flatpak packaging](flatpak/README.md).

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
   flatpak run --command=roblox-studio-linux-launcher \
     io.github.checkpickerupper.RobloxStudioLinuxLauncher \
     mcp setup --client-config ~/.config/your-client/mcp.json
   ```

   To print a configuration without editing a file:

   ```bash
   flatpak run --command=roblox-studio-linux-launcher \
     io.github.checkpickerupper.RobloxStudioLinuxLauncher \
     mcp setup --print
   ```

4. Restart the AI client, then verify the live connection:

   ```bash
   flatpak run --command=roblox-studio-linux-launcher \
     io.github.checkpickerupper.RobloxStudioLinuxLauncher \
     mcp doctor
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
flatpak run --command=roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher \
  install --installer ~/Downloads/RobloxStudioLauncherBeta.exe
```

If Studio is installed outside the launcher's Wine prefix, you can configure it as a launch-only fallback:

```bash
flatpak run --command=roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher \
  configure --studio-executable /path/to/RobloxStudioBeta.exe
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

Use this Flatpak command to return to the normal in-Studio page after testing
browser mode:

```bash
flatpak run --command=roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher configure --embedded-webview
```

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

The Flatpak installs the GUI desktop entry and registers the
`roblox-studio-auth:` browser callback for the managed Studio sandbox.
`Install / update` and `Launch Studio` register the callback
automatically. To register it without launching Studio:

```bash
flatpak run --command=roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher register
```

Registration writes a per-user desktop entry and refreshes the user MIME cache
for `roblox-studio-auth:`. There is no separate native desktop installation.

## Contributor development

The Rust binary is an implementation detail bundled into the Flatpak. Source
checkout commands are for contributors and debugging; they are not a supported
end-user installation and do not provide the managed Flatpak runtime.

```bash
cargo fmt --all
cargo check --all-targets
cargo run -- --help
```

Build and exercise the release-shaped package with the instructions in
[flatpak/README.md](flatpak/README.md). Do not publish or distribute a native
`cargo install` build as an alternative launcher.

## Flatpak package

The published release page retains the standalone `.flatpak` bundle as a
fallback for the same official Flatpak installation. It does not publish a
native executable or native desktop package.

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
