use crate::editor::{Backend, Event, HexEditor, Pane, Prompt, parse_hex};
use vtcode_extension_api::editor::DocumentInfo;

struct Memory {
    bytes: Vec<u8>,
    saved: Vec<u8>,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
    clipboard: String,
    amend: bool,
    read_only: bool,
    reads: Vec<(u64, usize)>,
    transaction: Option<Vec<u8>>,
}
impl Memory {
    fn new(bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes.into();
        Self {
            saved: bytes.clone(),
            bytes,
            undo: vec![],
            redo: vec![],
            clipboard: String::new(),
            amend: false,
            read_only: false,
            reads: vec![],
            transaction: None,
        }
    }
}
impl Backend for Memory {
    fn info(&mut self, _: u32) -> Result<DocumentInfo, String> {
        Ok(DocumentInfo {
            path: "sample.bin".into(),
            size: self.bytes.len() as u64,
            dirty: self.bytes != self.saved,
            read_only: self.read_only,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        })
    }
    fn read(&mut self, _: u32, offset: u64, count: usize) -> Result<Vec<u8>, String> {
        self.reads.push((offset, count));
        let begin = usize::try_from(offset).map_err(|_| "offset")?;
        if begin > self.bytes.len() {
            return Err("offset".into());
        }
        Ok(self.bytes[begin..begin.saturating_add(count).min(self.bytes.len())].into())
    }
    fn edit(&mut self, _: u32, offset: u64, delete: u64, bytes: &[u8]) -> Result<(), String> {
        if self.read_only {
            return Err("This file is read-only".into());
        }
        if !std::mem::take(&mut self.amend) && self.transaction.is_none() {
            self.undo.push(self.bytes.clone());
        }
        if self.transaction.is_none() {
            self.redo.clear();
        }
        self.bytes.splice(
            offset as usize..(offset + delete) as usize,
            bytes.iter().copied(),
        );
        Ok(())
    }
    fn action(&mut self, _: u32, action: u32) -> Result<bool, String> {
        match action {
            1 => {
                if let Some(bytes) = self.undo.pop() {
                    self.redo.push(std::mem::replace(&mut self.bytes, bytes));
                    return Ok(true);
                }
            }
            2 => {
                if let Some(bytes) = self.redo.pop() {
                    self.undo.push(std::mem::replace(&mut self.bytes, bytes));
                    return Ok(true);
                }
            }
            3 => self.saved = self.bytes.clone(),
            5 => self.amend = true,
            6 => {
                assert!(self.transaction.is_none());
                self.transaction = Some(self.bytes.clone());
            }
            7 => {
                let original = self.transaction.take().unwrap();
                if original != self.bytes {
                    self.undo.push(original);
                    self.redo.clear();
                }
            }
            8 => self.bytes = self.transaction.take().unwrap(),
            _ => return Err("action".into()),
        }
        Ok(false)
    }
    fn clipboard(&mut self) -> Result<String, String> {
        Ok(self.clipboard.clone())
    }
    fn copy(&mut self, text: &str) -> Result<(), String> {
        self.clipboard = text.into();
        Ok(())
    }
    fn changed(&mut self, _: u32, offset: u64, count: usize) -> Result<Vec<u8>, String> {
        Ok(
            (offset as usize..(offset as usize + count).min(self.bytes.len()))
                .map(|index| u8::from(self.saved.get(index) != self.bytes.get(index)))
                .collect(),
        )
    }
}
fn key(editor: &mut HexEditor, memory: &mut Memory, code: &str, text: &str) {
    editor.event(
        Event {
            code: code.into(),
            text: text.into(),
            ..Default::default()
        },
        memory,
    );
    assert!(!editor.error, "{}", editor.status);
}
fn command(editor: &mut HexEditor, memory: &mut Memory, text: &str) {
    key(editor, memory, "command", text);
}

