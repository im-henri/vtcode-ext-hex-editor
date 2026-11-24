//! Frame layout: a toolbar of buttons, an optional find or go-to bar, the
//! offset/hex/text grid with its scrollbar, the data inspector beside or
//! below the grid, and a status bar.
use crate::editor::{HEX, HexEditor, Hit, Layout, Pane, Prompt};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use vtcode_extension_api::editor::{DocumentInfo, Frame, Span};

/// One frame row. Adjacent text with the same style shares a span, which
/// keeps frames small for the host to parse and paint.
#[derive(Default)]
struct Row {
    spans: Vec<Span>,
    x: usize,
}

impl Row {
    fn styled(&mut self, text: &str, role: &str, bg: &str, bold: bool) {
        if text.is_empty() {
            return;
        }
        self.x += if text.is_ascii() {
            text.len()
        } else {
            text.width()
        };
        if let Some(last) = self.spans.last_mut()
            && last.role == role
            && last.background == bg
            && last.bold == bold
        {
            last.text.push_str(text);
            return;
        }
        self.spans.push(Span {
            text: run(text),
            role: role.into(),
            background: bg.into(),
            bold,
            underline: false,
        });
    }
    fn text(&mut self, text: &str, role: &str, bg: &str) {
        self.styled(text, role, bg, false);
    }
    /// One-column characters on the hot path skip width measurement.
    fn chars(&mut self, text: &[u8], role: &str, bg: &str) {
        let text = std::str::from_utf8(text).unwrap_or("?");
        if let Some(last) = self.spans.last_mut()
            && last.role == role
            && last.background == bg
            && !last.bold
        {
            last.text.push_str(text);
        } else {
            self.spans.push(Span {
                text: run(text),
                role: role.into(),
                background: bg.into(),
                ..Span::default()
            });
        }
        self.x += text.len();
    }
    fn pad(&mut self, to: usize, bg: &str) {
        if to > self.x {
            self.text(&" ".repeat(to - self.x), "text", bg);
        }
    }
}

/// Span text with room for the run to grow without reallocating.
fn run(text: &str) -> String {
    let mut run = String::with_capacity(text.len().max(32));
    run.push_str(text);
    run
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Normal,
    Primary,
    Disabled,
}

struct Item {
    label: &'static str,
    hit: &'static str,
    look: Look,
    /// Columns of surface before the button; 0 joins a segmented control.
    gap: usize,
}

fn item(label: &'static str, hit: &'static str, look: Look, gap: usize) -> Item {
    Item {
        label,
        hit,
        look,
        gap,
    }
}

fn items_width(items: &[Item]) -> usize {
    items
        .iter()
        .map(|item| item.gap + item.label.len() + 2)
        .sum()
}

struct Painter<'a> {
    hits: &'a mut Vec<(u16, u16, u16, Hit)>,
    hover: Option<(u16, u16)>,
}

impl Painter<'_> {
    fn hovered(&self, y: usize, start: usize, end: usize) -> bool {
        self.hover.is_some_and(|(x, row)| {
            row as usize == y && (x as usize) >= start && (x as usize) < end
        })
    }

    fn button(&mut self, row: &mut Row, y: usize, label: &str, hit: &'static str, look: Look) {
        let start = row.x;
        let end = start + label.width() + 2;
        let hovered = look != Look::Disabled && self.hovered(y, start, end);
        let bg = match (look, hovered) {
            (Look::Primary, false) => "buttonPrimary",
            (Look::Primary, true) => "buttonPrimaryHover",
            (_, true) => "buttonHover",
            _ => "button",
        };
        let role = if look == Look::Disabled {
            "muted"
        } else {
            "text"
        };
        row.text(" ", role, bg);
        row.text(label, role, bg);
        row.text(" ", role, bg);
        if look != Look::Disabled {
            self.hits
                .push((y as u16, start as u16, end as u16, Hit::Command(hit)));
        }
    }

    fn items(&mut self, row: &mut Row, y: usize, items: &[Item], surface: &str) {
        for item in items {
            row.pad(row.x + item.gap, surface);
            self.button(row, y, item.label, item.hit, item.look);
        }
    }
}

