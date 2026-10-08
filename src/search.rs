use crate::editor::{Backend, Event, HexEditor, Prompt, parse_hex};
use vtcode_extension_api::editor::DocumentInfo;

pub struct SearchTask {
    needle: Vec<u8>,
    prefix: Vec<usize>,
    ranges: std::collections::VecDeque<(u64, u64)>,
    backward: bool,
    size: u64,
    scanned: u64,
    replacement: Option<Vec<u8>>,
    replaced: u64,
}

impl SearchTask {
    fn new(needle: Vec<u8>, start: u64, size: u64, backward: bool) -> Self {
        let mut prefix = vec![0; needle.len()];
        for i in 1..needle.len() {
            let mut matched = prefix[i - 1];
            while matched > 0 && needle[matched] != needle[i] {
                matched = prefix[matched - 1];
            }
            if needle[matched] == needle[i] {
                matched += 1;
            }
            prefix[i] = matched;
        }
        let ranges = if backward {
            [(0, start), (start, size)]
        } else {
            [(start, size), (0, start)]
        };
        Self {
            needle,
            prefix,
            ranges: ranges.into(),
            backward,
            size,
            scanned: 0,
            replacement: None,
            replaced: 0,
        }
    }

    fn replace_step(&mut self, backend: &mut impl Backend, document: u32) -> Result<bool, String> {
        let Some(&(start, end)) = self.ranges.front() else {
            return Ok(true);
        };
        let count = (end - start).min(64 * 1024) as usize;
        if count == 0 {
            self.ranges.clear();
            return Ok(true);
        }
        let bytes = backend.read(
            document,
            start,
            (end - start).min(count as u64 + self.needle.len() as u64 - 1) as usize,
        )?;
        let replacement = self.replacement.as_ref().ok_or("Missing replacement")?;
        let mut output = Vec::new();
        let mut matched = 0;
        let mut copied = 0;
        let mut consumed = count;
        let mut matches = 0;
        for (index, byte) in bytes.iter().enumerate() {
            while matched > 0 && self.needle[matched] != *byte {
                matched = self.prefix[matched - 1];
            }
            if self.needle[matched] == *byte {
                matched += 1;
            }
            if matched == self.needle.len() {
                let found = index + 1 - matched;
                if found >= count {
                    break;
                }
                output.extend_from_slice(&bytes[copied..found]);
                output.extend_from_slice(replacement);
                copied = index + 1;
                consumed = consumed.max(copied);
                matched = 0; // Replacement matches never overlap or scan inserted bytes.
                matches += 1;
                if output.len() >= 64 * 1024 {
                    consumed = copied;
                    break;
                }
            }
        }
        let inserted = if matches > 0 {
            output.extend_from_slice(&bytes[copied..consumed]);
            backend.edit(document, start, consumed as u64, &output)?;
            output.len() as u64
        } else {
            consumed as u64
        };
        self.replaced += matches;
        self.scanned += consumed as u64;
        let end = end - consumed as u64 + inserted;
        let next = start + inserted;
        self.ranges[0] = (next, end);
        if next == end {
            self.ranges.clear();
        }
        Ok(self.ranges.is_empty())
    }

    fn step(&mut self, backend: &mut impl Backend, document: u32) -> Result<Option<u64>, String> {
        let Some((start, end)) = self.ranges.front_mut() else {
            return Ok(None);
        };
        let (begin, limit) = if self.backward {
            (end.saturating_sub(64 * 1024).max(*start), *end)
        } else {
            (*start, start.saturating_add(64 * 1024).min(*end))
        };
        let count = (limit
            .saturating_add(self.needle.len() as u64 - 1)
            .min(self.size)
            - begin) as usize;
        let bytes = backend.read(document, begin, count)?;
        let mut matched = 0;
        let mut found = None;
        for (index, byte) in bytes.iter().enumerate() {
            while matched > 0 && self.needle[matched] != *byte {
                matched = self.prefix[matched - 1];
            }
            if self.needle[matched] == *byte {
                matched += 1;
            }
            if matched == self.needle.len() {
                let offset = begin + (index + 1 - matched) as u64;
                if offset < limit {
                    found = Some(offset);
                    if !self.backward {
                        break;
                    }
                }
                matched = self.prefix[matched - 1];
            }
        }
        self.scanned += limit - begin;
        if self.backward {
            *end = begin;
        } else {
            *start = limit;
        }
        if start == end {
            self.ranges.pop_front();
        }
        Ok(found)
    }
}