#[test]
fn two_nibbles_are_one_undoable_edit_and_save_preserves_history() {
    let mut memory = Memory::new(vec![0x12, 0x34]);
    let mut editor = HexEditor::new(1);
    key(&mut editor, &mut memory, "char", "c");
    key(&mut editor, &mut memory, "char", "a");
    assert_eq!(memory.bytes, [0xCA, 0x34]);
    assert_eq!(editor.cursor, 1);
    command(&mut editor, &mut memory, "save");
    command(&mut editor, &mut memory, "undo");
    assert_eq!(memory.bytes, [0x12, 0x34]);
    command(&mut editor, &mut memory, "redo");
    assert_eq!(memory.bytes, memory.saved);
}

#[test]
fn insert_overwrite_ascii_and_selection_paste_work_at_eof() {
    let mut memory = Memory::new(b"ABC".to_vec());
    let mut editor = HexEditor::new(1);
    editor.pane = Pane::Text;
    key(&mut editor, &mut memory, "char", "x");
    assert_eq!(memory.bytes, b"xBC");
    key(&mut editor, &mut memory, "insert", "");
    key(&mut editor, &mut memory, "char", "é");
    assert_eq!(memory.bytes, "xéBC".as_bytes());
    editor.anchor = Some(0);
    editor.cursor = 3;
    key(&mut editor, &mut memory, "paste", "hi");
    assert_eq!(memory.bytes, b"hiBC");
    editor.cursor = memory.bytes.len() as u64;
    editor.pane = Pane::Hex;
    key(&mut editor, &mut memory, "paste", "00 ff");
    assert_eq!(memory.bytes, b"hiBC\0\xff");
    key(&mut editor, &mut memory, "backspace", "");
    assert_eq!(memory.bytes, b"hiBC\0");
}

#[test]
fn copy_cut_and_paste_follow_the_active_pane() {
    let mut memory = Memory::new(b"ABCD".to_vec());
    let mut editor = HexEditor::new(1);
    editor.anchor = Some(0);
    editor.cursor = 2;
    command(&mut editor, &mut memory, "copy");
    assert_eq!(memory.clipboard, "41 42");
    command(&mut editor, &mut memory, "cut");
    assert_eq!(memory.bytes, b"CD");
    editor.insert = true;
    command(&mut editor, &mut memory, "paste");
    assert_eq!(memory.bytes, b"ABCD");
    editor.pane = Pane::Text;
    editor.cursor = 0;
    editor.anchor = Some(4);
    command(&mut editor, &mut memory, "copy");
    assert_eq!(memory.clipboard, "ABCD");
}

#[test]
fn invalid_input_and_read_only_errors_preserve_bytes() {
    let mut memory = Memory::new(vec![1]);
    let mut editor = HexEditor::new(1);
    key(&mut editor, &mut memory, "right", "");
    assert_eq!(editor.cursor, 1);
    memory.read_only = true;
    editor.event(
        Event {
            code: "char".into(),
            text: "a".into(),
            ..Default::default()
        },
        &mut memory,
    );
    assert!(editor.error);
    assert_eq!(memory.bytes, [1]);
    for input in ["a", "0g", "12 3", "00; rm"] {
        assert!(parse_hex(input).is_err());
    }
    assert_eq!(parse_hex("0xCA_0xfe\n 00").unwrap(), [0xCA, 0xFE, 0]);
}

#[test]
fn searches_cross_chunk_boundaries_wrap_backward_and_cancel() {
    let mut bytes = vec![0; 200_000];
    bytes[65_535..65_539].copy_from_slice(b"find");
    bytes[150_000..150_004].copy_from_slice(b"find");
    let mut memory = Memory::new(bytes);
    let mut editor = HexEditor::new(1);
    editor.prompt = Some(Prompt::Find {
        query: "find".into(),
        replacement: String::new(),
        hex: false,
        replace: false,
        field: 0,
    });
    key(&mut editor, &mut memory, "enter", "");
    while editor.search.is_some() {
        editor.search_step(&mut memory).unwrap();
    }
    assert_eq!(
        editor.selection(memory.bytes.len() as u64),
        (65_535, 65_539)
    );
    key(&mut editor, &mut memory, "enter", "");
    while editor.search.is_some() {
        editor.search_step(&mut memory).unwrap();
    }
    assert_eq!(editor.cursor, 150_000);
    editor.event(
        Event {
            code: "enter".into(),
            shift: true,
            ..Default::default()
        },
        &mut memory,
    );
    while editor.search.is_some() {
        editor.search_step(&mut memory).unwrap();
    }
    assert_eq!(editor.cursor, 65_535);
    key(&mut editor, &mut memory, "enter", "");
    key(&mut editor, &mut memory, "escape", "");
    assert!(editor.search.is_none());
    assert!(
        memory
            .reads
            .iter()
            .all(|(_, count)| *count <= crate::search::CHUNK + 4095)
    );
}