/// An editable field drawn on the input background. Returns the caret column
/// when the field is active.
#[allow(clippy::too_many_arguments)]
fn field(
    row: &mut Row,
    hits: &mut Vec<(u16, u16, u16, Hit)>,
    y: usize,
    index: usize,
    text: &str,
    placeholder: &str,
    width: usize,
    active: bool,
    cursor: usize,
    anchor: Option<usize>,
) -> Option<usize> {
    let start = row.x;
    hits.push((
        y as u16,
        start as u16,
        (start + width) as u16,
        Hit::Field(index),
    ));
    row.text(" ", "text", "input");
    let inner = width.saturating_sub(2);
    let mut caret = None;
    if text.is_empty() {
        if active {
            caret = Some(row.x);
        }
        row.text(&clip(placeholder, inner), "muted", "input");
    } else {
        let cursor = cursor.min(text.len());
        // Scroll so the caret stays inside the field.
        let mut begin = 0;
        if active {
            let mut used = 0;
            for (index, ch) in text[..cursor].char_indices().rev() {
                used += ch.width().unwrap_or(0);
                if used >= inner {
                    begin = index + ch.len_utf8();
                    break;
                }
            }
        }
        let selection = anchor
            .filter(|_| active)
            .map(|anchor| (anchor.min(cursor), anchor.max(cursor)));
        let limit = start + 1 + inner;
        for (index, ch) in text.char_indices().filter(|(index, _)| *index >= begin) {
            if active && index == cursor {
                caret = Some(row.x);
            }
            let w = ch.width().unwrap_or(0);
            if row.x + w > limit {
                break;
            }
            let selected = selection.is_some_and(|(a, b)| index >= a && index < b);
            let mut buffer = [0; 4];
            let ch = if ch.is_control() { ' ' } else { ch };
            row.text(
                ch.encode_utf8(&mut buffer),
                "text",
                if selected { "selection" } else { "input" },
            );
        }
        if active && cursor >= text.len() {
            caret = Some(row.x.min(limit));
        }
    }
    row.pad(start + width, "input");
    caret
}

