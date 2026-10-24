# Hex Editor for VTCode

Inspect and edit binary files in a hex and decoded-text grid, with a data
inspector, mouse selection, byte highlighting, undo/redo, and hex or UTF-8 search
and replacement. The layout follows the familiar Microsoft VS Code Hex Editor:
offsets on the left, synchronized byte and text columns, and an inspector beside
the grid or below it in narrower terminals.

## Open and edit

Choose **Open With Hex Editor** in the Explorer, select **Hex Editor** when
opening a binary file, or run **Hex Editor: Reopen File as Hex** from `F1`.
Save unsaved text changes first if the same file is open in a text tab.

| Shortcut | Action |
| --- | --- |
| Arrow keys, Home/End, Page Up/Down | Move through bytes |
| Ctrl+Home / Ctrl+End | First byte / end of file |
| Shift + movement, mouse drag, Shift+click | Select a byte range |
| Tab | Switch between hex and decoded text |
| Insert | Toggle overwrite / insert mode |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut, paste in the active pane's format |
| Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z | Undo / redo |
| Ctrl+S / Ctrl+Shift+S | Save / Save As |
| Ctrl+F / Ctrl+H | Find / replace |
| F3 / Shift+F3 | Next / previous match |
| Alt+H in the search form | Toggle hex / UTF-8 search |
| Enter / Shift+Enter in the search form | Next / previous match |
| Tab in the replace form | Switch query and replacement fields |
| Ctrl+Enter / Ctrl+Shift+Enter | Replace selected match / replace all |
| Ctrl+G | Go to decimal or `0x` hexadecimal offset |
| Alt+I / Alt+E | Toggle inspector / endianness |
| Escape | Cancel a search or replacement, or close the form |

Hex paste accepts pairs such as `CA FE BA BE`, `cafebabe`, or `0xCA_0xFE`.
Decoded text displays printable ASCII; typing and text search use UTF-8 bytes.
The inspector decodes signed and unsigned integers, floats, bits, UTF-8, and
UTF-16 in the selected endianness. An empty file supports insertion at offset 0.

The two nibbles of a byte form one undo step. Replace All runs in bounded chunks
and forms one undo step; Escape rolls back the whole replacement. Inserted
replacement bytes are never searched again during the same operation.

## Save and recovery

VTCode owns the binary buffer and streams file ranges instead of loading the
whole file into Wasm. Files larger than 4 GiB use 64-bit offsets. Each read or
paste is limited to 1 MiB; search patterns are limited to 4096 bytes.

Save replaces the file atomically after checking for external changes. Save As,
Save All, close confirmation, and the application's exit choices operate on
the binary document. **Keep unsaved** stores a recovery copy and reopens it on
the next launch when the extension is available. Recovery preserves current
bytes; undo history is scoped to the current session.

If another program changes a source file in place, VTCode refuses to read stale
file ranges or overwrite it. Close with **Discard** and reopen the file to use
the external version. An undo snapshot that depends on such a changed file is
also protected. Keep sufficient free disk space for saves and recovery copies.

## Build

Requires Git, Python 3.11 or newer, and the Rust version declared by this project (1.98 or
newer). Run from this directory:

```sh
rustup target add wasm32-unknown-unknown
python build.py
```

This runs the native behavior tests, builds the Wasm module with the locked
dependencies, and refreshes `hex-editor.wasm`. The checked-in module lets users
install without a compiler. Commit the source, manifest, lockfile, and rebuilt
Wasm together.

## Install from Git

Requires a VTCode build with extension API version 2 and the Git extension
installer. Run **Extensions: Install Extension from GitHub**, paste this link,
and press Enter:

```text
https://github.com/im-henri/vtcode-ext-hex-editor.git
```

VTCode shows a version picker before downloading the package. Stable release
tags such as `v0.2.0` appear first, followed by prereleases, other tags, and
development branches. The newest stable release is recommended; the installed
version is shown so you can update or return to an older release.

Use the same link to change versions. Append `#ref=tag-or-branch` to preselect a
specific entry. Each installation records the chosen tag or branch and exact
commit. There are no automatic updates. Branch snapshots can change; published
version tags must remain fixed.

Installation downloads the committed source and prebuilt Wasm; it does not run
build scripts. A repository with no tags offers its development branches until
the author publishes a version tag.

## Publish versions

The extension version in `extension.json`, the Cargo package version, and the
Git tag describe the same release. `apiVersion` describes host compatibility
and is independent of the extension version.

For a new version, run:

```sh
python build.py --version 0.1.1
git add -A
git commit -m "Release 0.1.1"
git tag -a v0.1.1 -m "Hex Editor 0.1.1"
git push origin main
git push origin v0.1.1
```

`--version` synchronizes the manifest, Cargo.toml, and Cargo.lock, then runs
tests and rebuilds the Wasm module. Commit all source changes and the rebuilt
module before tagging. CI rejects mismatched package versions or release tags.
For a prerelease, use a version such as `0.2.0-beta.1` and tag `v0.2.0-beta.1`.

For the initial `0.1.0` release, tag its existing committed implementation as
`v0.2.0` and push that tag. The version becomes visible to VTCode once the tag
is pushed; a GitHub Release page is optional. Never move or force-update a
published version tag. Publish a new patch version for corrections.

## Repository layout

This repository contains the extension source in `src/`, its manifest and
prebuilt Wasm at the root, and the MIT-licensed VTCode extension SDK in
`vendor/vtcode-extension-api/`. It builds independently of a VTCode checkout.
The workflow in `.github/workflows/ci.yml` tests and builds it on Windows and
Linux. Real-Wasm workbench integration tests live in the
[VTCode repository](https://github.com/im-henri/vtcode).

## License

MIT; see `LICENSE`. This is a VTCode extension implemented in Rust, inspired by
the Microsoft extension's interaction model.

## Publish a VTCode release

Keep `extension.json`, `Cargo.toml`, and `Cargo.lock` versions identical. Build
and test the extension, then run:

```sh
python package.py --tag v0.2.0 --output dist/vtcode-extension.zip
```

`package-files.json` explicitly lists the manifest, prebuilt Wasm, documentation,
and license notices needed in the runtime package. Source trees, Git metadata,
build scripts, and unlisted files are excluded. Add runtime assets to this list
when needed. ZIP creation is reproducible and streams files in bounded chunks.

Create and push the matching version tag. The Release workflow builds and tests
the extension and attaches `vtcode-extension.zip` to its GitHub release. A tag
without this package is not installable in VTCode. Existing published packages
are never overwritten: publish a new version for corrections.

Private GitHub releases use your existing `gh auth login` sign-in, or
`GH_TOKEN` / `GITHUB_TOKEN`. VTCode retrieves release metadata and the runtime
ZIP directly; it never clones the repository to install an extension.
