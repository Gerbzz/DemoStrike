"""Record resolved CI dependency metadata without distributing binaries."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    runner_temp = os.environ.get("RUNNER_TEMP")
    if not runner_temp:
        raise SystemExit("RUNNER_TEMP is required for the hosted manifest")
    lock = json.loads((ROOT / "package-lock.json").read_text(encoding="utf-8"))
    node = [
        {
            "package_path": package_path,
            "version": entry.get("version"),
            "license": entry.get("license"),
        }
        for package_path, entry in lock["packages"].items()
        if package_path
    ]
    metadata = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(ROOT / "src-tauri" / "Cargo.toml")],
        check=True,
        capture_output=True,
    )
    rust = [
        {
            "name": package["name"],
            "version": package["version"],
            "license": package.get("license"),
            "license_file": package.get("license_file"),
            "repository": package.get("repository"),
        }
        for package in json.loads(metadata.stdout)["packages"]
        if package["name"] != "demostrike-rust-candidate"
    ]
    output = Path(runner_temp) / "demostrike-smoke" / "dependencies-resolved.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps({"commit": os.environ.get("GITHUB_SHA"), "npm": node, "rust": rust}, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"Resolved dependency metadata: {len(node)} npm, {len(rust)} Rust; {output}")


if __name__ == "__main__":
    main()