fn group(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn human(size: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KiB", "MiB", "GiB", "TiB"];
    if size < 1024 {
        return format!("{size} {}", if size == 1 { "byte" } else { "bytes" });
    }
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn hex_digits(value: u64, width: usize, out: &mut [u8; 16]) -> &[u8] {
    for index in 0..width {
        out[width - 1 - index] = HEX[((value >> (index * 4)) & 15) as usize];
    }
    &out[..width]
}

impl HexEditor {
    pub fn render(&mut self, backend: &mut impl crate::editor::Backend) -> Result<Frame, String> {
        let info = backend.info(self.document)?;
        self.size = info.size;
        self.cursor = self.cursor.min(info.size);
        let layout = self.layout(info.size);
        let width = self.width as usize;
        let height = self.height as usize;
        let bpr = layout.bpr as u64;
        if self.reveal_cursor && self.cursor < self.top {
            self.top = self.cursor / bpr * bpr;
        }
        if self.reveal_cursor && self.cursor >= self.top + (layout.rows as u64 * bpr) {
            self.top = (self.cursor / bpr).saturating_sub(layout.rows as u64 - 1) * bpr;
        }
        self.reveal_cursor = false;
        self.top = (self.top / bpr * bpr).min(Self::max_top(info.size, &layout));
        let name = info
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("Binary file")
            .chars()
            .map(|ch| if ch.is_control() { '�' } else { ch })
            .collect::<String>();
        let title = Some(format!("{name} · Hex"));
        self.hits.clear();
        if height == 0 || width == 0 {
            return Ok(Frame {
                width: self.width,
                height: self.height,
                lines: vec![Vec::new(); height],
                document: Some(self.document),
                title,
                ..Frame::default()
            });
        }
        let visible = layout.bpr * layout.rows;
        let bytes = backend
            .read(self.document, self.top, visible)
            .unwrap_or_else(|error| {
                self.status = error;
                self.error = true;
                Vec::new()
            });
        let changed = if info.dirty {
            backend
                .changed(self.document, self.top, bytes.len())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut rows: Vec<Row> = (0..height).map(|_| Row::default()).collect();
        let mut caret = None;
        let hover = self.hover;
        let mut hits = std::mem::take(&mut self.hits);
        {
            let mut painter = Painter {
                hits: &mut hits,
                hover,
            };
            self.toolbar(&mut rows[0], &mut painter, &info, &name, width);
            if let Some(prompt) = self.prompt.clone() {
                for (offset, y) in
                    (1..1 + crate::editor::prompt_rows(&Some(prompt.clone()))).enumerate()
                {
                    if y + 1 >= height {
                        break;
                    }
                    let found =
                        self.prompt_row(&mut rows[y], &mut painter, &prompt, offset, y, width);
                    caret = caret.or(found.map(|x| (x, y)));
                }
            }
        }
        if layout.data_start >= 1 && layout.data_start - 1 < height.saturating_sub(1) {
            self.column_header(&mut rows[layout.data_start - 1], &layout);
        }
        let (start, end) = self.selection(info.size);
        let hovered_byte = hover
            .and_then(|(x, y)| self.byte_at(&layout, x as usize, y as usize))
            .map(|(offset, _)| offset)
            .filter(|offset| *offset < info.size && !self.dragging);
        let thumb = self.thumb(info.size, &layout);
        let thumb_hovered = hover.is_some_and(|(x, _)| x as usize == layout.scrollbar);
        for index in 0..layout.rows {
            let y = layout.data_start + index;
            if y + 1 >= height || layout.inspector_top.is_some_and(|top| y >= top) {
                break;
            }
            let row = &mut rows[y];
            let offset = self.top + (index * layout.bpr) as u64;
            let slice = bytes
                .get(index * layout.bpr..((index + 1) * layout.bpr).min(bytes.len()))
                .unwrap_or(&[]);
            let flags = changed
                .get(index * layout.bpr..((index + 1) * layout.bpr).min(changed.len()))
                .unwrap_or(&[]);
            if offset <= info.size {
                self.data_row(row, &layout, offset, slice, flags, start, end, hovered_byte);
            }
            if layout.scrollbar >= row.x && layout.scrollbar < width {
                row.pad(layout.scrollbar, "");
                let bg = match thumb {
                    Some((top, len)) if index >= top && index < top + len => {
                        if self.scroll_grab.is_some() {
                            "scrollbarThumbActive"
                        } else if thumb_hovered {
                            "scrollbarThumbHover"
                        } else {
                            "scrollbarThumb"
                        }
                    }
                    Some(_) => "scrollbar",
                    None => "",
                };
                row.text(" ", "text", bg);
                if thumb.is_some() {
                    hits.push((
                        y as u16,
                        layout.scrollbar as u16,
                        layout.scrollbar as u16 + 1,
                        Hit::Scrollbar,
                    ));
                }
            }
        }
        let sample = backend.read(self.document, start, 16).unwrap_or_default();
        {
            let mut painter = Painter {
                hits: &mut hits,
                hover,
            };
            if let Some(x) = layout.inspector_start {
                self.side_inspector(&mut rows, &mut painter, &layout, x, &sample, start, end);
            } else if let Some(top) = layout.inspector_top
                && top + 1 < height
            {
                self.bottom_inspector(&mut rows, &mut painter, top, &sample, width);
            }
        }
        self.hits = hits;
        if height > 1 {
            self.status_bar(&mut rows[height - 1], &info, width, start, end);
        }
        // Rows track their width as they are built; only overlong ones need clipping.
        let lines: Vec<Vec<Span>> = rows
            .into_iter()
            .map(|mut row| {
                if row.x > width {
                    clip_spans(&mut row.spans, width);
                }
                row.spans
            })
            .collect();
        let cursor = if self.prompt.is_some() {
            caret
        } else {
            let row = self.cursor.checked_sub(self.top).map(|delta| delta / bpr);
            row.filter(|row| (*row as usize) < layout.rows)
                .map(|row| row as usize + layout.data_start)
                .filter(|y| *y + 1 < height && layout.inspector_top.is_none_or(|top| *y < top))
                .map(|y| {
                    let column = (self.cursor % bpr) as usize;
                    let x = if self.pane == Pane::Hex {
                        layout.hex_x(column) + usize::from(self.low_nibble)
                    } else {
                        layout.ascii_start + column
                    };
                    (x, y)
                })
        }
        .filter(|(x, y)| *x < width && *y < height)
        .map(|(x, y)| (x as u16, y as u16));
        Ok(Frame {
            width: self.width,
            height: self.height,
            lines,
            cursor,
            document: Some(self.document),
            title,
            ..Frame::default()
        })
    }

    fn toolbar(
        &self,
        row: &mut Row,
        painter: &mut Painter,
        info: &DocumentInfo,
        name: &str,
        width: usize,
    ) {
        let surface = "header";
        let save = if info.read_only || !info.dirty {
            Look::Disabled
        } else {
            Look::Primary
        };
        let enabled = |on: bool| if on { Look::Normal } else { Look::Disabled };
        let seg = |on: bool| if on { Look::Primary } else { Look::Normal };
        let find_open = matches!(self.prompt, Some(Prompt::Find { .. }));
        let goto_open = matches!(self.prompt, Some(Prompt::Goto(_)));
        let left_full = [
            item("Save", "save", save, 0),
            item("Undo", "undo", enabled(info.can_undo), 1),
            item("Redo", "redo", enabled(info.can_redo), 1),
            item("Find", "find", seg(find_open), 2),
            item("Go to", "goto_line", seg(goto_open), 1),
        ];
        let mode = |short: bool| {
            if short {
                vec![
                    item("OVR", "overwrite_mode", seg(!self.insert), 0),
                    item("INS", "insert_mode", seg(self.insert), 0),
                ]
            } else {
                vec![
                    item("Overwrite", "overwrite_mode", seg(!self.insert), 0),
                    item("Insert", "insert_mode", seg(self.insert), 0),
                ]
            }
        };
        let inspector = item("Inspector", "inspector", seg(self.inspector), 2);
        let title = format!(" {name}");
        let title_width = title.width().min(28) + if info.dirty { 2 } else { 0 };
        // Try the full toolbar first, then drop the least important controls.
        let mut chosen = None;
        for level in 0..6 {
            let mut left: Vec<Item> = Vec::new();
            for (index, entry) in left_full.iter().enumerate() {
                let skip = (level >= 3 && matches!(index, 1 | 2)) || (level >= 5 && index == 4);
                if !skip {
                    left.push(item(entry.label, entry.hit, entry.look, entry.gap));
                }
            }
            let mut right = if level >= 4 {
                Vec::new()
            } else {
                mode(level >= 2)
            };
            if level < 1 {
                right.push(item(inspector.label, inspector.hit, inspector.look, 2));
            }
            let needed = title_width + 2 + items_width(&left) + 2 + items_width(&right) + 1;
            if needed <= width || level == 5 {
                chosen = Some((left, right));
                break;
            }
        }
        let (left, right) = chosen.unwrap_or_default();
        let title = clip(&title, 28);
        row.styled(&title, "text", surface, true);
        if info.dirty {
            row.text(" ●", "accent", surface);
        }
        if info.read_only {
            row.text("  read-only", "muted", surface);
        }
        row.pad(row.x + 2, surface);
        painter.items(row, 0, &left, surface);
        let right_width = items_width(&right);
        if !right.is_empty() && row.x + 2 + right_width < width {
            row.pad(width - right_width - 1, surface);
            painter.items(row, 0, &right, surface);
        }
        row.pad(width, surface);
    }

    /// Draws one row of the find or go-to bar; returns the caret column.
    fn prompt_row(
        &self,
        row: &mut Row,
        painter: &mut Painter,
        prompt: &Prompt,
        line: usize,
        y: usize,
        width: usize,
    ) -> Option<usize> {
        let surface = "panel";
        let seg = |on: bool| if on { Look::Primary } else { Look::Normal };
        let label = |row: &mut Row, text: &str| {
            row.text(" ", "muted", surface);
            row.text(text, "muted", surface);
            row.pad(10, surface);
        };
        let close = item("×", "close_prompt", Look::Normal, 1);
        let caret = match prompt {
            Prompt::Goto(input) => {
                label(row, "Go to");
                let buttons = [item("Go", "goto_submit", Look::Primary, 1)];
                let field_width = width.saturating_sub(10 + 6 + 30).clamp(12, 28);
                let caret = field(
                    row,
                    painter.hits,
                    y,
                    0,
                    input,
                    "offset, e.g. 4096 or 0x1000",
                    field_width,
                    true,
                    self.prompt_cursor,
                    self.prompt_anchor,
                );
                painter.items(row, y, &buttons, surface);
                row.pad(row.x + 2, surface);
                row.text("decimal, or 0x for hex", "muted", surface);
                caret
            }
            Prompt::Find {
                query,
                replacement,
                hex,
                replace,
                field: active,
            } => {
                if line == 0 {
                    label(row, "Find");
                    let modes = [
                        item("Text", "find_mode_text", seg(!*hex), 1),
                        item("Hex", "find_mode_hex", seg(*hex), 0),
                    ];
                    let navigate = [
                        item("Previous", "find_prev", Look::Normal, 2),
                        item("Next", "find_next", Look::Primary, 1),
                    ];
                    let toggle = [item("Replace", "toggle_replace", seg(*replace), 2)];
                    let fixed = 10
                        + items_width(&modes)
                        + items_width(&navigate)
                        + items_width(&toggle)
                        + 4
                        + 2
                        + if self.search.is_some() { 15 } else { 1 };
                    let field_width = width.saturating_sub(fixed).clamp(12, 40);
                    let caret = field(
                        row,
                        painter.hits,
                        y,
                        0,
                        query,
                        if *hex {
                            "hex bytes, e.g. DE AD BE EF"
                        } else {
                            "text"
                        },
                        field_width,
                        *active == 0,
                        self.prompt_cursor,
                        self.prompt_anchor,
                    );
                    painter.items(row, y, &modes, surface);
                    painter.items(row, y, &navigate, surface);
                    let progress = self
                        .search
                        .as_ref()
                        .filter(|task| !task.replacing())
                        .map(|task| format!("  Searching {}%", task.progress()));
                    if let Some(progress) = progress {
                        row.text(&progress, "accent", surface);
                    }
                    let tail =
                        items_width(&toggle) + items_width(&[item("×", "", Look::Normal, 1)]) + 1;
                    if row.x + tail <= width {
                        row.pad(width - tail, surface);
                        painter.items(row, y, &toggle, surface);
                        painter.items(row, y, &[close], surface);
                    }
                    caret
                } else {
                    label(row, "Replace");
                    let field_width = width.saturating_sub(10 + 30 + 2).clamp(12, 40);
                    let caret = field(
                        row,
                        painter.hits,
                        y,
                        1,
                        replacement,
                        if *hex {
                            "hex bytes, or empty to delete"
                        } else {
                            "text, or empty to delete"
                        },
                        field_width,
                        *active == 1,
                        self.prompt_cursor,
                        self.prompt_anchor,
                    );
                    painter.items(
                        row,
                        y,
                        &[
                            item("Replace", "replace_one", Look::Normal, 1),
                            item("Replace All", "replace_all", Look::Normal, 1),
                        ],
                        surface,
                    );
                    if let Some(task) = self.search.as_ref().filter(|task| task.replacing()) {
                        row.text(
                            &format!("  Replacing {}%", task.progress()),
                            "accent",
                            surface,
                        );
                    }
                    caret
                }
            }
        };
        row.pad(width, surface);
        caret
    }

    fn column_header(&self, row: &mut Row, layout: &Layout) {
        let column = (self.cursor % layout.bpr as u64) as usize;
        row.pad(layout.offset_start, "");
        row.text(&clip("Offset", layout.offset_width), "muted", "");
        for index in 0..layout.bpr {
            row.pad(layout.hex_x(index), "");
            let digits = [HEX[index >> 4], HEX[index & 15]];
            if index == column {
                row.styled(std::str::from_utf8(&digits).unwrap(), "accent", "", true);
            } else {
                row.chars(&digits, "muted", "");
            }
        }
        row.pad(layout.ascii_start, "");
        let label = if layout.bpr >= 12 {
            "Decoded text"
        } else {
            "Text"
        };
        row.text(&clip(label, layout.bpr), "muted", "");
    }

    #[allow(clippy::too_many_arguments)]
    fn data_row(
        &self,
        row: &mut Row,
        layout: &Layout,
        offset: u64,
        bytes: &[u8],
        changed: &[u8],
        start: u64,
        end: u64,
        hovered: Option<u64>,
    ) {
        let bpr = layout.bpr;
        let cursor_row = self.cursor / bpr as u64 == offset / bpr as u64;
        let mut digits = [0u8; 16];
        row.pad(layout.offset_start, "");
        let text = hex_digits(offset, layout.offset_width, &mut digits);
        if cursor_row {
            row.styled(std::str::from_utf8(text).unwrap(), "accent", "", true);
        } else {
            row.chars(text, "muted", "");
        }
        row.chars(b"  ", "text", "");
        let selecting = self.anchor.is_some_and(|anchor| anchor != self.cursor);
        let background = |absolute: u64, pane: Pane| -> &'static str {
            if !selecting && absolute == self.cursor {
                if pane == self.pane {
                    "cursor"
                } else {
                    "selection"
                }
            } else if selecting && absolute >= start && absolute < end {
                "selection"
            } else if hovered == Some(absolute) {
                "hover"
            } else {
                ""
            }
        };
        let joined = |absolute: u64| selecting && absolute > start && absolute < end;
        for column in 0..bpr {
            let absolute = offset + column as u64;
            if column > 0 {
                let gap: &[u8] = if column % 8 == 0 { b"  " } else { b" " };
                row.chars(gap, "text", if joined(absolute) { "selection" } else { "" });
            }
            let bg = background(absolute, Pane::Hex);
            match bytes.get(column) {
                Some(&byte) => {
                    let role = if changed.get(column) == Some(&1) {
                        "changed"
                    } else if byte == 0 {
                        "muted"
                    } else {
                        "text"
                    };
                    row.chars(
                        &[HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]],
                        role,
                        bg,
                    );
                }
                None => row.chars(b"  ", "text", if absolute == self.cursor { bg } else { "" }),
            }
        }
        row.chars(b"   ", "text", "");
        for column in 0..bpr {
            let absolute = offset + column as u64;
            let bg = background(absolute, Pane::Text);
            match bytes.get(column) {
                Some(&byte) => {
                    let printable = (32..127).contains(&byte);
                    let role = if changed.get(column) == Some(&1) {
                        "changed"
                    } else if printable {
                        "text"
                    } else {
                        "muted"
                    };
                    if printable {
                        row.chars(&[byte], role, bg);
                    } else {
                        row.text("·", role, bg);
                    }
                }
                None => row.chars(b" ", "text", if absolute == self.cursor { bg } else { "" }),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn side_inspector(
        &self,
        rows: &mut [Row],
        painter: &mut Painter,
        layout: &Layout,
        x: usize,
        sample: &[u8],
        start: u64,
        end: u64,
    ) {
        let height = rows.len();
        let surface = "panel";
        let top = layout.data_start.saturating_sub(1);
        let width = x + crate::editor::INSPECTOR_WIDTH;
        let inner = crate::editor::INSPECTOR_WIDTH - 2;
        let mut lines: Vec<(&str, String)> = vec![
            ("Offset", format!("0x{start:X}")),
            ("", group(start)),
            (
                "Selected",
                if end > start + 1 || self.anchor.is_some() {
                    let len = end - start;
                    format!("{} ({})", group(len), human(len))
                } else {
                    "1 byte".into()
                },
            ),
            ("", String::new()),
        ];
        lines.extend(inspect(sample, self.little_endian));
        for (y, row) in rows
            .iter_mut()
            .enumerate()
            .take(height.saturating_sub(1))
            .skip(top)
        {
            clip_row(row, x);
            row.pad(x, "");
            row.text(" ", "text", surface);
            if y == top {
                row.styled("Data inspector", "text", surface, true);
                let endian = [
                    item(
                        "LE",
                        "little_endian",
                        if self.little_endian {
                            Look::Primary
                        } else {
                            Look::Normal
                        },
                        0,
                    ),
                    item(
                        "BE",
                        "big_endian",
                        if self.little_endian {
                            Look::Normal
                        } else {
                            Look::Primary
                        },
                        0,
                    ),
                ];
                row.pad(width - items_width(&endian) - 1, surface);
                painter.items(row, y, &endian, surface);
            } else if let Some((label, value)) = lines.get(y - top - 1) {
                row.text(label, "muted", surface);
                row.pad(x + 11, surface);
                row.text(&clip(value, inner - 10), "text", surface);
            }
            row.pad(width, surface);
        }
    }

    fn bottom_inspector(
        &self,
        rows: &mut [Row],
        painter: &mut Painter,
        top: usize,
        sample: &[u8],
        width: usize,
    ) {
        let height = rows.len();
        let surface = "panel";
        for row in rows.iter_mut().take(height - 1).skip(top) {
            *row = Row::default();
        }
        {
            let row = &mut rows[top];
            row.text(" ", "text", surface);
            row.styled("Data inspector", "text", surface, true);
            let endian = [
                item(
                    "LE",
                    "little_endian",
                    if self.little_endian {
                        Look::Primary
                    } else {
                        Look::Normal
                    },
                    0,
                ),
                item(
                    "BE",
                    "big_endian",
                    if self.little_endian {
                        Look::Normal
                    } else {
                        Look::Primary
                    },
                    0,
                ),
            ];
            row.pad(row.x + 2, surface);
            painter.items(row, top, &endian, surface);
            row.pad(width, surface);
        }
        let values: Vec<(&str, String)> = inspect(sample, self.little_endian)
            .into_iter()
            .filter(|(label, _)| !label.is_empty())
            .collect();
        let columns = if width >= 96 {
            4
        } else if width >= 72 {
            3
        } else {
            2
        };
        let cell = width / columns;
        for (index, (label, value)) in values.iter().enumerate() {
            let y = top + 1 + index / columns;
            if y + 1 >= height {
                break;
            }
            let row = &mut rows[y];
            let x = (index % columns) * cell;
            row.pad(x, surface);
            row.text(" ", "text", surface);
            row.text(label, "muted", surface);
            row.pad(x + 9, surface);
            row.text(&clip(value, cell.saturating_sub(11)), "text", surface);
        }
        for row in rows.iter_mut().take(height - 1).skip(top + 1) {
            row.pad(width, surface);
        }
    }

    fn status_bar(&self, row: &mut Row, info: &DocumentInfo, width: usize, start: u64, end: u64) {
        let surface = "header";
        row.text(" ", "text", surface);
        if !self.status.is_empty() {
            let status = self
                .status
                .chars()
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .collect::<String>();
            row.text(
                &clip(&status, width.saturating_sub(2)),
                if self.error { "error" } else { "text" },
                surface,
            );
        } else {
            row.text("Offset ", "muted", surface);
            row.text(&format!("0x{:X}", self.cursor), "text", surface);
            row.text(&format!(" ({})", group(self.cursor)), "muted", surface);
            if self.anchor.is_some_and(|anchor| anchor != self.cursor) {
                row.text("   Selected ", "muted", surface);
                row.text(&group(end - start), "text", surface);
                row.text(
                    if end - start == 1 { " byte" } else { " bytes" },
                    "muted",
                    surface,
                );
            }
        }
        let right = format!(
            "{}{} · {} · {}",
            if info.read_only { "Read-only · " } else { "" },
            if self.pane == Pane::Hex {
                "Hex"
            } else {
                "Text"
            },
            if self.insert { "Insert" } else { "Overwrite" },
            human(info.size),
        );
        if row.x + right.width() + 3 <= width {
            row.pad(width - right.width() - 1, surface);
            row.text(&right, "muted", surface);
        }
        row.pad(width, surface);
    }
}

/// Typed values of the bytes at the selection start, as label and value.
pub fn inspect(bytes: &[u8], little: bool) -> Vec<(&'static str, String)> {
    let mut lines = vec![(
        "Binary",
        bytes.first().map_or("—".into(), |value| {
            format!("{:04b} {:04b}", value >> 4, value & 15)
        }),
    )];
    macro_rules! value {
        ($label:literal, $type:ty, $size:literal) => {{
            let value = bytes.get(..$size).map(|part| {
                let raw: [u8; $size] = part.try_into().unwrap();
                if little {
                    <$type>::from_le_bytes(raw)
                } else {
                    <$type>::from_be_bytes(raw)
                }
            });
            lines.push(($label, value.map_or("—".into(), |value| value.to_string())));
        }};
    }
    value!("UInt8", u8, 1);
    value!("Int8", i8, 1);
    value!("UInt16", u16, 2);
    value!("Int16", i16, 2);
    value!("UInt32", u32, 4);
    value!("Int32", i32, 4);
    value!("UInt64", u64, 8);
    value!("Int64", i64, 8);
    let float = |value: f64| {
        if value != 0.0 && (value.abs() >= 1e9 || value.abs() < 1e-4) {
            format!("{value:.6e}")
        } else {
            let text = format!("{value:.6}");
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    };
    lines.push((
        "Float32",
        bytes.get(..4).map_or("—".into(), |part| {
            let raw = part.try_into().unwrap();
            float(if little {
                f32::from_le_bytes(raw)
            } else {
                f32::from_be_bytes(raw)
            } as f64)
        }),
    ));
    lines.push((
        "Float64",
        bytes.get(..8).map_or("—".into(), |part| {
            let raw = part.try_into().unwrap();
            float(if little {
                f64::from_le_bytes(raw)
            } else {
                f64::from_be_bytes(raw)
            })
        }),
    ));
    let first_char = (1..=4).find_map(|len| {
        bytes
            .get(..len)
            .and_then(|part| std::str::from_utf8(part).ok())
            .and_then(|text| text.chars().next())
    });
    let show = |ch: Option<char>| {
        ch.filter(|ch| !ch.is_control() && ch.width().is_some_and(|w| w > 0))
            .map_or("—".into(), |ch| format!("{ch}  U+{:04X}", ch as u32))
    };
    lines.push(("UTF-8", show(first_char)));
    let utf16 = bytes.get(..2).and_then(|part| {
        let raw = part.try_into().unwrap();
        char::from_u32(if little {
            u16::from_le_bytes(raw)
        } else {
            u16::from_be_bytes(raw)
        } as u32)
    });
    lines.push(("UTF-16", show(utf16)));
    lines
}

fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.into();
    }
    let mut result = truncate(text, width.saturating_sub(1));
    if width > 0 {
        result.push('…');
    }
    result
}

fn truncate(text: &str, width: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|ch| {
            used += ch.width().unwrap_or(0);
            used <= width
        })
        .collect()
}

fn clip_spans(spans: &mut Vec<Span>, width: usize) {
    let mut remaining = width;
    spans.retain_mut(|span| {
        if remaining == 0 {
            return false;
        }
        let w = span.text.width();
        if w > remaining {
            span.text = truncate(&span.text, remaining);
        }
        remaining -= span.text.width().min(remaining);
        true
    });
}

fn clip_row(row: &mut Row, width: usize) {
    if row.x > width {
        clip_spans(&mut row.spans, width);
        row.x = row.spans.iter().map(|span| span.text.width()).sum();
    }
}