#[test]
fn go_to_and_replace_validate_ranges_and_change_only_the_match() {
    let mut memory = Memory::new(b"abcabc".to_vec());
    let mut editor = HexEditor::new(1);
    editor.prompt = Some(Prompt::Goto("0x3".into()));
    key(&mut editor, &mut memory, "enter", "");
    assert_eq!(editor.cursor, 3);
    editor.prompt = Some(Prompt::Find {
        query: "61 62".into(),
        replacement: "ff".into(),
        hex: true,
        replace: true,
        field: 0,
    });
    key(&mut editor, &mut memory, "enter", "");
    assert_eq!(editor.selection(6), (3, 5));
    editor.event(
        Event {
            code: "enter".into(),
            control: true,
            ..Default::default()
        },
        &mut memory,
    );
    assert_eq!(memory.bytes, b"abc\xffc");
    editor.prompt = Some(Prompt::Goto("999".into()));
    editor.event(
        Event {
            code: "enter".into(),
            ..Default::default()
        },
        &mut memory,
    );
    assert!(editor.error);
    assert!(editor.prompt.is_some());
}

#[test]
fn responsive_frames_stay_inside_the_surface_and_inspect_endianness() {
    let mut memory = Memory::new((0..=255).collect::<Vec<u8>>());
    let mut editor = HexEditor::new(1);
    for width in [1, 16, 40, 80, 100, 120, 160] {
        for height in [1, 5, 18, 30, 50] {
            editor.width = width;
            editor.height = height;
            let frame = editor.render(&mut memory).unwrap();
            assert_eq!(frame.lines.len(), height as usize);
            assert!(frame.lines.iter().all(|line| {
                line.iter()
                    .map(|span| span.text.chars().count())
                    .sum::<usize>()
                    <= width as usize
            }));
            if let Some((x, y)) = frame.cursor {
                assert!(x < width && y < height);
            }
        }
    }
    assert!(
        crate::render::inspect(&[1, 2, 3, 4], true)
            .iter()
            .any(|(label, value)| *label == "UInt16" && value == "513")
    );
    assert!(
        crate::render::inspect(&[1, 2, 3, 4], false)
            .iter()
            .any(|(label, value)| *label == "UInt16" && value == "258")
    );
}

#[test]
fn clicking_and_dragging_select_synchronized_bytes() {
    let mut memory = Memory::new(vec![0; 100]);
    let mut editor = HexEditor::new(1);
    editor.render(&mut memory).unwrap();
    let layout = editor.layout(100);
    editor.event(
        Event {
            kind: "down".into(),
            view_column: layout.hex_start as u16,
            view_row: layout.data_start as u16,
            ..Default::default()
        },
        &mut memory,
    );
    editor.event(
        Event {
            kind: "drag".into(),
            view_column: (layout.hex_start + 6) as u16,
            view_row: layout.data_start as u16,
            ..Default::default()
        },
        &mut memory,
    );
    assert_eq!(editor.selection(100), (0, 3));
    editor.event(
        Event {
            kind: "up".into(),
            ..Default::default()
        },
        &mut memory,
    );
    assert!(!editor.dragging);
    // Shift+click keeps both the selection start and the clicked byte.
    editor.event(
        Event {
            kind: "down".into(),
            view_column: layout.hex_start as u16,
            view_row: layout.data_start as u16,
            ..Default::default()
        },
        &mut memory,
    );
    editor.event(
        Event {
            kind: "down".into(),
            shift: true,
            view_column: layout.hex_x(1) as u16,
            view_row: (layout.data_start + 1) as u16,
            ..Default::default()
        },
        &mut memory,
    );
    assert_eq!(editor.selection(100), (0, 18));
}

