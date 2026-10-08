//! File-backed custom editor API. Document IDs are scoped to the extension.
//! Reads are limited to 1 MiB per call; file sizes and offsets are 64-bit.
use serde::{Deserialize, Serialize};

/// Actions accepted by `document_action`. Save returns false; undo/redo return
/// whether the document changed. Transactions group multiple edits into one undo.
pub mod action {
    pub const UNDO: u32 = 1;
    pub const REDO: u32 = 2;
    pub const SAVE: u32 = 3;
    pub const AMEND_NEXT_EDIT: u32 = 5;
    pub const BEGIN_TRANSACTION: u32 = 6;
    pub const COMMIT_TRANSACTION: u32 = 7;
    pub const ROLLBACK_TRANSACTION: u32 = 8;
}

#[link(wasm_import_module = "vtcode")]
#[cfg(target_arch = "wasm32")]
unsafe extern "C" {
    fn host_document_open() -> i32;
    fn host_document_info(id: i32, ptr: *mut u8, max: i32) -> i32;
    fn host_document_read(id: i32, offset: i64, ptr: *mut u8, max: i32) -> i32;
    fn host_document_changed(id: i32, offset: i64, ptr: *mut u8, max: i32) -> i32;
    fn host_document_edit(id: i32, offset: i64, deleted: i64, ptr: *const u8, len: i32) -> i32;
    fn host_document_action(id: i32, action: i32) -> i32;
    fn host_last_error(ptr: *mut u8, max: i32) -> i32;
    fn host_clipboard_read(ptr: *mut u8, max: i32) -> i32;
    fn host_clipboard_write(ptr: *const u8, len: i32) -> i32;
    fn host_register_view_resize_handler(
        id: *const u8,
        id_len: i32,
        export: *const u8,
        export_len: i32,
    );
    fn host_set_view_frame(id: *const u8, id_len: i32, json: *const u8, len: i32) -> i32;
    fn host_register_task_handler(export: *const u8, len: i32, interval_ms: i32);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInfo {
    pub path: String,
    pub size: u64,
    pub dirty: bool,
    pub read_only: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub lines: Vec<Vec<Span>>,
    pub cursor: Option<(u16, u16)>,
    pub document: Option<u32>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub background: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
}

impl Span {
    pub fn new(text: impl Into<String>, role: &str) -> Self {
        Self {
            text: text.into(),
            role: role.into(),
            ..Self::default()
        }
    }
    pub fn on(mut self, background: &str) -> Self {
        self.background = background.into();
        self
    }
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

#[cfg(target_arch = "wasm32")]
fn string_call(call: impl Fn(*mut u8, i32) -> i32) -> Result<String, String> {
    let size = call(std::ptr::null_mut(), 0);
    if size < 0 {
        return Err("Host string operation failed".into());
    }
    let mut bytes = vec![0; size as usize];
    let count = call(bytes.as_mut_ptr(), size);
    if count < 0 {
        return Err("Host string operation failed".into());
    }
    bytes.truncate(count as usize);
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn last_error() -> String {
    string_call(|ptr, max| unsafe { host_last_error(ptr, max) }).unwrap_or_else(|error| error)
}

#[cfg(target_arch = "wasm32")]
pub fn open_document() -> Result<u32, String> {
    let id = unsafe { host_document_open() };
    if id <= 0 {
        Err(last_error())
    } else {
        Ok(id as u32)
    }
}

#[cfg(target_arch = "wasm32")]
pub fn document_info(id: u32) -> Result<DocumentInfo, String> {
    let json = string_call(|ptr, max| unsafe { host_document_info(id as i32, ptr, max) })?;
    serde_json::from_str(&json).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn read_document(id: u32, offset: u64, count: usize) -> Result<Vec<u8>, String> {
    let offset = i64::try_from(offset).map_err(|_| "Offset exceeds the host API range")?;
    let mut bytes = vec![0; count.min(1024 * 1024)];
    let count =
        unsafe { host_document_read(id as i32, offset, bytes.as_mut_ptr(), bytes.len() as i32) };
    if count < 0 {
        return Err(last_error());
    }
    bytes.truncate(count as usize);
    Ok(bytes)
}

/// One flag per byte: 1 when different from the last saved document, 0 otherwise.
#[cfg(target_arch = "wasm32")]
pub fn changed_bytes(id: u32, offset: u64, count: usize) -> Result<Vec<u8>, String> {
    let offset = i64::try_from(offset).map_err(|_| "Offset exceeds the host API range")?;
    let mut bytes = vec![0; count.min(1024 * 1024)];
    let count =
        unsafe { host_document_changed(id as i32, offset, bytes.as_mut_ptr(), bytes.len() as i32) };
    if count < 0 {
        return Err(last_error());
    }
    bytes.truncate(count as usize);
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
pub fn edit_document(id: u32, offset: u64, deleted: u64, bytes: &[u8]) -> Result<(), String> {
    let offset = i64::try_from(offset).map_err(|_| "Offset exceeds the host API range")?;
    let deleted = i64::try_from(deleted).map_err(|_| "Range exceeds the host API range")?;
    if bytes.len() > 1024 * 1024 {
        return Err("Paste at most 1 MiB in one operation".into());
    }
    let result = unsafe {
        host_document_edit(
            id as i32,
            offset,
            deleted,
            bytes.as_ptr(),
            bytes.len() as i32,
        )
    };
    if result < 0 {
        Err(last_error())
    } else {
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
pub fn document_action(id: u32, action: u32) -> Result<bool, String> {
    let result = unsafe { host_document_action(id as i32, action as i32) };
    if result < 0 {
        Err(last_error())
    } else {
        Ok(result != 0)
    }
}

#[cfg(target_arch = "wasm32")]
pub fn clipboard() -> Result<String, String> {
    string_call(|ptr, max| unsafe { host_clipboard_read(ptr, max) })
}

#[cfg(target_arch = "wasm32")]
pub fn set_clipboard(text: &str) -> Result<(), String> {
    let result = unsafe { host_clipboard_write(text.as_ptr(), text.len() as i32) };
    if result < 0 {
        Err(last_error())
    } else {
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
pub fn register_resize_handler(id: &str, export: &str) {
    unsafe {
        host_register_view_resize_handler(
            id.as_ptr(),
            id.len() as i32,
            export.as_ptr(),
            export.len() as i32,
        )
    };
}

/// Schedule bounded work even while the terminal is idle or unfocused.
/// Unregister when the task completes; 0 cancels, positive intervals are at least 10 ms.
#[cfg(target_arch = "wasm32")]
pub fn register_task_handler(export: &str, interval_ms: u32) {
    unsafe {
        host_register_task_handler(
            export.as_ptr(),
            export.len() as i32,
            interval_ms.min(i32::MAX as u32) as i32,
        )
    };
}

#[cfg(target_arch = "wasm32")]
pub fn set_frame(id: &str, frame: &Frame) -> Result<(), String> {
    let json = serde_json::to_string(frame).map_err(|error| error.to_string())?;
    let result = unsafe {
        host_set_view_frame(
            id.as_ptr(),
            id.len() as i32,
            json.as_ptr(),
            json.len() as i32,
        )
    };
    if result < 0 {
        Err(last_error())
    } else {
        Ok(())
    }
}
