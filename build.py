"""Build a tested Wasm package and optionally export its standalone Git source."""
import argparse
import json
import pathlib
import shutil
import subprocess
from urllib.parse import urlparse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-tests", action="store_true")
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
    for name in ("src", "Cargo.toml", "Cargo.lock", "extension.json", "hex-editor.wasm", "README.md", "LICENSE", "build.py", ".gitignore", ".gitattributes", "rust-toolchain.toml", ".github"):
        item = source / name
        if item.is_dir():
            shutil.copytree(item, destination / name)
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
    cargo.write_text(cargo.read_text(encoding="utf-8").replace('../../extension-api', 'vendor/vtcode-extension-api'), encoding="utf-8")
    package = destination / "extension.json"
    metadata = json.loads(package.read_text(encoding="utf-8"))
    if args.repository:
        metadata["repository"] = args.repository
    package.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    print(f"Standalone extension source and Wasm: {destination}")


if __name__ == "__main__":
    main()