impl HexEditor {
    pub(crate) fn prompt_key(
        &mut self,
        event: Event,
        backend: &mut impl Backend,
        info: &DocumentInfo,
    ) -> Result<(), String> {
        if event.code == "escape" {
            self.prompt = None;
            self.status.clear();
            return Ok(());
        }
        if event.alt && event.text.eq_ignore_ascii_case("h") {
            if let Some(Prompt::Find { hex, .. }) = &mut self.prompt {
                *hex = !*hex;
            }
            return Ok(());
        }
        if event.code == "tab" || event.code == "backTab" {
            if let Some(Prompt::Find {
                replace: true,
                field,
                query,
                replacement,
                ..
            }) = &mut self.prompt
            {
                *field = 1 - *field;
                self.prompt_cursor = if *field == 0 {
                    query.len()
                } else {
                    replacement.len()
                };
                self.prompt_anchor = None;
            }
            return Ok(());
        }
        if event.code == "enter" {
            match self.prompt.clone() {
                Some(Prompt::Goto(input)) => {
                    let input = input.trim();
                    let offset = if let Some(hex) = input
                        .strip_prefix("0x")
                        .or_else(|| input.strip_prefix("0X"))
                    {
                        u64::from_str_radix(hex, 16)
                    } else {
                        input.parse::<u64>()
                    }
                    .map_err(
                        |_| "Enter a decimal offset or a hexadecimal offset beginning with 0x",
                    )?;
                    if offset > info.size {
                        return Err(format!(
                            "Offset must be between 0 and {} (0x{:X})",
                            info.size, info.size
                        ));
                    }
                    self.move_cursor(offset);
                    self.prompt = None;
                    self.status.clear();
                }
                Some(Prompt::Find {
                    query,
                    replacement,
                    hex,
                    replace,
                    ..
                }) => {
                    self.last_find = Some((query.clone(), hex));
                    let needle = if hex {
                        parse_hex(&query)?
                    } else {
                        query.into_bytes()
                    };
                    if needle.is_empty() {
                        return Err("Enter something to find".into());
                    }
                    if needle.len() > 4096 {
                        return Err("Search for at most 4096 bytes at once".into());
                    }
                    if event.control && event.shift && replace {
                        let replacement = if replacement.is_empty() {
                            Vec::new()
                        } else if hex {
                            parse_hex(&replacement)?
                        } else {
                            replacement.into_bytes()
                        };
                        backend.action(self.document, 6)?;
                        let mut task = SearchTask::new(needle, 0, info.size, false);
                        task.ranges = [(0, info.size)].into();
                        task.replacement = Some(replacement);
                        self.search = Some(task);
                        self.search_step(backend)?;
                    } else if event.control && replace {
                        let replacement = if replacement.is_empty() {
                            Vec::new()
                        } else if hex {
                            parse_hex(&replacement)?
                        } else {
                            replacement.into_bytes()
                        };
                        let (start, end) = self.selection(info.size);
                        let current = backend.read(self.document, start, needle.len())?;
                        if current == needle && end - start == needle.len() as u64 {
                            backend.edit(
                                self.document,
                                start,
                                needle.len() as u64,
                                &replacement,
                            )?;
                            self.move_cursor(start + replacement.len() as u64);
                            self.status = "Replaced one match".into();
                        } else {
                            self.status = "Find a matching selection before replacing".into();
                        }
                    } else {
                        let start = if event.shift {
                            self.selection(info.size).0
                        } else {
                            if self.anchor.is_some() {
                                self.selection(info.size).1
                            } else {
                                self.cursor.min(info.size)
                            }
                        };
                        self.search = Some(SearchTask::new(needle, start, info.size, event.shift));
                        self.search_step(backend)?;
                    }
                }
                None => {}
            }
            return Ok(());
        }
        let input = match &mut self.prompt {
            Some(Prompt::Goto(input)) => input,
            Some(Prompt::Find {
                query,
                replacement,
                field,
                ..
            }) => {
                if *field == 0 {
                    query
                } else {
                    replacement
                }
            }
            None => return Ok(()),
        };
        self.prompt_cursor = self.prompt_cursor.min(input.len());
        while !input.is_char_boundary(self.prompt_cursor) {
            self.prompt_cursor -= 1;
        }
        let previous = input[..self.prompt_cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
        let next = input[self.prompt_cursor..]
            .chars()
            .next()
            .map_or(input.len(), |ch| self.prompt_cursor + ch.len_utf8());
        let selection = self
            .prompt_anchor
            .filter(|anchor| *anchor != self.prompt_cursor)
            .map(|anchor| {
                (
                    anchor.min(self.prompt_cursor).min(input.len()),
                    anchor.max(self.prompt_cursor).min(input.len()),
                )
            });
        let command = if event.code == "command" {
            event.text.as_str()
        } else if event.control {
            match event.text.to_ascii_lowercase().as_str() {
                "a" => "select_all",
                "c" => "copy",
                "x" => "cut",
                "v" => "paste",
                _ => "",
            }
        } else {
            ""
        };
        let target = match event.code.as_str() {
            "left" => Some(if event.control { 0 } else { previous }),
            "right" => Some(if event.control { input.len() } else { next }),
            "home" => Some(0),
            "end" => Some(input.len()),
            _ => None,
        };
        if let Some(target) = target {
            if event.shift {
                self.prompt_anchor.get_or_insert(self.prompt_cursor);
            } else {
                self.prompt_anchor = None;
            }
            self.prompt_cursor = target;
            return Ok(());
        }
        if command == "select_all" {
            self.prompt_anchor = Some(0);
            self.prompt_cursor = input.len();
            return Ok(());
        }
        if matches!(command, "copy" | "cut") {
            if let Some((start, end)) = selection {
                backend.copy(&input[start..end])?;
                if command == "cut" {
                    input.replace_range(start..end, "");
                    self.prompt_cursor = start;
                    self.prompt_anchor = None;
                }
            }
            return Ok(());
        }
        let replacement = if event.code == "paste" {
            Some(event.text.clone())
        } else if command == "paste" {
            Some(backend.clipboard()?)
        } else if event.code == "char" && !event.control && !event.alt {
            Some(event.text)
        } else {
            None
        };
        if let Some(text) = replacement {
            let (start, end) = selection.unwrap_or((self.prompt_cursor, self.prompt_cursor));
            if input.len() - (end - start) + text.len() > 8192 {
                return Err("Input is limited to 8192 bytes".into());
            }
            input.replace_range(start..end, &text);
            self.prompt_cursor = start + text.len();
            self.prompt_anchor = None;
        } else if matches!(event.code.as_str(), "backspace" | "delete") {
            let (start, end) = selection.unwrap_or(if event.code == "backspace" {
                (previous, self.prompt_cursor)
            } else {
                (self.prompt_cursor, next)
            });
            input.replace_range(start..end, "");
            self.prompt_cursor = start;
            self.prompt_anchor = None;
        }
        Ok(())
    }

    fn move_cursor(&mut self, offset: u64) {
        self.cursor = offset;
        self.anchor = None;
        self.low_nibble = false;
    }

    pub fn search_step(&mut self, backend: &mut impl Backend) -> Result<(), String> {
        let Some(task) = self.search.as_mut() else {
            return Ok(());
        };
        if task.replacement.is_some() {
            let done = match task.replace_step(backend, self.document) {
                Ok(done) => done,
                Err(error) => {
                    self.search = None;
                    let _ = backend.action(self.document, 8);
                    return Err(error);
                }
            };
            if done {
                let count = task.replaced;
                backend.action(self.document, 7)?;
                self.search = None;
                self.cursor = self.cursor.min(backend.info(self.document)?.size);
                self.anchor = None;
                self.status = format!(
                    "Replaced {count} match{} · Ctrl+Z to undo",
                    if count == 1 { "" } else { "es" }
                );
            } else {
                self.status = format!(
                    "Replacing… {}% · {} matches · Esc cancels all changes",
                    task.scanned
                        .saturating_mul(100)
                        .checked_div(task.size)
                        .unwrap_or(100),
                    task.replaced
                );
            }
            return Ok(());
        }
        let found = task.step(backend, self.document)?;
        if let Some(offset) = found {
            let len = task.needle.len();
            self.anchor = Some(offset + len as u64);
            self.cursor = offset;
            self.status = format!("Match at 0x{offset:X} · {len} bytes");
            self.reveal_cursor = true;
            self.search = None;
        } else if task.ranges.is_empty() {
            self.status = "No matches".into();
            self.search = None;
        } else {
            self.status = format!(
                "Searching… {}% · Esc to cancel",
                task.scanned
                    .saturating_mul(100)
                    .checked_div(task.size)
                    .unwrap_or(100)
            );
        }
        Ok(())
    }

    pub(crate) fn cancel_search(&mut self, backend: &mut impl Backend) -> Result<(), String> {
        if self
            .search
            .take()
            .is_some_and(|task| task.replacement.is_some())
        {
            backend.action(self.document, 8)?;
        }
        Ok(())
    }
}

pub fn find_range(
    backend: &mut impl Backend,
    id: u32,
    needle: &[u8],
    start: u64,
    end: u64,
) -> Result<Option<u64>, String> {
    if needle.is_empty() {
        return Ok(None);
    }
    let mut offset = start;
    while offset < end {
        let count = (end - offset).min(64 * 1024 + needle.len() as u64 - 1) as usize;
        let chunk = backend.read(id, offset, count)?;
        if let Some(index) = chunk
            .windows(needle.len())
            .position(|window| window == needle)
        {
            return Ok(Some(offset + index as u64));
        }
        if chunk.len() < needle.len() || count <= needle.len() {
            break;
        }
        offset += (chunk.len() - needle.len() + 1) as u64;
    }
    Ok(None)
}

pub fn find_backward(
    backend: &mut impl Backend,
    id: u32,
    needle: &[u8],
    before: u64,
    size: u64,
) -> Result<Option<u64>, String> {
    for (mut end, floor) in [(before.min(size), 0), (size, before.min(size))] {
        while end > floor {
            let start = end.saturating_sub(64 * 1024).max(floor);
            let count = (end.saturating_add(needle.len() as u64 - 1).min(size) - start) as usize;
            let chunk = backend.read(id, start, count)?;
            if let Some(index) = chunk
                .windows(needle.len())
                .rposition(|window| window == needle)
            {
                let found = start + index as u64;
                if found < end {
                    return Ok(Some(found));
                }
            }
            end = start;
        }
    }
    Ok(None)
}
