use serde::Deserialize;
use vtcode_extension_api::editor::DocumentInfo;

pub trait Backend {
    fn info(&mut self, id: u32) -> Result<DocumentInfo, String>;
    fn read(&mut self, id: u32, offset: u64, count: usize) -> Result<Vec<u8>, String>;
    fn changed(&mut self, _id: u32, _offset: u64, count: usize) -> Result<Vec<u8>, String> {
        Ok(vec![0; count])
    }
    fn edit(&mut self, id: u32, offset: u64, delete: u64, bytes: &[u8]) -> Result<(), String>;
    fn action(&mut self, id: u32, action: u32) -> Result<bool, String>;
    fn clipboard(&mut self) -> Result<String, String>;
    fn copy(&mut self, text: &str) -> Result<(), String>;
    /// True when a long search should return and continue on the next task call.
    fn should_yield(&mut self) -> bool {
        true
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub control: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub width: u16,
    #[serde(default)]
    pub height: u16,
    #[serde(default)]
    pub view_column: u16,
    #[serde(default)]
    pub view_row: u16,
    #[serde(default)]
    pub direction: i32,
    #[serde(default)]
    pub click_count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Hex,
    Text,
}

#[derive(Clone, Debug)]
pub enum Prompt {
    Goto(String),
    Find {
        query: String,
        replacement: String,
        hex: bool,
        replace: bool,
        field: usize,
    },
}

/// A clickable region of the last frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Command(&'static str),
    Field(usize),
    Scrollbar,
}

pub struct HexEditor {
    pub document: u32,
    pub cursor: u64,
    pub anchor: Option<u64>,
    pub top: u64,
    pub pane: Pane,
    pub insert: bool,
    pub little_endian: bool,
    pub inspector: bool,
    pub low_nibble: bool,
    pub width: u16,
    pub height: u16,
    pub prompt: Option<Prompt>,
    pub status: String,
    pub error: bool,
    pub dragging: bool,
    drag_origin: u64,
    pub reveal_cursor: bool,
    /// Clickable regions of the last frame: row, first column, end column.
    pub hits: Vec<(u16, u16, u16, Hit)>,
    pub search: Option<crate::search::SearchTask>,
    pub last_find: Option<(String, bool)>,
    pub prompt_cursor: usize,
    pub prompt_anchor: Option<usize>,
    /// Pointer position while it is over the view.
    pub hover: Option<(u16, u16)>,
    /// Row within the thumb where a scrollbar drag grabbed it.
    pub scroll_grab: Option<u64>,
    /// Document size seen by the last event or frame.
    pub size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub bpr: usize,
    pub offset_width: usize,
    pub offset_start: usize,
    pub hex_start: usize,
    pub ascii_start: usize,
    /// First row of byte data; the column header is the row above it.
    pub data_start: usize,
    pub rows: usize,
    /// Column of the scrollbar, right of the decoded text.
    pub scrollbar: usize,
    pub inspector_start: Option<usize>,
    pub inspector_top: Option<usize>,
}

impl Layout {
    pub fn hex_x(&self, column: usize) -> usize {
        self.hex_start + column * 3 + column / 8
    }
    pub fn hex_end(&self) -> usize {
        self.hex_x(self.bpr - 1) + 2
    }
}

pub const INSPECTOR_WIDTH: usize = 34;

pub fn prompt_rows(prompt: &Option<Prompt>) -> usize {
    match prompt {
        None => 0,
        Some(Prompt::Find { replace: true, .. }) => 2,
        Some(_) => 1,
    }
}

impl HexEditor {
    pub fn new(document: u32) -> Self {
        Self {
            document,
            cursor: 0,
            anchor: None,
            top: 0,
            pane: Pane::Hex,
            insert: false,
            little_endian: true,
            inspector: true,
            low_nibble: false,
            width: 100,
            height: 28,
            prompt: None,
            status: String::new(),
            error: false,
            dragging: false,
            drag_origin: 0,
            reveal_cursor: true,
            hits: Vec::new(),
            search: None,
            last_find: None,
            prompt_cursor: 0,
            prompt_anchor: None,
            hover: None,
            scroll_grab: None,
            size: 0,
        }
    }

