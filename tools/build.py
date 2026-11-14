#!/usr/bin/env python3
"""Plan, stage, compile, test and validate StrataDrill with the pinned Horizon tools."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
COMPAT = json.loads((ROOT / "compatibility.json").read_text(encoding="utf-8"))


def run(args: list, env: dict, *, capture: bool = False) -> str:
    command = [str(arg) for arg in args]
    command[0] = shutil.which(command[0], path=env["PATH"]) or command[0]
    result = subprocess.run(command, env=env, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout or ""


def verify_source(source: Path, env: dict) -> None:
    expected = COMPAT["horizon_revision"]
    checkout = (source / ".git").exists()
    if checkout:
        if run(["git", "-C", source, "rev-parse", "HEAD"], env, capture=True).strip() != expected:
            raise SystemExit(f"Horizon checkout must be at {expected}")
        if run(["git", "-C", source, "status", "--porcelain", "-uno"], env, capture=True).strip():
            raise SystemExit("Horizon checkout has modified tracked files")
    elif (source / ".horizon-revision").read_text(encoding="utf-8").strip() != expected:
        raise SystemExit(f"Horizon export must have .horizon-revision {expected}")
    for path, expected_sha in COMPAT["source_sha256"].items():
        content = (source / path).read_bytes()
        if checkout:
            content = content.replace(b"\r\n", b"\n")
        if hashlib.sha256(content).hexdigest() != expected_sha:
            raise SystemExit(f"Pinned Horizon source checksum mismatch: {path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("plan", "stage", "build", "test", "validate"))
    parser.add_argument("--horizon", type=Path, required=True,
                        help="clean pinned Git checkout or launcher's pristine source export")
    parser.add_argument("--mixins", type=Path,
                        help="clean Git checkout of the pinned Horizon Mixins revision")
    parser.add_argument("--rust", type=Path, help="launcher Rust directory, containing bin and mingw64")
    parser.add_argument("--cargo-cache", type=Path, help="optional populated Cargo registry cache directory")
    parser.add_argument("--build-dir", type=Path, default=ROOT / "target")
    parser.add_argument("--cargo-target", type=Path, help="explicit target directory for this exact Horizon pin")
    parser.add_argument("--mixins-target", type=Path, help="explicit target directory for the pinned Mixins CLI")
    parser.add_argument("--bundle", type=Path, help="source bundle to check with the pinned cargo-horizon-mod CLI")
    parser.add_argument("--profile", choices=("fast", "dev", "release"), default="fast",
                        help="launcher performance, dev or release build")
    parser.add_argument("--no-default-features", action="store_true", help="skip game audio and controllers")
    args = parser.parse_args()

    env = dict(os.environ)
    if args.rust:
        dirs = [args.rust / "bin", args.rust / "mingw64" / "bin"]
        env["PATH"] = os.pathsep.join([*(str(path.resolve()) for path in dirs), env["PATH"]])
    if args.cargo_cache:
        env["CARGO_HOME"] = str(args.cargo_cache.resolve())
    rust = run(["rustc", "-vV"], env, capture=True)
    if (f"host: {COMPAT['rust_target']}\n" not in rust
            or not re.search(rf"^release: {re.escape(COMPAT['rust_toolchain'])}$", rust, re.MULTILINE)):
        raise SystemExit(f"Expected Rust {COMPAT['rust_toolchain']} for {COMPAT['rust_target']}")
    horizon = args.horizon.resolve()
    verify_source(horizon, env)
    build_dir = args.build_dir.resolve()
    build_dir.mkdir(parents=True, exist_ok=True)
    cargo_target = (args.cargo_target or build_dir / "horizon-cargo").resolve()
    env["CARGO_TARGET_DIR"] = str(cargo_target)

    if args.action == "validate":
        if not args.bundle:
            parser.error("validate requires --bundle")
        profile = "fast" if args.profile == "dev" else args.profile
        run(["cargo", "run", "--manifest-path", horizon / "Cargo.toml", "--profile", profile,
             "--locked", "--offline", "-p", "cargo-horizon-mod", "--", "check",
             args.bundle.resolve(), "--kind", "native-mod", "--json"], env)
        return

    if not args.mixins:
        parser.error(f"{args.action} requires --mixins")
    mixins = args.mixins.resolve()
    revision = run(["git", "-C", mixins, "rev-parse", "HEAD"], env, capture=True).strip()
    if revision != COMPAT["horizon_mixins_revision"]:
        raise SystemExit(f"Horizon Mixins checkout must be at {COMPAT['horizon_mixins_revision']}")
    if run(["git", "-C", mixins, "status", "--porcelain", "-uno"], env, capture=True).strip():
        raise SystemExit("Horizon Mixins checkout has modified tracked files")
    mixins_target = (args.mixins_target or build_dir / "mixins-cargo").resolve()
    mixins_env = dict(env, CARGO_TARGET_DIR=str(mixins_target))
    run(["cargo", "build", "--locked", "--offline", "--manifest-path", mixins / "Cargo.toml",
         "-p", "horizon-mixins-cli"], mixins_env)
    cli = mixins_target / "debug" / "horizon-mixins.exe"
    stage = build_dir / "modded"
    common = ["--workspace", horizon, "--mods", ROOT / "mixins"]
    if args.action == "plan":
        run([cli, "plan", *common, "--out", build_dir / "plan.json"], env)
        return
    if stage.is_symlink() or (stage.exists() and stage.resolve().parent != build_dir):
        raise SystemExit(f"Refusing to move a stage outside {build_dir}")
    if stage.exists() and not (stage / ".horizon-mixins-stage.json").is_file():
        raise SystemExit(f"Existing stage is not a generated Horizon workspace: {stage}")
    with tempfile.TemporaryDirectory(prefix=".stage-", dir=build_dir) as directory:
        scratch = Path(directory)
        staged = scratch / "new"
        previous = scratch / "previous"
        run([cli, "apply", *common, "--out", staged], env)
        if stage.exists():
            stage.rename(previous)
        try:
            staged.rename(stage)
        except OSError:
            if previous.exists():
                previous.rename(stage)
            raise
    if args.action == "stage":
        print(stage)
        return
    profile = "fast" if args.profile == "dev" else args.profile
    target = ["--lib"] if args.action == "test" else ["--bin", "horizon-client"]
    cargo = ["cargo", args.action, "--manifest-path", stage / "Cargo.toml", "--locked", "--offline",
             "-p", "horizon-client", *target, "--profile", profile]
    if args.profile == "dev":
        cargo += ["--config", "profile.fast.package.horizon-client.opt-level=0"]
    if args.no_default_features:
        cargo.append("--no-default-features")
    if args.action == "test":
        cargo += ["--", "ore_miner", "--nocapture"]
    run(cargo, env)
    if args.action == "build":
        print(cargo_target / profile / "horizon-client.exe")


if __name__ == "__main__":
    main()
