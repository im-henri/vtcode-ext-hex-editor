"""Build a tested Wasm package and optionally export its standalone Git source."""
import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import tomllib
from urllib.parse import urlparse


def valid_version(value):
    if not isinstance(value, str):
        return False
    match = re.fullmatch(
        r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
        r"(?:-(?P<pre>[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
        r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?", value)
    return bool(match) and not any(
        item.isdigit() and len(item) > 1 and item.startswith("0")
        for item in (match.group("pre") or "").split("."))


def package_version(source, requested=None, git_ref=""):
    manifest_path = source / "extension.json"
    cargo_path = source / "Cargo.toml"
    lock_path = source / "Cargo.lock"
    metadata = json.loads(manifest_path.read_text(encoding="utf-8"))
    cargo_text = cargo_path.read_text(encoding="utf-8")
    lock_text = lock_path.read_text(encoding="utf-8")
    package = tomllib.loads(cargo_text)["package"]
    locked = [item for item in tomllib.loads(lock_text)["package"] if item["name"] == package["name"]]
    if len(locked) != 1:
        raise ValueError("Cargo.lock must contain exactly one entry for the extension package")
    version = requested if requested is not None else metadata["version"]
    if not valid_version(version):
        raise ValueError("Use a semantic version such as 0.1.1 or 0.2.0-beta.1")
    if git_ref.startswith("refs/tags/") and git_ref[len("refs/tags/"):] not in (version, f"v{version}"):
        raise ValueError(f"Git tag {git_ref} does not match extension version {version}")
    if requested is not None:
        cargo_pattern = r'(?ms)(^\[package\]\s*\n.*?^version\s*=\s*")[^"]+(".*?$)'
        lock_pattern = rf'(?m)(^name = "{re.escape(package["name"])}"\nversion = ")[^"]+(".*?$)'
        cargo_text, cargo_count = re.subn(cargo_pattern, lambda item: item[1] + version + item[2], cargo_text, count=1)
        lock_text, lock_count = re.subn(lock_pattern, lambda item: item[1] + version + item[2], lock_text, count=1)
        if cargo_count != 1 or lock_count != 1:
            raise ValueError("Could not locate the extension version in Cargo.toml and Cargo.lock")
        metadata["version"] = version
        manifest_path.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8", newline="\n")
        cargo_path.write_text(cargo_text, encoding="utf-8", newline="\n")
        lock_path.write_text(lock_text, encoding="utf-8", newline="\n")
    elif package["version"] != version or locked[0]["version"] != version:
        raise ValueError("extension.json, Cargo.toml and Cargo.lock versions differ; use --version to synchronize them")
    return version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-tests", action="store_true")
    parser.add_argument("--version", help="Set the extension's semantic version before testing and rebuilding")
    parser.add_argument("--package", type=pathlib.Path)
    parser.add_argument("--repository", help="Hosted Git repository for the exported manifest")
    args = parser.parse_args()
    source = pathlib.Path(__file__).resolve().parent
    if args.repository:
        url = urlparse(args.repository)
        if url.scheme not in ("https", "ssh") or not url.hostname or url.password or url.query or url.fragment:
            parser.error("--repository must be an HTTPS or SSH Git repository root link")
        if not args.package:
            parser.error("--repository requires --package")
    if args.package and args.package.exists():
        parser.error("The export directory must not already exist")
    try:
        version = package_version(source, args.version, os.environ.get("GITHUB_REF", ""))
    except (ValueError, KeyError) as error:
        parser.error(str(error))
    print(f"Building Hex Editor {version}", flush=True)
    manifest = str(source / "Cargo.toml")
    if not args.skip_tests:
        subprocess.run(["cargo", "test", "--locked", "--manifest-path", manifest], check=True)
    subprocess.run([
        "cargo", "build", "--locked", "--release", "--target", "wasm32-unknown-unknown",
        "--manifest-path", manifest, "--target-dir", str(source / "target"),
    ], check=True)
    shutil.copy2(source / "target/wasm32-unknown-unknown/release/vtcode_hex_editor.wasm", source / "hex-editor.wasm")
    if not args.package:
        return
    destination = args.package.resolve()
    destination.mkdir(parents=True)
    for name in ("src", "tests", "Cargo.toml", "Cargo.lock", "extension.json", "hex-editor.wasm", "README.md", "LICENSE", "build.py", ".gitignore", ".gitattributes", "rust-toolchain.toml", ".github"):
        item = source / name
        if item.is_dir():
            shutil.copytree(item, destination / name, ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "*.pyo"))
        else:
            shutil.copy2(item, destination / name)
    # The exported package can itself be exported again without the VTCode tree.
    sdk = source / "vendor/vtcode-extension-api"
    if not sdk.is_dir():
        sdk = source.parent.parent / "extension-api"
    vendor = destination / "vendor/vtcode-extension-api"
    shutil.copytree(sdk / "src", vendor / "src")
    for name in ("Cargo.toml", "README.md"):
        shutil.copy2(sdk / name, vendor / name)
    shutil.copy2(source / "LICENSE", vendor / "LICENSE")
    cargo = destination / "Cargo.toml"
    cargo.write_text(cargo.read_text(encoding="utf-8").replace('../../extension-api', 'vendor/vtcode-extension-api'), encoding="utf-8", newline="\n")
    package = destination / "extension.json"
    metadata = json.loads(package.read_text(encoding="utf-8"))
    if args.repository:
        metadata["repository"] = args.repository
    package.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(f"Standalone extension source and Wasm: {destination}")


if __name__ == "__main__":
    main()
