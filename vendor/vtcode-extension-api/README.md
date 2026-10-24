# VTCode extension API

VTCode runs `wasm32-unknown-unknown` extensions with the `vtcode` host imports.
API version `2` adds custom file editors to the existing commands, views,
overlays, settings, diagnostics, and text-edit APIs. API version `1` remains
accepted by the host. This SDK exposes `API_VERSION = "2"`.

## Tagged runtime packages

Each extension has a GitHub repository and versioned GitHub releases.
Users choose **Extensions: Install Extension from GitHub**, paste the repository
link, then select a published version. `#tag=v1.2.0` or a GitHub release URL can
select a particular tag. Branches, source-tree links, bare tags without a release
package, and repositories without eligible releases cannot be installed.

The release must have a semantic version tag (such as `v1.2.0`) and an uploaded
asset named **`vtcode-extension.zip`**. GitHub supplies the asset's SHA-256 digest.
Its ZIP root contains `extension.json`, the prebuilt Wasm module, required runtime
assets, and license notices. Source code, Git history, build tools, and vendored
development dependencies stay in the repository. Git is not required to install.

```json
{
  "id": "example.custom-editor",
  "name": "Example Custom Editor",
  "version": "1.2.0",
  "repository": "https://github.com/owner/custom-editor",
  "apiVersion": "2",
  "main": "custom-editor.wasm"
}
```

The tag and manifest version must match; `apiVersion` is independent. Publish a
new version for corrections. An already installed tag cannot be replaced by a
different package. The picker recommends the highest stable semantic version,
labels prereleases, identifies the installed package, and supports deliberate
upgrades and downgrades. There are no automatic updates.

Public releases work without sign-in. For private repositories or authenticated
API access, VTCode uses `GH_TOKEN` / `GITHUB_TOKEN`, or your existing GitHub CLI
sign-in (`gh auth login`). Credential lookup happens only during an explicit
release lookup or download, with a five-second limit for the CLI. Tokens stay
out of URLs, logs, and installation metadata; redirects to asset-storage hosts
do not receive the authorization header.

Downloads stream to temporary disk storage in bounded chunks and are verified
against the selected asset's exact size and SHA-256 before extraction. The host
checks the manifest, repository identity, version, and Wasm host API before an
atomic replacement. Failures preserve the existing installation. Provenance in
`.vtcode-source.json` records the repository, tag, asset ID, and SHA-256.

Packages contain one root manifest. Absolute paths, traversal, links, special
files, case-colliding paths, nested extension manifests, Git metadata, and forged
installation metadata are rejected. Installed build scripts are never executed.
Compressed and unpacked packages are bounded at 256 MiB, with at most 4096 ZIP
entries and 32 MiB per Wasm module, to keep installation resource usage bounded.

The three example extension projects include `package.py`, an explicit
`package-files.json` allowlist, and a Release workflow. Their build commands also
accept `--package <zip-path>`. To publish, update the manifest/Cargo versions,
build and test, then push the matching tag. The workflow creates the GitHub
release and uploads only the listed runtime files. Existing published assets
are left unchanged. Locally built packages can be uploaded to that release too:

```sh
python package.py --tag v1.2.0 --output dist/vtcode-extension.zip
gh release create v1.2.0 dist/vtcode-extension.zip dist/vtcode-extension.zip.sha256 --verify-tag
```

## File-backed editors

Use `editor::open_document()` during a command invoked for a file. The workbench
authorizes the command's context path or the active file; a guest cannot request
an arbitrary path. An unsaved text buffer for that file must be saved or
discarded first. IDs are positive, scoped to the extension, and valid while its
runtime remains loaded. Opening the same path reuses its document.

`document_info` reports the path, 64-bit size, dirty/read-only state, and history
availability. `read_document(id, offset, count)` returns up to 1 MiB;
`edit_document(id, offset, deleted, bytes)` replaces a range, preserving undo
history. An empty replacement deletes bytes; inserting at the end is supported.
Offsets must fit a nonnegative signed 64-bit integer. `changed_bytes` returns a
0/1 flag per returned byte, compared with the saved bytes at the same offset.

