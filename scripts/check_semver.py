#!/usr/bin/env python3
"""Validate the package version and, when provided, its release tag."""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


NUMERIC_IDENTIFIER = r"(?:0|[1-9][0-9]*)"
PRERELEASE_IDENTIFIER = rf"(?:{NUMERIC_IDENTIFIER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(
    rf"{NUMERIC_IDENTIFIER}\.{NUMERIC_IDENTIFIER}\.{NUMERIC_IDENTIFIER}"
    rf"(?:-{PRERELEASE_IDENTIFIER}(?:\.{PRERELEASE_IDENTIFIER})*)?"
    rf"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


def package_version(manifest_path: Path) -> str:
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(manifest_path),
            "--no-deps",
            "--format-version",
            "1",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        message = result.stderr.strip() or "cargo metadata failed without an error message"
        raise ValueError(message)

    metadata = json.loads(result.stdout)
    packages = metadata.get("packages", [])
    if len(packages) != 1:
        raise ValueError("expected Cargo metadata to contain exactly one package")

    version = packages[0].get("version")
    if not isinstance(version, str):
        raise ValueError("Cargo metadata did not provide a package version")
    return version


def require_semver(value: str, description: str) -> None:
    if SEMVER.fullmatch(value) is None:
        raise ValueError(f"{description} {value!r} is not valid SemVer 2.0")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest-path",
        type=Path,
        default=Path("Cargo.toml"),
        help="path to the Cargo manifest (default: Cargo.toml)",
    )
    parser.add_argument(
        "--tag",
        help="release tag to validate against the package version, such as v0.1.0",
    )
    args = parser.parse_args()

    try:
        version = package_version(args.manifest_path)
        require_semver(version, "package version")
        if args.tag is not None:
            if not args.tag.startswith("v"):
                raise ValueError("release tags must start with 'v'")
            tag_version = args.tag[1:]
            require_semver(tag_version, "release tag version")
            if tag_version != version:
                raise ValueError(
                    f"release tag {args.tag!r} does not match package version {version!r}"
                )
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"SemVer check failed: {error}", file=sys.stderr)
        return 1

    if args.tag is None:
        print(f"SemVer check passed for package version {version}")
    else:
        print(f"SemVer check passed for package version {version} and tag {args.tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
