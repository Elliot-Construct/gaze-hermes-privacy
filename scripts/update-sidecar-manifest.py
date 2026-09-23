#!/usr/bin/env python3
"""Update sidecar-release.json from built artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path


def compute_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(8192), b""):
            h.update(chunk)
    return h.hexdigest()


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Update sidecar release manifest")
    p.add_argument("--protocol-version", type=int, default=1)
    p.add_argument("--artifacts-dir", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    return p.parse_args()


def main() -> int:
    args = parse_args()

    artifacts_dir = args.artifacts_dir
    if not artifacts_dir.exists():
        print(f"Artifacts directory not found: {artifacts_dir}", file=sys.stderr)
        return 1

    # Expected targets and their OS/arch mapping
    target_map = {
        "x86_64-unknown-linux-gnu": ("linux", "x86_64"),
        "aarch64-unknown-linux-gnu": ("linux", "aarch64"),
        "x86_64-apple-darwin": ("darwin", "x86_64"),
        "aarch64-apple-darwin": ("darwin", "aarch64"),
        "x86_64-pc-windows-msvc": ("windows", "x86_64"),
    }

    artifacts = []

    for target, (os_name, arch) in target_map.items():
        binary_name = f"gaze-hermes-sidecar{'-' + target if target != 'x86_64-unknown-linux-gnu' else ''}"
        if os_name == "windows":
            binary_name += ".exe"

        # Find the binary in dist directory
        binary_path = args.artifacts_dir / f"gaze-hermes-sidecar-{target}"
        if not binary_path.exists():
            # Try without target suffix for linux x86_64
            if target == "x86_64-unknown-linux-gnu":
                binary_path = args.artifacts_dir / "gaze-hermes-sidecar"
                if not binary_path.exists():
                    print(f"Binary not found: {binary_path}", file=sys.stderr)
                    return 1
            else:
                print(f"Binary not found for {target}: {binary_path}", file=sys.stderr)
                return 1

        checksum_path = binary_path.with_suffix(binary_path.suffix + ".sha256")
        if not checksum_path.exists():
            print(f"Checksum not found for {target}: {checksum_path}", file=sys.stderr)
            return 1

        # Verify checksum
        expected = checksum_path.read_text().strip().split()[0]
        actual = compute_sha256(binary_path)
        if expected != actual:
            print(f"Checksum mismatch for {target}: expected {expected}, got {actual}", file=sys.stderr)
            return 1

        artifacts.append({
            "os": os_name,
            "arch": arch,
            "url": f"https://github.com/example/gaze-hermes-privacy/releases/download/v0.0.0/{binary_path.name}",
            "sha256": actual,
        })

    manifest = {
        "protocol_version": args.protocol_version,
        "artifacts": artifacts,
    }

    args.output.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(f"Manifest written to {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())