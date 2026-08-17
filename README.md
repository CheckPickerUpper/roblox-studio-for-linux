# Roblox Studio Linux Launcher

An unofficial, experimental launcher for attempting to run the Windows build of Roblox Studio on Linux through Wine.

Roblox officially supports Studio on Windows and macOS. This project does not provide a native Linux build, bypass Roblox authentication, or modify Roblox binaries. Studio updates may stop working until the compatibility code is adjusted.

## What this first version does

- Stores a per-user Wine prefix and launcher configuration.
- Checks whether Wine is available.
- Runs the official Windows Studio installer inside that prefix.
- Finds the newest installed `RobloxStudioBeta.exe` when possible.
- Launches Studio through Wine.
- Includes a desktop-entry template for a clickable launcher.

## Dependencies

Required system software:

- Linux
- Rust stable and Cargo
- Wine, installed through your Linux distribution
- The official Windows Roblox Studio installer

The launcher uses Rust's standard library and has no third-party Rust crate dependencies.

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

1. Download the official Windows Studio installer from Roblox.
2. Run the installer through the launcher:

   ```bash
   roblox-studio-linux-launcher install --installer ~/Downloads/RobloxStudio.exe
   ```

3. Check what was installed:

   ```bash
   roblox-studio-linux-launcher doctor
   ```

4. Try launching Studio:

   ```bash
   roblox-studio-linux-launcher launch
   ```

Rerunning `install` with a newer official installer is the update path. The default data directory is `~/.local/share/roblox-studio-linux-launcher`. Use `--config` to keep the configuration somewhere else.

## Reference implementation

This repository includes Vinegar as a pinned, source-only Git submodule at references/vinegar. It is the working reference for Roblox Studio installation, Wine setup, version discovery, and launching:

- references/vinegar/cmd/vinegar: command entry point
- references/vinegar/internal: configuration and platform behavior
- references/vinegar/layer: Wine and runtime layers

Vinegar is GPL-3.0 licensed. We study its behavior and reimplement the needed ideas in Rust; we do not compile or copy its code into this project.

## Desktop launcher

After installing the command with Cargo, copy the template into your user application menu:

```bash
mkdir -p ~/.local/share/applications
cp assets/roblox-studio-linux-launcher.desktop ~/.local/share/applications/
```

The entry opens a terminal so Wine errors remain visible while this project is experimental.

## Development checks

```bash
cargo fmt --all
cargo check --all-targets
cargo run -- --help
```

## Current limits

- Linux is not an officially supported Roblox platform.
- Wine compatibility can change after any Roblox Studio update.
- Plugins, graphics, login, and play-testing still need real Linux testing.
- Windows dual boot remains the reliable fallback for Studio work.
