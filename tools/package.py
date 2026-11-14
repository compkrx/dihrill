#!/usr/bin/env python3
"""Package the manifest and native Rust sources using Horizon's .hnmod layout.

Adapted from the MIT-licensed horizon-rust/horizon-native-mod-template packager.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[1]


def package(output: Path) -> None:
    metadata = ROOT / "horizon-native.json"
    native = json.loads(metadata.read_text(encoding="utf-8"))
    compat = json.loads((ROOT / "compatibility.json").read_text(encoding="utf-8"))
    if native["schema"] != 1 or not re.fullmatch(r"[a-z0-9_]+", native["id"]):
        raise SystemExit("Invalid native bundle schema or mod ID")
    if "hooks" in native:
        raise SystemExit("The native manifest must keep its exact Horizon pin")
    for field, key in (("horizon", "horizon_revision"),
                       ("mixins", "horizon_mixins_revision"), ("game", "game")):
        if native[field] != compat[key]:
            raise SystemExit(f"Native manifest and compatibility metadata disagree on {field}")
    folder = ROOT / "mixins" / native["id"]
    with (folder / "horizon-mixin.toml").open("rb") as stream:
        manifest = tomllib.load(stream)
    for field in ("id", "version"):
        if native[field] != manifest["mod"][field]:
            raise SystemExit(f"Native and mixin manifests disagree on {field}")
    if manifest["compatibility"]["horizon_revision"] != native["horizon"]:
        raise SystemExit("Native and mixin manifests disagree on the Horizon revision")

    files = [metadata]
    for path in sorted(folder.iterdir()):
        if path.is_symlink() or not path.is_file() or path.suffix not in (".rs", ".toml"):
            raise SystemExit(f"Unexpected native bundle entry {path}")
        files.append(path)
    if len(files) > 512 or sum(path.stat().st_size for path in files) > 64 * 2**20:
        raise SystemExit("Native bundle exceeds Horizon's entry or size limit")
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w") as archive:
        for file in sorted(files):
            info = zipfile.ZipInfo(file.relative_to(ROOT).as_posix(), (2026, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, file.read_bytes(), compress_type=zipfile.ZIP_DEFLATED)
    with zipfile.ZipFile(output) as archive:
        corrupt = archive.testzip()
        if corrupt:
            raise SystemExit(f"Invalid ZIP member {corrupt}")
    print(json.dumps({"path": str(output.resolve()),
                      "size": output.stat().st_size,
                      "sha256": hashlib.sha256(output.read_bytes()).hexdigest()}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path,
                        default=ROOT / "target" / "Dihrill-26.3-9f42064.hnmod")
    package(parser.parse_args().output)
