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
- Python 3.11 or newer
- Wine, installed through your Linux distribution
- The official Windows Roblox Studio installer

There are no third-party Python runtime dependencies. `uv` is optional, but recommended for installing the project as a command.

## Install from a checkout

```bash
uv tool install .
roblox-studio-linux-launcher doctor
```

Without `uv`, run it directly from the checkout:

```bash
python3 -m roblox_studio_launcher doctor
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

The default data directory is `~/.local/share/roblox-studio-linux-launcher`. Use `--config` to keep the configuration somewhere else.

## Desktop launcher

After installing the command with `uv`, copy the template into your user application menu:

```bash
mkdir -p ~/.local/share/applications
cp assets/roblox-studio-linux-launcher.desktop ~/.local/share/applications/
```

The entry opens a terminal so Wine errors remain visible while this project is experimental.

## Current limits

- Linux is not an officially supported Roblox platform.
- Wine compatibility can change after any Roblox Studio update.
- Plugins, graphics, login, and play-testing still need real Linux testing.
- Windows dual boot remains the reliable fallback for Studio work.