#[test]
fn prompt_editing_handles_unicode_selection_paste_and_delete() {
    let mut memory = Memory::new(Vec::new());
    let mut editor = HexEditor::new(1);
    command(&mut editor, &mut memory, "find");
    key(&mut editor, &mut memory, "paste", "aé界z");
    key(&mut editor, &mut memory, "left", "");
    key(&mut editor, &mut memory, "backspace", "");
    editor.event(
        Event {
            code: "left".into(),
            shift: true,
            ..Default::default()
        },
        &mut memory,
    );
    command(&mut editor, &mut memory, "copy");
    assert_eq!(memory.clipboard, "é");
    key(&mut editor, &mut memory, "paste", "hello");
    if let Some(Prompt::Find { query, .. }) = &editor.prompt {
        assert_eq!(query, "ahelloz");
    } else {
        panic!("missing prompt");
    }
    command(&mut editor, &mut memory, "select_all");
    key(&mut editor, &mut memory, "delete", "");
    if let Some(Prompt::Find { query, .. }) = &editor.prompt {
        assert!(query.is_empty());
    }
    assert!(memory.bytes.is_empty());
}

#[test]
fn replace_all_crosses_blocks_expands_deletes_and_is_one_undoable_operation() {
    for replacement in ["", "X", "abcabcabc"] {
        let mut original = vec![b'_'; 65_535];
        original.extend_from_slice(b"abcabc");
        original.extend(vec![b'_'; 65_537]);
        original.extend_from_slice(b"abc");
        let expected = String::from_utf8(original.clone())
            .unwrap()
            .replace("abc", replacement)
            .into_bytes();
        let mut memory = Memory::new(original.clone());
        let mut editor = HexEditor::new(1);
        editor.prompt = Some(Prompt::Find {
            query: "abc".into(),
            replacement: replacement.into(),
            hex: false,
            replace: true,
            field: 0,
        });
        editor.event(
            Event {
                code: "enter".into(),
                control: true,
                shift: true,
                ..Default::default()
            },
            &mut memory,
        );
        while editor.search.is_some() {
            editor.search_step(&mut memory).unwrap();
        }
        assert_eq!(memory.bytes, expected);
        assert_eq!(memory.undo.len(), 1);
        command(&mut editor, &mut memory, "undo");
        assert_eq!(memory.bytes, original);
        command(&mut editor, &mut memory, "redo");
        assert_eq!(memory.bytes, expected);
    }
}

#[test]
fn cancelled_replace_all_restores_bytes_and_existing_history() {
    let original = vec![b'a'; 150_000];
    let mut memory = Memory::new(original.clone());
    let mut editor = HexEditor::new(1);
    editor.prompt = Some(Prompt::Find {
        query: "a".into(),
        replacement: "long replacement".into(),
        hex: false,
        replace: true,
        field: 0,
    });
    editor.event(
        Event {
            code: "enter".into(),
            control: true,
            shift: true,
            ..Default::default()
        },
        &mut memory,
    );
    assert!(editor.search.is_some());
    assert_ne!(memory.bytes, original);
    key(&mut editor, &mut memory, "escape", "");
    assert_eq!(memory.bytes, original);
    assert!(memory.undo.is_empty());
    assert!(memory.transaction.is_none());
}

