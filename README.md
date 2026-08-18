# Roblox Studio Linux Launcher

An unofficial, experimental launcher for attempting to run the Windows build of Roblox Studio on Linux through Wine.

Roblox officially supports Studio on Windows and macOS. This project does not provide a native Linux build, bypass Roblox authentication, or modify Roblox binaries. Studio updates may stop working until the compatibility code is adjusted.

## What this first version does

- Stores a per-user Wine prefix and launcher configuration.
- Checks whether Wine is available.
- Installs the current official Windows Studio deployment directly into that prefix.
- Keeps the official bootstrapper available as an explicit `--installer` fallback.
- Finds the newest installed `RobloxStudioBeta.exe` on every launch.
- Configures the Wine prefix for Studio and installs its WebView2 runtime when needed.
- Registers the `roblox-studio-auth:` browser callback with the Linux desktop.
- Launches Studio through Wine and forwards Studio command-line arguments.
- Includes a desktop-entry template for a clickable launcher.

## Dependencies

Required system software:

- Linux
- Rust stable and Cargo
- Wine, installed through your Linux distribution
- Winetricks with the `corefonts` and `vcrun2019` verbs available

The launcher downloads and verifies the current Studio packages from Roblox's official deployment endpoints. It does not require a manually downloaded installer for the normal path.

## Install from a checkout

Install the launcher command:

```bash
cargo install --path .
roblox-studio-linux-launcher doctor
```

During development, run it without installing:

```bash
cargo run -- doctor
```

## First run

1. Install the Windows compatibility components into the launcher's prefix:

   ```bash
   WINEPREFIX="$HOME/.local/share/roblox-studio-linux-launcher/wine" \
     winetricks --unattended corefonts vcrun2019
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

Rerun `install` to check for and install a newer Studio deployment. Normal launches discover the newest installed Studio executable instead of pinning launches to an older version directory. The default data directory is `~/.local/share/roblox-studio-linux-launcher`. Use `--config` to keep the configuration somewhere else.

If Roblox changes the direct deployment service, a manually downloaded bootstrapper can still be run explicitly:

```bash
roblox-studio-linux-launcher install --installer ~/Downloads/RobloxStudioLauncherBeta.exe
```

If Studio is installed outside the launcher's Wine prefix, configure it as a fallback:

```bash
roblox-studio-linux-launcher configure --studio-executable /path/to/RobloxStudioBeta.exe
```

Additional arguments after `launch` are passed to Studio.

## Browser login

Studio starts login in the external browser when Wine's embedded WebView2 renderer cannot load Roblox's login page. The browser returns through the `roblox-studio-auth:` URI, which the launcher's registered desktop entry forwards to Studio.

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

## Current limits

- Linux is not an officially supported Roblox platform.
- Wine compatibility can change after any Roblox Studio update.
- Embedded WebView2 login may fail under Wine; use the external browser callback.
- Browser callback delivery depends on the Linux desktop handler. Windows Chrome cannot invoke a WSL `.desktop` entry.
- Plugins, graphics, and play-testing still need real Linux testing.
- Windows dual boot remains the reliable fallback for Studio work.
