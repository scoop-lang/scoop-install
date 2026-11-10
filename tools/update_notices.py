"""Refresh the dependency notices embedded in the standalone installer."""

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT
        )
    )
    sections = ["Scoop installer third-party notices", "Generated from Cargo.lock."]
    for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
        if package["source"] is None:
            continue
        directory = Path(package["manifest_path"]).parent
        paths = {
            path
            for path in directory.iterdir()
            if path.is_file()
            and path.name.upper().startswith(
                ("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE")
            )
        }
        if package["license_file"]:
            paths.add(directory / package["license_file"])
        if package["name"] == "liblzma-sys":
            paths.update(path for path in directory.rglob("COPYING*") if path.is_file())
        sections.append(
            f"{package['name']} {package['version']} — {package['license'] or 'See documents'}"
        )
        for path in sorted(paths):
            sections.append(
                f"{path.relative_to(directory)}\n\n{path.read_text(encoding='utf-8', errors='replace')}"
            )
    destination = ROOT / "licenses/THIRD-PARTY.txt"
    destination.parent.mkdir(exist_ok=True)
    destination.write_text("\n\n".join(sections) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
