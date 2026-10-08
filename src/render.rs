use crate::editor::{Backend, HexEditor, Pane, Prompt};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use vtcode_extension_api::editor::{Frame, Span};

impl HexEditor {
    pub fn render(&mut self, backend: &mut impl Backend) -> Result<Frame, String> {
        let info = backend.info(self.document)?;
        self.cursor = self.cursor.min(info.size);
        let layout = self.layout(info.size);
        let width = self.width as usize;
        let height = self.height as usize;
        let row_width = layout.bpr as u64;
        let focus = self.cursor;
        if self.reveal_cursor && focus < self.top {
            self.top = focus / row_width * row_width;
        }
        if self.reveal_cursor && focus >= self.top + (layout.rows * layout.bpr) as u64 {
            self.top = (focus / row_width).saturating_sub(layout.rows as u64 - 1) * row_width;
        }
        self.reveal_cursor = false;
        self.top = self.top.min(info.size / row_width * row_width);
        let bytes = backend
            .read(self.document, self.top, layout.bpr * layout.rows)
            .unwrap_or_else(|error| {
                self.status = error;
                self.error = true;
                Vec::new()
            });
        let changed = backend
            .changed(self.document, self.top, bytes.len())
            .unwrap_or_else(|error| {
                self.status = error;
                self.error = true;
                Vec::new()
            });
        let mut lines = vec![Vec::<Span>::new(); height];
        if height == 0 {
            return Ok(Frame {
                width: self.width,
                height: self.height,
                lines,
                document: Some(self.document),
                ..Frame::default()
            });
        }
        self.toolbar.clear();
        let name = info
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("Binary file")
            .chars()
            .map(|ch| if ch.is_control() { '�' } else { ch })
            .collect::<String>();
        let mut column = 0;
        let title = clip(
            &format!(" {}{} ", name, if info.dirty { " ●" } else { "" }),
            width.min(32),
        );
        lines[0].push(Span::new(title.clone(), "text").on("header").bold());
        column += title.width();
        let buttons = [
            ("Save", "save"),
            ("Find", "find"),
            ("Go to", "goto_line"),
            ("Inspector", "inspector"),
            (if self.little_endian { "LE" } else { "BE" }, "endian"),
            (if self.insert { "INS" } else { "OVR" }, "mode"),
        ];
        for (label, action) in buttons {
            let text = format!(" [{label}] ");
            let len = text.len();
            if column + len <= width {
                self.toolbar.push((column, column + len, action));
                lines[0].push(Span::new(text, "accent").on("header"));
                column += len;
            }
        }
        if column < width {
            lines[0].push(Span::new(" ".repeat(width - column), "text").on("header"));
        }
        if height > 1 {
            let mut header = format!("{:width$}  ", "OFFSET", width = layout.offset_width);
            for index in 0..layout.bpr {
                header.push_str(&format!("{index:02X} "));
                if index % 8 == 7 && index + 1 < layout.bpr {
                    header.push(' ');
                }
            }
            header.push_str("  DECODED TEXT");
            lines[1].push(Span::new(header, "muted"));
        }
        if let Some(prompt) = &self.prompt
            && height > 2
        {
            match prompt {
                Prompt::Goto(input) => {
                    lines[2] = vec![Span::new(" Go to offset: ", "accent")];
                    lines[2].extend(input_spans(
                        input,
                        true,
                        self.prompt_cursor,
                        self.prompt_anchor,
                        width.saturating_sub(15),
                    ));
                    lines[2].push(Span::new("  decimal or 0xHEX · Enter · Esc", "muted"));
                }
                Prompt::Find {
                    query,
                    replacement,
                    hex,
                    replace,
                    field,
                } => {
                    lines[2] = vec![Span::new(
                        if *hex {
                            " Find [HEX]: "
                        } else {
                            " Find [TEXT]: "
                        },
                        "accent",
                    )];
                    lines[2].extend(input_spans(
                        query,
                        *field == 0,
                        self.prompt_cursor,
                        self.prompt_anchor,
                        width.saturating_sub(15),
                    ));
                    lines[2].push(Span::new(
                        "  Enter next · Shift+Enter previous · Alt+H mode",
                        "muted",
                    ));
                    if *replace && height > 3 {
                        lines[3] = vec![Span::new(" Replace:    ", "accent")];
                        lines[3].extend(input_spans(
                            replacement,
                            *field == 1,
                            self.prompt_cursor,
                            self.prompt_anchor,
                            width.saturating_sub(15),
                        ));
                        lines[3].push(Span::new(
                            "  Tab field · Ctrl+Enter one · Ctrl+Shift+Enter all · Esc close",
                            "muted",
                        ));
                    }
                }
            }
        }
        let (selected_start, selected_end) = self.selection(info.size);
        for row in 0..layout.rows {
            let y = layout.data_start + row;
            if y >= height.saturating_sub(1) {
                break;
            }
            let offset = self.top + (row * layout.bpr) as u64;
            if offset > info.size {
                continue;
            }
            lines[y].push(Span::new(
                format!("{offset:0width$X}  ", width = layout.offset_width),
                "offset",
            ));
            for column in 0..layout.bpr {
                let absolute = offset + column as u64;
                let byte = bytes.get(row * layout.bpr + column);
                let selected = absolute >= selected_start && absolute < selected_end;
                let active = absolute == self.cursor && self.anchor.is_none();
                let bg = if active && self.pane == Pane::Hex {
                    "cursor"
                } else if selected || active {
                    "selection"
                } else {
                    ""
                };
                let role = if bg == "cursor" {
                    "cursor"
                } else if changed.get(row * layout.bpr + column) == Some(&1) {
                    "changed"
                } else if byte == Some(&0) {
                    "muted"
                } else {
                    "text"
                };
                lines[y].push(
                    Span::new(byte.map_or("  ".into(), |byte| format!("{byte:02X}")), role).on(bg),
                );
                lines[y].push(Span::new(" ", "text"));
                if column % 8 == 7 && column + 1 < layout.bpr {
                    lines[y].push(Span::new(" ", "text"));
                }
            }
            lines[y].push(Span::new("  ", "muted"));
            for column in 0..layout.bpr {
                let absolute = offset + column as u64;
                let byte = bytes.get(row * layout.bpr + column);
                let selected = absolute >= selected_start && absolute < selected_end;
                let active = absolute == self.cursor && self.anchor.is_none();
                let bg = if active && self.pane == Pane::Text {
                    "cursor"
                } else if selected || active {
                    "selection"
                } else {
                    ""
                };
                let text = byte.map_or(' ', |byte| {
                    if (32..127).contains(byte) {
                        *byte as char
                    } else {
                        '·'
                    }
                });
                lines[y].push(
                    Span::new(
                        text.to_string(),
                        if bg == "cursor" {
                            "cursor"
                        } else if changed.get(row * layout.bpr + column) == Some(&1) {
                            "changed"
                        } else {
                            "text"
                        },
                    )
                    .on(bg),
                );
            }
        }
        if let Some(start) = layout.inspector_start {
            let sample = backend
                .read(self.document, selected_start, 16)
                .unwrap_or_default();
            let inspector = inspect(&sample, self.little_endian);
            for (index, text) in inspector.into_iter().enumerate() {
                let y = index + 1;
                if y >= height.saturating_sub(1) {
                    break;
                }
                clip_spans(&mut lines[y], start);
                let used = lines[y].iter().map(|span| span.text.width()).sum::<usize>();
                if used < start {
                    lines[y].push(Span::new(" ".repeat(start - used), "text"));
                }
                lines[y].push(Span::new("│ ", "muted").on("panel"));
                let text = clip(&text, width.saturating_sub(start + 2));
                let len = text.width();
                lines[y]
                    .push(Span::new(text, if index == 0 { "accent" } else { "text" }).on("panel"));
                lines[y].push(
                    Span::new(" ".repeat(width.saturating_sub(start + 2 + len)), "text")
                        .on("panel"),
                );
            }
        }
        if let Some(top) = layout.inspector_top {
            let sample = backend
                .read(self.document, selected_start, 16)
                .unwrap_or_default();
            let inspector = inspect(&sample, self.little_endian);
            for line in lines.iter_mut().take(height - 1).skip(top) {
                *line = Vec::new();
            }
            lines[top] = vec![
                Span::new(
                    format!(
                        " DATA INSPECTOR · {} endian · Alt+E change · Alt+I hide",
                        if self.little_endian { "Little" } else { "Big" }
                    ),
                    "accent",
                )
                .on("panel"),
            ];
            let columns = if width >= 75 { 3 } else { 2 };
            let cell_width = width / columns;
            for (index, value) in inspector
                .iter()
                .skip(3)
                .filter(|text| !text.is_empty())
                .enumerate()
            {
                let y = top + 1 + index / columns;
                if y >= height - 1 {
                    break;
                }
                let text = clip(value, cell_width.saturating_sub(2));
                let padding = cell_width.saturating_sub(text.width() + 1);
                lines[y]
                    .push(Span::new(format!(" {text}{}", " ".repeat(padding)), "text").on("panel"));
            }
            for line in lines.iter_mut().take(height - 1).skip(top) {
                let used = line.iter().map(|span| span.text.width()).sum::<usize>();
                if used < width {
                    line.push(Span::new(" ".repeat(width - used), "text").on("panel"));
                }
            }
        }
        let footer = if !self.status.is_empty() {
            self.status.clone()
        } else {
            format!(
                " {} bytes │ Offset 0x{:X} ({}) │ {} │ {} │ Ctrl+F Find · Ctrl+G Go to · Tab Hex/Text",
                info.size,
                self.cursor,
                self.cursor,
                if info.read_only {
                    "READ ONLY"
                } else if self.insert {
                    "INSERT"
                } else {
                    "OVERWRITE"
                },
                if self.anchor.is_some() {
                    format!("{} selected", selected_end - selected_start)
                } else {
                    "Ready".into()
                }
            )
        };
        lines[height - 1] = vec![
            Span::new(
                footer
                    .chars()
                    .map(|ch| if ch.is_control() { ' ' } else { ch })
                    .collect::<String>(),
                if self.error { "error" } else { "muted" },
            )
            .on("header"),
        ];
        for line in &mut lines {
            clip_spans(line, width);
        }
        let row = ((self.cursor.saturating_sub(self.top)) / row_width) as usize + layout.data_start;
        let column = self.cursor as usize % layout.bpr;
        let x = if self.pane == Pane::Hex {
            layout.hex_start + column * 3 + column / 8 + usize::from(self.low_nibble)
        } else {
            layout.ascii_start + column
        };
        let cursor = (self.prompt.is_none()
            && self.cursor >= self.top
            && row < layout.data_start + layout.rows
            && row < height.saturating_sub(1)
            && x < width)
            .then_some((x as u16, row as u16));
        Ok(Frame {
            width: self.width,
            height: self.height,
            lines,
            cursor,
            document: Some(self.document),
            title: Some(format!("{name} · Hex Editor")),
        })
    }
}

