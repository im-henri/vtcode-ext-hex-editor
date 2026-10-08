# VTCode extension API

VTCode runs `wasm32-unknown-unknown` extensions with the `vtcode` host imports.
API version `2` adds custom file editors to the existing commands, views,
overlays, settings, diagnostics, and text-edit APIs. API version `1` remains
accepted by the host. This SDK exposes `API_VERSION = "2"`.

## Source-backed packages

Every `extension.json` must declare a hosted Git `repository`, as well as its
`id`, `name`, `version`, `apiVersion`, and Wasm `main` (or LSP configuration).
Repository metadata is required for locally discovered extensions too. Existing
manifests must add it before they can load.

```json
{
  "id": "example.custom-editor",
  "name": "Example Custom Editor",
  "version": "0.1.0",
  "repository": "https://git.example/team/custom-editor",
  "apiVersion": "2",
  "main": "custom-editor.wasm"
}
```

Commit the extension's source and prebuilt module in the same package directory.
Users install through **Extensions: Install Extension from Git Link**. The host
clones HTTPS or SSH links using Git's existing credentials, copies tracked files,
validates every package, and records the commit in `.vtcode-source.json`.
Repositories may contain a single package or a collection. `#path=folder&ref=tag`
selects a package and preselects a branch/tag; GitHub, GitLab and Codeberg browser tree links
are also recognized. Use fragments for branch names containing `/`.

The installer lists remote Git tags and branches before downloading a package.
Semantic release tags such as `v1.2.0` are ordered by version with the latest
stable release recommended; prereleases, other tags, and development branches
are labeled separately. The installed version and selected commit are shown.
Choosing an older tag deliberately downgrades the package. There are no
automatic updates. A repository without tags offers development branches.

Each selection is pinned to the advertised commit. Annotated tags are resolved
to their underlying commits, and full refs distinguish a tag from a branch
with the same name. If a ref changes between selection and download, the host
refuses the installation and asks the user to refresh. `.vtcode-source.json`
records `version`, `reference`, `referenceKind`, and `commit`; older provenance
files without `referenceKind` remain readable.

For a single package, a semantic version tag must match `extension.json`'s
version. Commit source and prebuilt Wasm before tagging, then push the tag to
publish that version. `apiVersion` remains independent of the package version.
Do not force-update published version tags: replacing an already installed tag
with a different commit is refused. Publish a new version for corrections.

Pasting the same source link lets users change installed versions. A different repository
cannot replace the same extension ID. Installation is staged, preserves the
previous version on validation failure, and rolls back a failed publication.
Downloaded build scripts and Git hooks are never run. Packages cannot contain
symlinks, submodules, absolute paths, or parent-directory traversal. Installation
requires Git on PATH and noninteractive access to the repository. Each package
is limited to 256 MiB and each Wasm module to 32 MiB.

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
