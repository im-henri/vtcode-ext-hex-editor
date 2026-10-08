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

Requires Git, Python 3, and the Rust version declared by this project (1.98 or
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
installer. Run **Extensions: Install Extension from Git Link**, paste this link,
and press Enter:

```text
https://github.com/im-henri/vtcode-ext-hex-editor.git
```

Use the same link to update. Append `#ref=tag-or-branch` to select a version.
Installation downloads the committed source and prebuilt Wasm; it does not run
build scripts. New commits become installable after they are pushed to GitHub.

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
