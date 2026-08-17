"""Command-line entry point for the launcher."""

# @error-boundary

from __future__ import annotations

import argparse
import configparser
import sys
from dataclasses import replace
from pathlib import Path
from typing import Sequence

from .config import (
    LauncherConfig,
    default_config_path,
    load_config,
    save_config,
)
from .runtime import (
    discover_studio_executable,
    resolve_wine_binary,
    run_wine,
)


def build_parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""

    parser = argparse.ArgumentParser(
        description="Attempt to run Roblox Studio through Wine on Linux."
    )
    parser.add_argument(
        "--config",
        type=Path,
        default=None,
        help="Use a configuration file at this path.",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    commands.add_parser(
        "doctor",
        help="Check Wine, the prefix, and the Studio installation.",
    )

    configure = commands.add_parser(
        "configure",
        help="Save Wine and Studio paths.",
    )
    configure.add_argument("--wine-binary", help="Wine command or executable path.")
    configure.add_argument("--wine-prefix", type=Path, help="Wine prefix directory.")
    configure.add_argument(
        "--studio-executable",
        type=Path,
        help="Path to RobloxStudioBeta.exe.",
    )

    install = commands.add_parser(
        "install",
        help="Run the official Windows Studio installer through Wine.",
    )
    install.add_argument(
        "--installer",
        type=Path,
        required=True,
        help="Path to the downloaded official Studio installer.",
    )

    commands.add_parser("launch", help="Launch the installed Studio executable.")
    return parser


def print_doctor(config: LauncherConfig) -> int:
    """Report whether the configured launcher environment is usable."""

    issues: list[str] = []
    wine_path = resolve_wine_binary(config.wine_binary)
    print(f"Config: {config.config_path}")
    if wine_path is None:
        print(f"Wine: missing ({config.wine_binary})")
        issues.append("Wine is not available on PATH.")
    else:
        print(f"Wine: {wine_path}")

    print(f"Wine prefix: {config.wine_prefix}")
    if not config.wine_prefix.exists():
        print("Wine prefix: not created yet")

    if config.studio_executable is not None:
        print(f"Configured Studio: {config.studio_executable}")
        if not config.studio_executable.is_file():
            issues.append("The configured Studio executable does not exist.")

    discovered_executable = discover_studio_executable(config.wine_prefix)
    if discovered_executable is None:
        print("Discovered Studio: not found")
        issues.append("RobloxStudioBeta.exe was not found in the Wine prefix.")
    else:
        print(f"Discovered Studio: {discovered_executable}")

    if issues:
        for issue in issues:
            print(f"Issue: {issue}", file=sys.stderr)
        return 1
    print("Launcher environment looks ready.")
    return 0


def configure_launcher(arguments: argparse.Namespace, config: LauncherConfig) -> int:
    """Save the explicitly supplied launcher settings."""

    configured_binary = arguments.wine_binary
    wine_binary = (
        configured_binary if isinstance(configured_binary, str) else config.wine_binary
    )
    configured_prefix = arguments.wine_prefix
    wine_prefix = (
        configured_prefix.expanduser()
        if isinstance(configured_prefix, Path)
        else config.wine_prefix
    )
    configured_executable = arguments.studio_executable
    studio_executable = (
        configured_executable.expanduser()
        if isinstance(configured_executable, Path)
        else config.studio_executable
    )
    updated_config = replace(
        config,
        wine_binary=wine_binary,
        wine_prefix=wine_prefix,
        studio_executable=studio_executable,
    )
    save_config(updated_config)
    print(f"Saved configuration to {updated_config.config_path}")
    return 0


def install_studio(arguments: argparse.Namespace, config: LauncherConfig) -> int:
    """Run an official Studio installer and remember the discovered executable."""

    installer = arguments.installer
    if not isinstance(installer, Path) or not installer.expanduser().is_file():
        print("Installer file was not found.", file=sys.stderr)
        return 2

    wine_path = resolve_wine_binary(config.wine_binary)
    if wine_path is None:
        print(
            f"Wine command was not found: {config.wine_binary}",
            file=sys.stderr,
        )
        return 2

    installer_path = installer.expanduser()
    print(f"Running installer with Wine: {installer_path}")
    return_code = run_wine(wine_path, config.wine_prefix, [str(installer_path)])
    if return_code != 0:
        print(f"Installer exited with status {return_code}.", file=sys.stderr)
        return return_code

    discovered_executable = discover_studio_executable(config.wine_prefix)
    if discovered_executable is not None:
        save_config(replace(config, studio_executable=discovered_executable))
        print(f"Saved Studio executable: {discovered_executable}")
    else:
        print("Installer finished, but Studio was not found automatically.")
    return 0


def launch_studio(config: LauncherConfig) -> int:
    """Launch the configured or newest discovered Studio executable."""

    wine_path = resolve_wine_binary(config.wine_binary)
    if wine_path is None:
        print(
            f"Wine command was not found: {config.wine_binary}",
            file=sys.stderr,
        )
        return 2

    studio_executable = config.studio_executable
    if studio_executable is None or not studio_executable.is_file():
        studio_executable = discover_studio_executable(config.wine_prefix)
    if studio_executable is None:
        print(
            "RobloxStudioBeta.exe was not found. Run the install command first.",
            file=sys.stderr,
        )
        return 2

    print(f"Launching Studio: {studio_executable}")
    return run_wine(wine_path, config.wine_prefix, [str(studio_executable)])


def main(argv: Sequence[str] | None = None) -> int:
    """Run the requested launcher command and return its exit status."""

    parser = build_parser()
    arguments = parser.parse_args(argv)
    config_argument = arguments.config
    config_path = (
        config_argument.expanduser()
        if isinstance(config_argument, Path)
        else default_config_path()
    )

    try:
        config = load_config(config_path)
        if arguments.command == "doctor":
            return print_doctor(config)
        if arguments.command == "configure":
            return configure_launcher(arguments, config)
        if arguments.command == "install":
            return install_studio(arguments, config)
        if arguments.command == "launch":
            return launch_studio(config)
        print(f"Unknown command: {arguments.command}", file=sys.stderr)
        return 2
    except (OSError, configparser.Error) as error:
        print(f"Launcher could not complete the command: {error}", file=sys.stderr)
        return 1
