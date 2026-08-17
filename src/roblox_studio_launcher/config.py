"""Configuration storage for the launcher."""

from __future__ import annotations

import configparser
import os
from dataclasses import dataclass
from pathlib import Path


APP_DIRECTORY_NAME = "roblox-studio-linux-launcher"
CONFIG_FILENAME = "config.ini"
DEFAULT_WINE_BINARY = "wine"


@dataclass(frozen=True, slots=True)
class LauncherConfig:
    """The paths and executable names needed to launch Studio."""

    config_path: Path
    data_directory: Path
    wine_binary: str
    wine_prefix: Path
    studio_executable: Path | None


def default_data_directory() -> Path:
    """Return the user's standard Linux data directory for this app."""

    configured_data_home = os.environ.get("XDG_DATA_HOME")
    if configured_data_home:
        return Path(configured_data_home).expanduser() / APP_DIRECTORY_NAME
    return Path.home() / ".local" / "share" / APP_DIRECTORY_NAME


def default_config_path() -> Path:
    """Return the default path for the launcher configuration."""

    return default_data_directory() / CONFIG_FILENAME


def load_config(config_path: Path) -> LauncherConfig:
    """Load configuration from disk, falling back to safe defaults."""

    expanded_config_path = config_path.expanduser()
    data_directory = expanded_config_path.parent
    default_prefix = data_directory / "wine"
    parser = configparser.ConfigParser()
    parser.read(expanded_config_path, encoding="utf-8")

    wine_binary = parser.get("wine", "binary", fallback=DEFAULT_WINE_BINARY)
    wine_binary = wine_binary or DEFAULT_WINE_BINARY
    configured_prefix = parser.get("wine", "prefix", fallback="")
    wine_prefix = (
        Path(configured_prefix).expanduser()
        if configured_prefix
        else default_prefix
    )
    configured_executable = parser.get("studio", "executable", fallback="")
    studio_executable = (
        Path(configured_executable).expanduser()
        if configured_executable
        else None
    )

    return LauncherConfig(
        config_path=expanded_config_path,
        data_directory=data_directory,
        wine_binary=wine_binary,
        wine_prefix=wine_prefix,
        studio_executable=studio_executable,
    )


def save_config(config: LauncherConfig) -> None:
    """Persist configuration after creating its parent directory."""

    config.config_path.parent.mkdir(parents=True, exist_ok=True)
    parser = configparser.ConfigParser()
    parser.add_section("wine")
    parser.set("wine", "binary", config.wine_binary)
    parser.set("wine", "prefix", str(config.wine_prefix))
    parser.add_section("studio")
    parser.set(
        "studio",
        "executable",
        str(config.studio_executable) if config.studio_executable else "",
    )
    with config.config_path.open("w", encoding="utf-8") as config_file:
        parser.write(config_file)
