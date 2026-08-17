"""Wine process and Studio executable discovery helpers."""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path
from typing import Sequence


def resolve_wine_binary(configured_binary: str) -> str | None:
    """Resolve a configured Wine command or executable path."""

    if os.sep in configured_binary:
        executable_path = Path(configured_binary).expanduser()
        return str(executable_path) if executable_path.is_file() else None
    return shutil.which(configured_binary)


def modification_time(path: Path) -> float:
    """Return a candidate executable's last modification time."""

    return path.stat().st_mtime


def discover_studio_executable(wine_prefix: Path) -> Path | None:
    """Find the newest Studio executable inside a Wine prefix."""

    windows_drive = wine_prefix / "drive_c"
    if not windows_drive.is_dir():
        return None

    candidates = [
        candidate
        for candidate in windows_drive.rglob("RobloxStudioBeta.exe")
        if candidate.is_file()
    ]
    if not candidates:
        return None
    return max(candidates, key=modification_time)


def run_wine(
    wine_binary: str,
    wine_prefix: Path,
    arguments: Sequence[str],
) -> int:
    """Run a Windows program with the launcher's dedicated Wine prefix."""

    wine_prefix.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment["WINEPREFIX"] = str(wine_prefix)
    command = [wine_binary, *arguments]
    completed_process = subprocess.run(command, env=environment, check=False)
    return completed_process.returncode