    pub fn layout(&self, size: u64) -> Layout {
        let width = self.width as usize;
        let height = self.height as usize;
        let offset_width = if size > u32::MAX as u64 { 16 } else { 8 };
        // Left pad, offset, gap, bytes, gap, text, gap, scrollbar.
        let grid = |n: usize| 1 + offset_width + 2 + n * 3 + (n - 1) / 8 - 1 + 3 + n + 2;
        let side = self.inspector && width >= grid(16) + INSPECTOR_WIDTH;
        let inspector_start = side.then(|| width - INSPECTOR_WIDTH);
        let available = inspector_start.unwrap_or(width);
        let bpr = [16, 8, 4, 1]
            .into_iter()
            .find(|n| grid(*n) <= available)
            .unwrap_or(1);
        let offset_start = 1;
        let hex_start = offset_start + offset_width + 2;
        let hex_end = hex_start + bpr * 3 + (bpr - 1) / 8 - 1;
        let ascii_start = hex_end + 3;
        let data_start = 2 + prompt_rows(&self.prompt);
        let inspector_top = (self.inspector && !side && width >= 40 && height >= 20)
            .then(|| height.saturating_sub(8));
        let bottom = inspector_top.unwrap_or(height.saturating_sub(1));
        let rows = bottom.saturating_sub(data_start).max(1);
        Layout {
            bpr,
            offset_width,
            offset_start,
            hex_start,
            ascii_start,
            data_start,
            rows,
            scrollbar: available.saturating_sub(1),
            inspector_start,
            inspector_top,
        }
    }

    /// Rows in the document, including the empty row that accepts appends.
    pub fn total_rows(size: u64, layout: &Layout) -> u64 {
        size / layout.bpr as u64 + 1
    }

    pub fn max_top(size: u64, layout: &Layout) -> u64 {
        Self::total_rows(size, layout).saturating_sub(layout.rows as u64) * layout.bpr as u64
    }

    /// Scrollbar thumb as (first row, height) within the track, if the
    /// document needs one.
    pub fn thumb(&self, size: u64, layout: &Layout) -> Option<(usize, usize)> {
        let total = Self::total_rows(size, layout) as u128;
        let track = layout.rows as u128;
        if total <= track {
            return None;
        }
        let height = (track * track / total).clamp(1, track);
        let max_row = total - track;
        let top_row = (self.top / layout.bpr as u64) as u128;
        let y = top_row.min(max_row) * (track - height) / max_row;
        Some((y as usize, height as usize))
    }

    fn scroll_to_thumb(&mut self, size: u64, layout: &Layout, thumb_y: i64) {
        let Some((_, height)) = self.thumb(size, layout) else {
            return;
        };
        let room = (layout.rows - height) as i128;
        let total = Self::total_rows(size, layout) as i128;
        let max_row = total - layout.rows as i128;
        let row = if room <= 0 {
            0
        } else {
            (thumb_y as i128).clamp(0, room) * max_row / room
        };
        self.top = row as u64 * layout.bpr as u64;
    }

    pub fn selection(&self, size: u64) -> (u64, u64) {
        match self.anchor {
            Some(anchor) if anchor != self.cursor => (
                anchor.min(self.cursor).min(size),
                anchor.max(self.cursor).min(size),
            ),
            _ => (
                self.cursor.min(size),
                self.cursor.saturating_add(1).min(size),
            ),
        }
    }

