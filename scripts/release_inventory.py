"""Create deterministic dependency and artifact inventories for a Planora build."""

import argparse
import hashlib
import json
import pathlib
import subprocess


def command(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def is_release_artifact(name: str, version_tokens: set[str]) -> bool:
    if not any(token in name for token in version_tokens):
        return False
    return name.endswith(
        (
            "-Setup.exe",
            ".msi",
            ".deb",
            ".rpm",
            ".pkg",
            ".dmg",
            ".AppImage",
            ".zip",
            ".tar.gz",
            ".manifest.json",
        )
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dist", default="dist")
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parent.parent
    dist = (root / args.dist).resolve()
    dist.mkdir(parents=True, exist_ok=True)

    metadata = json.loads(command("cargo", "metadata", "--locked", "--format-version", "1"))
    workspace = set(metadata["workspace_members"])
    dependencies = sorted(
        (
            {
                "name": package["name"],
                "version": package["version"],
                "license": package.get("license"),
                "license_file": package.get("license_file"),
                "source": package.get("source"),
                "workspace": package["id"] in workspace,
            }
            for package in metadata["packages"]
        ),
        key=lambda item: (item["name"], item["version"]),
    )
    (dist / "dependency-inventory.json").write_text(
        json.dumps(dependencies, indent=2) + "\n", encoding="utf-8"
    )
    missing = [item["name"] for item in dependencies if not item["license"] and not item["license_file"]]
    if missing:
        raise SystemExit(f"Dependencies without declared licenses: {', '.join(missing)}")

    version = next(p["version"] for p in dependencies if p["name"] == "planora-ui" and p["workspace"])
    version_tokens = {version, version.replace("-", "-0.", 1)}
    artifacts = []
    for path in sorted(dist.iterdir()):
        if not path.is_file() or path.name in {"SHA256SUMS.txt", "provenance.json"}:
            continue
        if path.name != "dependency-inventory.json" and not is_release_artifact(path.name, version_tokens):
            continue
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        artifacts.append({"file": path.name, "sha256": digest, "bytes": path.stat().st_size})
    (dist / "SHA256SUMS.txt").write_text(
        "".join(f'{item["sha256"]}  {item["file"]}\n' for item in artifacts),
        encoding="utf-8",
    )
    provenance = {
        "version": version,
        "source_commit": command("git", "rev-parse", "HEAD"),
        "source_clean": not bool(command("git", "status", "--porcelain")),
        "artifacts": artifacts,
    }
    (dist / "provenance.json").write_text(
        json.dumps(provenance, indent=2) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