#[test]
fn edits_are_highlighted_in_both_panes_until_saved() {
    let mut memory = Memory::new(vec![0; 16]);
    let mut editor = HexEditor::new(1);
    key(&mut editor, &mut memory, "paste", "FE");
    let frame = editor.render(&mut memory).unwrap();
    let changed: Vec<_> = frame
        .lines
        .iter()
        .flatten()
        .filter(|span| span.role == "changed")
        .collect();
    assert_eq!(changed.len(), 2);
    assert_eq!(changed[0].text, "FE");
    command(&mut editor, &mut memory, "save");
    assert!(
        !editor
            .render(&mut memory)
            .unwrap()
            .lines
            .iter()
            .flatten()
            .any(|span| span.role == "changed")
    );
}

#[test]
fn searches_and_replacements_straddle_large_read_chunks() {
    let chunk = crate::search::CHUNK;
    let mut bytes = vec![b'.'; chunk * 2 + 1000];
    for at in [chunk - 2, chunk * 2 - 1] {
        bytes[at..at + 4].copy_from_slice(b"seam");
    }
    let mut memory = Memory::new(bytes.clone());
    let mut editor = HexEditor::new(1);
    let find = |editor: &mut HexEditor, memory: &mut Memory, shift: bool| {
        editor.prompt = Some(Prompt::Find {
            query: "seam".into(),
            replacement: String::new(),
            hex: false,
            replace: false,
            field: 0,
        });
        editor.event(
            Event {
                code: "enter".into(),
                shift,
                ..Default::default()
            },
            memory,
        );
        while editor.search.is_some() {
            editor.search_step(memory).unwrap();
        }
        editor.cursor
    };
    assert_eq!(find(&mut editor, &mut memory, false), (chunk - 2) as u64);
    assert_eq!(
        find(&mut editor, &mut memory, false),
        (chunk * 2 - 1) as u64
    );
    assert_eq!(find(&mut editor, &mut memory, true), (chunk - 2) as u64);
    editor.prompt = Some(Prompt::Find {
        query: "seam".into(),
        replacement: "joined!".into(),
        hex: false,
        replace: true,
        field: 0,
    });
    editor.event(
        Event {
            code: "enter".into(),
            control: true,
            shift: true,
            ..Default::default()
        },
        &mut memory,
    );
    while editor.search.is_some() {
        editor.search_step(&mut memory).unwrap();
    }
    let expected = String::from_utf8(bytes)
        .unwrap()
        .replace("seam", "joined!")
        .into_bytes();
    assert_eq!(memory.bytes, expected);
    assert!(
        editor.status.contains("Replaced 2 matches"),
        "{}",
        editor.status
    );
}

#[test]
fn scrollbar_drags_cover_the_whole_file_and_wheel_pages_with_shift() {
    let mut memory = Memory::new(vec![7; 1 << 20]);
    let mut editor = HexEditor::new(1);
    editor.width = 100;
    editor.height = 30;
    editor.inspector = false;
    editor.render(&mut memory).unwrap();
    let layout = editor.layout(1 << 20);
    let (thumb, _) = editor.thumb(1 << 20, &layout).unwrap();
    assert_eq!(thumb, 0);
    let x = layout.scrollbar as u16;
    let pointer = |editor: &mut HexEditor, memory: &mut Memory, kind: &str, y: usize| {
        editor.event(
            Event {
                kind: kind.into(),
                view_column: x,
                view_row: y as u16,
                ..Default::default()
            },
            memory,
        );
        editor.render(memory).unwrap();
    };
    pointer(&mut editor, &mut memory, "down", layout.data_start);
    pointer(&mut editor, &mut memory, "drag", layout.data_start + 200);
    assert_eq!(editor.top, HexEditor::max_top(1 << 20, &layout));
    pointer(&mut editor, &mut memory, "drag", 0);
    assert_eq!(editor.top, 0);
    pointer(&mut editor, &mut memory, "up", 0);
    assert!(editor.scroll_grab.is_none());
    editor.event(
        Event {
            direction: 1,
            shift: true,
            ..Default::default()
        },
        &mut memory,
    );
    assert_eq!(editor.top, ((layout.rows - 1) * layout.bpr) as u64);
    assert_eq!(editor.cursor, 0, "scrolling leaves the cursor in place");
}
