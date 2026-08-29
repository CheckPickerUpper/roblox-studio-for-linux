# Flatpak packaging

The manifest in this directory packages the graphical launcher as
`io.github.checkpickerupper.RobloxStudioLinuxLauncher`:

## Install a published release

Download the `.flatpak` file from the project's GitHub Releases page, then run:

```bash
flatpak install --user ./RobloxStudioLinuxLauncher-0.1.0-x86_64.flatpak
flatpak run io.github.checkpickerupper.RobloxStudioLinuxLauncher
```

Published bundles currently support x86-64 Linux systems. They are distributed
directly through GitHub rather than Flathub.

## Build from a checkout

```bash
flatpak install flathub \
  org.freedesktop.Sdk//25.08 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08 \
  org.freedesktop.Platform//25.08
flatpak-builder --user --install --force-clean build \
  flatpak/io.github.checkpickerupper.RobloxStudioLinuxLauncher.yml
flatpak run io.github.checkpickerupper.RobloxStudioLinuxLauncher
```

The local checkout source excludes generated build and cache directories, so a
normal workspace build does not copy the host `target/` tree into Flatpak.
`cargo-sources.json` is generated from the checked-in `Cargo.lock`, so the Rust
build runs offline and uses the same dependency versions every time. A release
or Flathub submission should replace the local-directory source with a tagged
source archive.

The package contains one managed Kombucha Wine build and the pinned DXVK graphics
layer used by the reference launcher. It does not nest that
build inside the separate `org.winehq.Wine` base because two Wine runtimes can
disagree while creating or updating the same prefix. The launcher, Roblox
Studio, and `StudioMCP.exe` run inside one sandbox and use the same prefix below
the app's `XDG_DATA_HOME`.

Start the GUI and launch Studio from its **Launch Studio** button. Keep the
GUI open while an AI client is connected. Flatpak normally isolates each
`flatpak run` process; the launcher uses the Flatpak host entry only to put the
external MCP command into the already-running launcher sandbox, where the
official `StudioMCP.exe --stdio` can see that Studio session. It never uses a
host Wine prefix or starts a replacement MCP server.

Studio's own sign-in window is enabled by default. Before launch, the launcher
installs the matching WebView2 runtime and makes it use its built-in SwiftShader
renderer instead of Wine's hanging D3D11 software path. The Linux browser remains
a backup through the GUI or `browser-login`; its verified callback returns to the
already-running Studio sandbox.

After installing Studio, enable its built-in MCP server in Studio:

`Assistant` → `…` → `Manage MCP Servers` → `Enable Studio as MCP server`

Then either use the GUI's **Copy client configuration** button or run:

```bash
flatpak run --command=roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher \
  --config "$HOME/.var/app/io.github.checkpickerupper.RobloxStudioLinuxLauncher/data/roblox-studio-linux-launcher/config.ini" \
  mcp doctor
```

An external MCP client may start the same server with:

```bash
flatpak run --command=roblox-studio-linux-launcher \
  io.github.checkpickerupper.RobloxStudioLinuxLauncher mcp serve
```

The Flatpak command deliberately uses the same internal MCP command as the GUI.
It requires the launcher GUI and an open Studio place. If they are not running,
the command reports that clearly instead of claiming that MCP is connected.

The manifest grants both Wayland and X11 access. Wine uses X11 through XWayland
when it is available; the launcher's own GUI can still use native Wayland. The
launcher records that Wine driver order in the prefix before any setup helper or
Studio process starts.

When dependencies change, regenerate `cargo-sources.json` with the official
`flatpak-cargo-generator.py` tool from `flatpak-builder-tools` and review the
result alongside `Cargo.lock`.