    fn move_to(&mut self, offset: u64, shift: bool, size: u64) {
        if shift {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = offset.min(size);
        self.low_nibble = false;
    }

    pub fn hit_at(&self, x: u16, y: u16) -> Option<Hit> {
        self.hits
            .iter()
            .find(|(row, start, end, _)| *row == y && x >= *start && x < *end)
            .map(|hit| hit.3)
    }

    /// Byte under a grid position, and which pane it is in.
    pub fn byte_at(&self, layout: &Layout, x: usize, y: usize) -> Option<(u64, Pane)> {
        if y < layout.data_start || y >= layout.data_start + layout.rows {
            return None;
        }
        let (column, pane) = if x >= layout.ascii_start && x < layout.ascii_start + layout.bpr {
            (x - layout.ascii_start, Pane::Text)
        } else if x + 1 >= layout.hex_start && x <= layout.hex_end() {
            let column = (0..layout.bpr)
                .min_by_key(|column| (layout.hex_x(*column) * 2 + 1).abs_diff(x * 2))
                .unwrap_or(0);
            (column, Pane::Hex)
        } else {
            return None;
        };
        Some((
            self.top + ((y - layout.data_start) * layout.bpr + column) as u64,
            pane,
        ))
    }

    /// What the pointer is over, for deciding whether a hover changes the frame.
    pub fn hover_key(&self) -> (Option<Hit>, Option<u64>) {
        let Some((x, y)) = self.hover else {
            return (None, None);
        };
        let layout = self.layout(self.size);
        (
            self.hit_at(x, y),
            self.byte_at(&layout, x as usize, y as usize)
                .map(|(offset, _)| offset)
                .filter(|offset| *offset < self.size),
        )
    }

    pub fn event(&mut self, event: Event, backend: &mut impl Backend) {
        if matches!(event.kind.as_str(), "move" | "leave") {
            self.hover = (event.kind == "move").then_some((event.view_column, event.view_row));
            return;
        }
        self.error = false;
        self.reveal_cursor = event.direction == 0
            && !matches!(event.kind.as_str(), "resize" | "drag" | "up" | "cancel")
            && !(event.kind == "down"
                && self.hit_at(event.view_column, event.view_row) == Some(Hit::Scrollbar));
        if event.width > 0 {
            self.width = event.width;
        }
        if event.height > 0 {
            self.height = event.height;
        }
        let result = self.handle(event, backend);
        if let Err(error) = result {
            self.status = error;
            self.error = true;
        }
    }

    fn handle(&mut self, event: Event, backend: &mut impl Backend) -> Result<(), String> {
        let info = backend.info(self.document)?;
        self.size = info.size;
        let layout = self.layout(info.size);
        self.cursor = self.cursor.min(info.size);
        if event.kind == "resize" || event.code == "documentChanged" {
            return Ok(());
        }
        if self.search.is_some() && event.code == "escape" {
            self.cancel_search(backend)?;
            self.status = "Search cancelled".into();
            return Ok(());
        }
        if self.search.is_some() && !matches!(event.code.as_str(), "" | "other") {
            self.cancel_search(backend)?;
        }
        if event.direction != 0 {
            let rows = if event.shift || event.control {
                layout.rows.saturating_sub(1).max(1)
            } else {
                3
            };
            let step = (layout.bpr * rows) as u64;
            self.top = if event.direction < 0 {
                self.top.saturating_sub(step)
            } else {
                self.top
                    .saturating_add(step)
                    .min(Self::max_top(info.size, &layout))
            };
            return Ok(());
        }
        if matches!(event.kind.as_str(), "down" | "up" | "drag" | "cancel") {
            return self.pointer(event, &info, layout, backend);
        }
        if event.code == "command"
            && !(self.prompt.is_some()
                && matches!(event.text.as_str(), "paste" | "copy" | "cut" | "select_all"))
        {
            return self.command(&event.text, backend, &info);
        }
        if self.prompt.is_some() {
            return self.prompt_key(event, backend, &info);
        }
        if event.code == "paste" {
            let bytes = if self.pane == Pane::Hex {
                parse_hex(&event.text)?
            } else {
                event.text.into_bytes()
            };
            return self.paste_bytes(&bytes, backend, &info);
        }
        if event.control {
            if matches!(event.code.as_str(), "home" | "end") {
                self.move_to(
                    if event.code == "home" { 0 } else { info.size },
                    event.shift,
                    info.size,
                );
                return Ok(());
            }
            let command = match event.text.to_ascii_lowercase().as_str() {
                "s" => "save",
                "z" if event.shift => "redo",
                "z" => "undo",
                "y" => "redo",
                "a" => "select_all",
                "c" => "copy",
                "x" => "cut",
                "v" => "paste",
                "f" => "find",
                "h" => "find_replace",
                "g" => "goto_line",
                _ => "",
            };
            return self.command(command, backend, &info);
        }
        if event.alt {
            match event.text.to_ascii_lowercase().as_str() {
                "i" => self.inspector = !self.inspector,
                "e" => self.little_endian = !self.little_endian,
                _ => {}
            }
            return Ok(());
        }
        let bpr = layout.bpr as u64;
        let page = (layout.bpr * layout.rows.saturating_sub(1).max(1)) as u64;
        match event.code.as_str() {
            "left" => self.move_to(self.cursor.saturating_sub(1), event.shift, info.size),
            "right" => self.move_to(self.cursor.saturating_add(1), event.shift, info.size),
            "up" => self.move_to(self.cursor.saturating_sub(bpr), event.shift, info.size),
            "down" => self.move_to(self.cursor.saturating_add(bpr), event.shift, info.size),
            "pageUp" | "pageDown" => {
                let up = event.code == "pageUp";
                self.move_to(
                    if up {
                        self.cursor.saturating_sub(page)
                    } else {
                        self.cursor.saturating_add(page)
                    },
                    event.shift,
                    info.size,
                );
                // Keep the cursor on the same screen row, like a text editor.
                self.top = if up {
                    self.top.saturating_sub(page)
                } else {
                    self.top
                        .saturating_add(page)
                        .min(Self::max_top(info.size, &layout))
                };
            }
            "home" => self.move_to(self.cursor / bpr * bpr, event.shift, info.size),
            "end" => self.move_to(
                (self.cursor / bpr * bpr + bpr - 1).min(info.size),
                event.shift,
                info.size,
            ),
            "tab" | "backTab" => {
                self.pane = if self.pane == Pane::Hex {
                    Pane::Text
                } else {
                    Pane::Hex
                };
                self.low_nibble = false;
            }
            "insert" => self.insert = !self.insert,
            "escape" => {
                self.anchor = None;
                self.status.clear();
            }
            "delete" | "backspace" => {
                let (mut start, end) = self.selection(info.size);
                let mut delete = end - start;
                if event.code == "backspace" && self.anchor.is_none() {
                    if self.cursor == 0 {
                        return Ok(());
                    }
                    start = self.cursor - 1;
                    delete = 1;
                }
                if delete == 0 {
                    return Ok(());
                }
                backend.edit(self.document, start, delete, &[])?;
                self.move_to(start, false, info.size - delete);
            }
            "char" if self.pane == Pane::Hex => {
                let Some(digit) = event.text.chars().next().and_then(|ch| ch.to_digit(16)) else {
                    return Err("Type 0–9 or A–F in the hex grid; Tab switches to text".into());
                };
                let (start, end) = self.selection(info.size);
                let previous = backend
                    .read(self.document, start, 1)?
                    .first()
                    .copied()
                    .unwrap_or(0);
                let value = if self.low_nibble {
                    (previous & 0xf0) | digit as u8
                } else {
                    (digit as u8) << 4 | previous & 0x0f
                };
                let delete = if self.low_nibble {
                    u64::from(self.cursor < info.size)
                } else if self.anchor.is_some() {
                    end - start
                } else if self.insert {
                    0
                } else {
                    u64::from(self.cursor < info.size)
                };
                if self.low_nibble {
                    backend.action(self.document, 5)?;
                }
                backend.edit(self.document, start, delete, &[value])?;
                self.cursor = start;
                self.anchor = None;
                if self.low_nibble {
                    self.cursor += 1;
                }
                self.low_nibble = !self.low_nibble;
                self.status.clear();
            }
            "char" => {
                self.paste_bytes(event.text.as_bytes(), backend, &info)?;
                self.status.clear();
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn command(
        &mut self,
        command: &str,
        backend: &mut impl Backend,
        info: &DocumentInfo,
    ) -> Result<(), String> {
        match command {
            "save" => {
                backend.action(self.document, 3)?;
                self.status = "Saved".into();
            }
            "undo" | "redo" => {
                backend.action(self.document, if command == "undo" { 1 } else { 2 })?;
                self.anchor = None;
                self.low_nibble = false;
                self.status.clear();
            }
            "select_all" => {
                self.anchor = Some(0);
                self.cursor = info.size;
            }
            "find" | "find_replace" => {
                let (query, hex) = match &self.prompt {
                    Some(Prompt::Find { query, hex, .. }) => (query.clone(), *hex),
                    _ => self
                        .last_find
                        .clone()
                        .unwrap_or((String::new(), self.pane == Pane::Hex)),
                };
                self.prompt_cursor = query.len();
                self.prompt_anchor = (!query.is_empty()).then_some(0);
                self.prompt = Some(Prompt::Find {
                    query,
                    replacement: String::new(),
                    hex,
                    replace: command == "find_replace",
                    field: 0,
                });
            }
            "toggle_replace" => {
                if let Some(Prompt::Find { replace, field, .. }) = &mut self.prompt {
                    *replace = !*replace;
                    *field = 0;
                }
            }
            "find_mode_text" | "find_mode_hex" => {
                if let Some(Prompt::Find { hex, .. }) = &mut self.prompt {
                    *hex = command == "find_mode_hex";
                }
            }
            "find_next" | "find_prev" => {
                if self.prompt.is_none() {
                    let Some((query, hex)) = self.last_find.clone() else {
                        return self.command("find", backend, info);
                    };
                    self.prompt = Some(Prompt::Find {
                        query,
                        replacement: String::new(),
                        hex,
                        replace: false,
                        field: 0,
                    });
                }
                self.prompt_key(
                    Event {
                        code: "enter".into(),
                        shift: command == "find_prev",
                        ..Default::default()
                    },
                    backend,
                    info,
                )?;
            }
            "replace_one" | "replace_all" => {
                self.prompt_key(
                    Event {
                        code: "enter".into(),
                        control: true,
                        shift: command == "replace_all",
                        ..Default::default()
                    },
                    backend,
                    info,
                )?;
            }
            "goto_submit" => {
                self.prompt_key(
                    Event {
                        code: "enter".into(),
                        ..Default::default()
                    },
                    backend,
                    info,
                )?;
            }
            "close_prompt" => {
                self.cancel_search(backend)?;
                self.prompt = None;
                self.status.clear();
            }
            "goto_line" => {
                self.prompt_cursor = 0;
                self.prompt_anchor = None;
                self.prompt = Some(Prompt::Goto(String::new()));
            }
            "copy" | "cut" => {
                let (start, end) = self.selection(info.size);
                if end - start > 1024 * 1024 {
                    return Err("Select at most 1 MiB to copy".into());
                }
                let bytes = backend.read(self.document, start, (end - start) as usize)?;
                let text = if self.pane == Pane::Hex {
                    hex_text(&bytes)
                } else {
                    String::from_utf8_lossy(&bytes).into_owned()
                };
                backend.copy(&text)?;
                self.status = format!("Copied {} bytes", bytes.len());
                if command == "cut" {
                    backend.edit(self.document, start, end - start, &[])?;
                    self.move_to(start, false, info.size - (end - start));
                }
            }
            "paste" => {
                let text = backend.clipboard()?;
                let bytes = if self.pane == Pane::Hex {
                    parse_hex(&text)?
                } else {
                    text.into_bytes()
                };
                self.paste_bytes(&bytes, backend, info)?;
            }
            "inspector" => self.inspector = !self.inspector,
            "endian" => self.little_endian = !self.little_endian,
            "little_endian" | "big_endian" => self.little_endian = command == "little_endian",
            "mode" => self.insert = !self.insert,
            "insert_mode" | "overwrite_mode" => self.insert = command == "insert_mode",
            _ => {}
        }
        Ok(())
    }

    fn paste_bytes(
        &mut self,
        bytes: &[u8],
        backend: &mut impl Backend,
        info: &DocumentInfo,
    ) -> Result<(), String> {
        let (start, end) = self.selection(info.size);
        let deleted = if self.anchor.is_some() {
            end - start
        } else if self.insert {
            0
        } else {
            (bytes.len() as u64).min(info.size - start)
        };
        backend.edit(self.document, start, deleted, bytes)?;
        self.move_to(
            start + bytes.len() as u64,
            false,
            info.size - deleted + bytes.len() as u64,
        );
        Ok(())
    }

    fn pointer(
        &mut self,
        event: Event,
        info: &DocumentInfo,
        layout: Layout,
        backend: &mut impl Backend,
    ) -> Result<(), String> {
        let x = event.view_column as usize;
        let y = event.view_row as usize;
        if matches!(event.kind.as_str(), "up" | "cancel") {
            self.dragging = false;
            self.scroll_grab = None;
            return Ok(());
        }
        if let Some(grab) = self.scroll_grab {
            let row = y as i64 - layout.data_start as i64 - grab as i64;
            self.scroll_to_thumb(info.size, &layout, row);
            return Ok(());
        }
        if event.kind == "down" {
            match self.hit_at(event.view_column, event.view_row) {
                Some(Hit::Command(command)) => return self.command(command, backend, info),
                Some(Hit::Field(field)) => {
                    self.focus_field(field, x);
                    return Ok(());
                }
                Some(Hit::Scrollbar) => {
                    if let Some((thumb, height)) = self.thumb(info.size, &layout) {
                        let row = y.saturating_sub(layout.data_start);
                        // Grabbing the thumb keeps its offset; the track jumps there.
                        let grab = if row >= thumb && row < thumb + height {
                            row - thumb
                        } else {
                            height / 2
                        };
                        self.scroll_grab = Some(grab as u64);
                        self.scroll_to_thumb(info.size, &layout, row as i64 - grab as i64);
                    }
                    return Ok(());
                }
                None => {}
            }
        }
        if event.kind == "drag" && !self.dragging {
            return Ok(());
        }
        // Dragging past the grid scrolls and keeps extending the selection.
        let last = layout.data_start + layout.rows - 1;
        let bpr = layout.bpr as u64;
        let mut row = y;
        if self.dragging && y < layout.data_start {
            self.top = self.top.saturating_sub(bpr);
            row = layout.data_start;
        } else if self.dragging && y > last {
            self.top = self
                .top
                .saturating_add(bpr)
                .min(Self::max_top(info.size, &layout));
            row = last;
        }
        let x = if self.dragging {
            let (lo, hi) = if self.pane == Pane::Hex {
                (layout.hex_start, layout.hex_end())
            } else {
                (layout.ascii_start, layout.ascii_start + layout.bpr - 1)
            };
            x.clamp(lo, hi)
        } else {
            x
        };
        let Some((offset, pane)) = self.byte_at(&layout, x, row) else {
            return Ok(());
        };
        let offset = offset.min(info.size);
        if event.kind == "down" {
            self.pane = pane;
            if event.click_count >= 2 && !event.shift {
                // Double click selects the whole row.
                let start = offset / bpr * bpr;
                self.anchor = Some(start.min(info.size));
                self.cursor = (start + bpr).min(info.size);
                self.low_nibble = false;
                return Ok(());
            }
            self.low_nibble = false;
            self.dragging = true;
            if !event.shift {
                self.move_to(offset, false, info.size);
                self.drag_origin = offset;
                return Ok(());
            }
            // Shift+click extends from the selection start like a drag does.
            self.drag_origin = match self.anchor {
                Some(anchor) if anchor > self.cursor => anchor - 1,
                Some(anchor) => anchor,
                None => self.cursor,
            };
        }
        if self.dragging {
            // Both the byte where the drag began and the one under the pointer
            // stay selected, whichever direction the drag goes.
            let origin = self.drag_origin;
            if offset >= origin {
                self.anchor = Some(origin);
                self.cursor = offset.saturating_add(1).min(info.size);
            } else {
                self.anchor = Some(origin.saturating_add(1).min(info.size));
                self.cursor = offset;
            }
        }
        Ok(())
    }

    fn focus_field(&mut self, field: usize, x: usize) {
        let start = self
            .hits
            .iter()
            .find(|hit| hit.3 == Hit::Field(field))
            .map_or(0, |hit| hit.1 as usize + 1);
        let text = match &mut self.prompt {
            Some(Prompt::Find {
                query,
                replacement,
                field: active,
                ..
            }) => {
                *active = field;
                if field == 0 { query } else { replacement }
            }
            Some(Prompt::Goto(input)) => input,
            None => return,
        };
        let column = x.saturating_sub(start);
        self.prompt_cursor = text
            .char_indices()
            .nth(column)
            .map_or(text.len(), |(index, _)| index);
        self.prompt_anchor = None;
    }
}

pub const HEX: &[u8; 16] = b"0123456789ABCDEF";

pub fn hex_text(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            text.push(' ');
        }
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 15) as usize] as char);
    }
    text
}

pub fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let compact = text
        .replace("0x", "")
        .replace("0X", "")
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '_')
        .collect::<String>();
    if compact.is_empty() {
        return Err("Enter hexadecimal bytes, for example DE AD BE EF".into());
    }
    if compact.len() % 2 != 0 || !compact.bytes().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("Enter complete hexadecimal byte pairs, for example DE AD BE EF".into());
    }
    compact
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                .map_err(|error| error.to_string())
        })
        .collect()
}