pub fn inspect(bytes: &[u8], little: bool) -> Vec<String> {
    let mut lines = vec![
        "DATA INSPECTOR".into(),
        format!("{} endian  [Alt+E]", if little { "Little" } else { "Big" }),
        String::new(),
    ];
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
            lines.push(format!(
                "{:<9} {}",
                $label,
                value.map_or("—".into(), |value| value.to_string())
            ));
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
    for (label, value) in [
        (
            "Float32",
            bytes
                .get(..4)
                .map(|part| {
                    if little {
                        f32::from_le_bytes(part.try_into().unwrap())
                    } else {
                        f32::from_be_bytes(part.try_into().unwrap())
                    }
                })
                .map(|v| v as f64),
        ),
        (
            "Float64",
            bytes.get(..8).map(|part| {
                if little {
                    f64::from_le_bytes(part.try_into().unwrap())
                } else {
                    f64::from_be_bytes(part.try_into().unwrap())
                }
            }),
        ),
    ] {
        lines.push(format!(
            "{label:<9} {}",
            value.map_or("—".into(), |value| format!("{value:.6e}"))
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "Binary   {}",
        bytes
            .first()
            .map_or("—".into(), |value| format!("{value:08b}"))
    ));
    let first_char = (1..=4).find_map(|len| {
        bytes
            .get(..len)
            .and_then(|part| std::str::from_utf8(part).ok())
            .and_then(|text| text.chars().next())
    });
    lines.push(format!(
        "UTF-8    {}",
        first_char
            .filter(|ch| !ch.is_control())
            .map_or("—".into(), |ch| ch.to_string())
    ));
    let first_utf16 = bytes.get(..2).and_then(|part| {
        let value = if little {
            u16::from_le_bytes(part.try_into().unwrap())
        } else {
            u16::from_be_bytes(part.try_into().unwrap())
        };
        char::from_u32(value as u32)
    });
    lines.push(format!(
        "UTF-16   {}",
        first_utf16
            .filter(|ch| !ch.is_control())
            .map_or("—".into(), |ch| ch.to_string())
    ));
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
        span.text = truncate(&span.text, remaining);
        remaining -= span.text.width();
        true
    });
}

fn input_spans(
    text: &str,
    active: bool,
    cursor: usize,
    anchor: Option<usize>,
    width: usize,
) -> Vec<Span> {
    if !active {
        return vec![Span::new(clip(text, width), "text")];
    }
    let cursor = cursor.min(text.len());
    let begin = text[..cursor]
        .char_indices()
        .rev()
        .take(width.saturating_sub(2))
        .last()
        .map_or(0, |(index, _)| index);
    let selection = anchor.map(|anchor| (anchor.min(cursor), anchor.max(cursor)));
    let mut spans = Vec::new();
    if begin > 0 {
        spans.push(Span::new("…", "muted"));
    }
    for (index, ch) in text.char_indices().filter(|(index, _)| *index >= begin) {
        if index == cursor {
            spans.push(Span::new("▏", "accent"));
        }
        spans.push(
            Span::new(
                if ch.is_control() {
                    " ".into()
                } else {
                    ch.to_string()
                },
                "text",
            )
            .on(
                if selection.is_some_and(|(start, end)| index >= start && index < end) {
                    "selection"
                } else {
                    ""
                },
            ),
        );
    }
    if cursor == text.len() {
        spans.push(Span::new("▏", "accent"));
    }
    clip_spans(&mut spans, width);
    spans
}