Use the constants in `editor::action` with `document_action`:

| Action | Behavior |
| --- | --- |
| `UNDO`, `REDO` | Return whether history changed the document |
| `SAVE` | Stream and atomically replace after external-change checks |
| `AMEND_NEXT_EDIT` | Fold the next edit into the preceding undo step |
| `BEGIN_TRANSACTION` | Start a group of edits without adding intermediate undo steps |
| `COMMIT_TRANSACTION` | Publish the group as one undo step |
| `ROLLBACK_TRANSACTION` | Restore the bytes and history before the group |

Do not nest transactions. Save, Save As, and Keep Unsaved refuse an unfinished
transaction; finish or roll it back first. Undo/redo are unavailable during a
transaction. History retains up to 1000 edit steps. File reads remain lazy, and
file handles referenced by undo snapshots survive atomic saves. If an external
writer mutates one of those files in place, the host reports an error rather
than reading changed bytes into an old snapshot.

Associate the document with a view using `editor::Frame { document: Some(id),
.. }`. The workbench then routes Save As, Save All, close confirmation, and exit
recovery to that document. `dirty` and `path` are host-owned; a guest cannot
forge them. Save failures preserve the open document. Recovery stores the
current bytes and the command which opened it, then invokes that command at the
next startup. Recovery does not persist guest state or undo history.

## Terminal surfaces and input

Register a normal view and its key/mouse/scroll handlers, then call
`editor::set_frame(view_id, &frame)`. Frames are structured lines of styled
spans, with a viewport size, optional title, cursor, and document ID. They use
theme roles rather than embedded ANSI escape sequences.

Foreground roles: `text`, `muted`, `accent`, `offset`, `changed`, `warning`,
`error`, `success`, `selection`, `cursor`. Background roles: `panel`, `header`,
`selection`, `cursor`, `match`. Unknown roles use the editor defaults. Bold and
underline are supported. The host enforces readable contrast and clips drawing
to the viewport. Control characters in span text are rejected. Use Unicode
terminal column widths when computing layout.

Register `editor::register_resize_handler` and read JSON through
`current_view_event()`. All rich-view events include `viewId`:

- Keys: `code`, optional `text`, `shift`, `control`, `alt`. Codes include `char`,
  `left`, `right`, `enter`, `escape`, `function`, and `paste`.
- Workbench commands: `code: "command"`, with `text` such as `save`, `undo`,
  `redo`, `copy`, `cut`, `paste`, `find`, `find_replace`, `goto_line`,
  `select_all`, `find_next`, or `find_prev`.
- Bracketed paste: `code: "paste"`, `text` containing the pasted content.
- Pointer: `kind: "down" | "up" | "drag"`, absolute `column`/`row`, relative
  `viewColumn`/`viewRow`, `width`, `height`, and modifier flags.
- Scroll: `direction` (-1 up, 1 down), `ctrl`, coordinates and viewport size.
- Resize: `kind: "resize"`, `width`, `height`.
- Workbench document actions: `code: "documentChanged"`; re-read document info
  and render the resulting state.

Input callbacks run on workers. Keystrokes preserve order while a callback is
busy; consecutive pending resize events are coalesced. Keep callbacks bounded:
the host supplies 10 million Wasm fuel units per call and limits guest memory
to 128 MiB. Frames are limited to 2 MiB, 512 lines, and 32768 total spans.
Clipboard reads are supplied for paste events, and writes return through the
UI thread. The guest has no WASI filesystem, process, or network capabilities.

For incremental work, call `editor::register_task_handler(export, interval_ms)`.
These handlers continue while the terminal is idle or unfocused, unlike
cosmetic tick handlers. A positive interval is clamped to at least 10 ms; zero
unregisters it. Unregister promptly when work finishes or is cancelled. Task
handlers run only while an extension's view is open (or it has no views).

[Hex Editor](https://github.com/im-henri/vtcode-ext-hex-editor) is a complete working
example with native behavior tests. VTCode keeps real-Wasm workbench integration
tests in its main repository.
