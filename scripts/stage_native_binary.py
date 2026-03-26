from __future__ import annotations

import argparse
import os
import shutil
import stat
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "rust" / "doctor_core" / "Cargo.toml"
RELEASE_BINARY = ROOT / "rust" / "doctor_core" / "target" / "release" / "fastapi-doctor-native"
PACKAGE_BIN_ROOT = ROOT / "src" / "fastapi_doctor" / "bin"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build and stage the native fastapi-doctor sidecar.")
    parser.add_argument("--platform-tag", required=True, help="Target asset directory, e.g. linux-x86_64.")
    parser.add_argument("--version", required=True, help="Version embedded into the native binary.")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    env = os.environ.copy()
    env["FASTAPI_DOCTOR_NATIVE_VERSION"] = args.version

    subprocess.run(
        ["cargo", "build", "--release", "--manifest-path", str(MANIFEST_PATH)],
        cwd=ROOT,
        env=env,
        check=True,
    )

    destination_dir = PACKAGE_BIN_ROOT / args.platform_tag
    destination_dir.mkdir(parents=True, exist_ok=True)
    destination = destination_dir / RELEASE_BINARY.name
    shutil.copy2(RELEASE_BINARY, destination)
    destination.chmod(destination.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    print(destination)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
