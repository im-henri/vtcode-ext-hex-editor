pub mod editor;
pub mod render;
pub mod search;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
mod guest {
    use crate::editor::{Backend, Event, HexEditor};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use vtcode_extension_api::{self as api, editor as host};

    thread_local! { static EDITORS: RefCell<HashMap<u32, HexEditor>> = RefCell::new(HashMap::new()); }
    struct Host;
    impl Backend for Host {
        fn info(&mut self, id: u32) -> Result<host::DocumentInfo, String> {
            host::document_info(id)
        }
        fn read(&mut self, id: u32, offset: u64, count: usize) -> Result<Vec<u8>, String> {
            host::read_document(id, offset, count)
        }
        fn changed(&mut self, id: u32, offset: u64, count: usize) -> Result<Vec<u8>, String> {
            host::changed_bytes(id, offset, count)
        }
        fn edit(&mut self, id: u32, offset: u64, delete: u64, bytes: &[u8]) -> Result<(), String> {
            host::edit_document(id, offset, delete, bytes)
        }
        fn action(&mut self, id: u32, action: u32) -> Result<bool, String> {
            host::document_action(id, action)
        }
        fn clipboard(&mut self) -> Result<String, String> {
            host::clipboard()
        }
        fn copy(&mut self, text: &str) -> Result<(), String> {
            host::set_clipboard(text)
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn vtcode_activate() -> i32 {
        api::register_command("hex.open", "Reopen File as Hex");
        api::register_explorer_context_command("hex.open", "Open With Hex Editor");
        api::register_binary_open_command("hex.open", "Hex Editor");
        api::set_status("Ready");
        0
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn vtcode_deactivate() -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn vtcode_run_command(index: i32) -> i32 {
        if index != 0 {
            return 1;
        }
        let result = (|| -> Result<(), String> {
            let id = host::open_document()?;
            let info = host::document_info(id)?;
            let name = info
                .path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("Binary file");
            let view = format!("hex:{id}");
            api::register_view(&view, &format!("{name} · Hex Editor"), "");
            api::register_view_key_handler(&view, "hex_event");
            api::register_view_mouse_handler(&view, "hex_event");
            api::register_view_scroll_handler(&view, "hex_event");
            host::register_resize_handler(&view, "hex_event");
            EDITORS.with(|editors| -> Result<(), String> {
                let mut editors = editors.borrow_mut();
                let editor = editors.entry(id).or_insert_with(|| HexEditor::new(id));
                host::set_frame(&view, &editor.render(&mut Host)?)
            })?;
            api::open_view(&view);
            Ok(())
        })();
        if let Err(error) = result {
            api::set_status(&error);
            api::log(&error);
            return 1;
        }
        0
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn hex_event() -> i32 {
        let result = (|| -> Result<(), String> {
            let json = api::current_view_event().ok_or("Missing editor event")?;
            let value: serde_json::Value =
                serde_json::from_str(&json).map_err(|error| error.to_string())?;
            let view = value["viewId"].as_str().ok_or("Missing view id")?;
            let id = view
                .strip_prefix("hex:")
                .ok_or("Invalid hex view")?
                .parse::<u32>()
                .map_err(|error| error.to_string())?;
            let event: Event = serde_json::from_str(&json).map_err(|error| error.to_string())?;
            EDITORS.with(|editors| -> Result<(), String> {
                let mut editors = editors.borrow_mut();
                let editor = editors.get_mut(&id).ok_or("The hex editor has closed")?;
                editor.event(event, &mut Host);
                if editors.values().any(|editor| editor.search.is_some()) {
                    host::register_task_handler("hex_tick", 16);
                } else {
                    api::unregister_tick_handler("hex_tick");
                }
                let editor = editors.get_mut(&id).ok_or("The hex editor has closed")?;
                host::set_frame(view, &editor.render(&mut Host)?)
            })
        })();
        if let Err(error) = result {
            api::set_status(&error);
            api::log(&error);
            return 2;
        }
        1
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn hex_tick() -> i32 {
        EDITORS.with(|editors| {
            let mut editors = editors.borrow_mut();
            if let Some((id, editor)) = editors
                .iter_mut()
                .find(|(_, editor)| editor.search.is_some())
            {
                if let Err(error) = editor.search_step(&mut Host) {
                    editor.search = None;
                    editor.status = error;
                    editor.error = true;
                }
                if let Ok(frame) = editor.render(&mut Host) {
                    let _ = host::set_frame(&format!("hex:{id}"), &frame);
                }
            }
            if !editors.values().any(|editor| editor.search.is_some()) {
                api::unregister_tick_handler("hex_tick");
            }
        });
        0
    }
}
