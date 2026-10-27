//! Platform glue for opening and saving maps.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::{Path, PathBuf};

    pub fn open() -> Result<Option<(PathBuf, String)>, String> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Metrocraft map", &["json"])
            .add_filter("JSON", &["json"])
            .pick_file()
        else {
            return Ok(None);
        };
        let contents = read(&path)?;
        Ok(Some((path, contents)))
    }

    pub fn read(path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path)
            .map_err(|err| format!("Could not read {}: {err}", path.display()))
    }

    /// Ask where a new map should live, without writing anything yet.
    pub fn choose_path(default_name: &str) -> Result<Option<PathBuf>, String> {
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Metrocraft map", &["json"])
            .set_file_name(default_name)
            .save_file()
        else {
            return Ok(None);
        };
        if path.extension().is_none() {
            path.set_extension("json");
        }
        Ok(Some(path))
    }

    pub fn save_as(default_name: &str, contents: &str) -> Result<Option<PathBuf>, String> {
        let Some(path) = choose_path(default_name)? else {
            return Ok(None);
        };
        write(&path, contents)?;
        Ok(Some(path))
    }

    pub fn write(path: &PathBuf, contents: &str) -> Result<(), String> {
        std::fs::write(path, contents)
            .map_err(|err| format!("Could not write {}: {err}", path.display()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{choose_path, open, read, save_as, write};

/// Save a JSON file with a browser download. Returns a human-readable label.
#[cfg(target_arch = "wasm32")]
pub fn download_json(filename: &str, contents: &str) -> Result<String, String> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or("no browser window")?;
    let document = window.document().ok_or("no document")?;
    let parts = js_sys::Array::new();
    parts.push(&wasm_bindgen::JsValue::from_str(contents));
    let blob = web_sys::Blob::new_with_str_sequence(&parts).map_err(|err| format!("{err:?}"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|err| format!("{err:?}"))?;

    let anchor = document
        .create_element("a")
        .map_err(|err| format!("{err:?}"))?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "could not create download link".to_owned())?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    if let Some(body) = document.body() {
        let _ = body.append_child(&anchor);
    }
    anchor.click();
    if let Some(parent) = anchor.parent_node() {
        let _ = parent.remove_child(&anchor);
    }
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(filename.to_owned())
}

/// Ask the browser for a JSON file. The result arrives on the returned
/// channel once the user picks a file.
#[cfg(target_arch = "wasm32")]
pub fn pick_json_file() -> Result<std::sync::mpsc::Receiver<Result<String, String>>, String> {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or("no browser window")?;
    let document = window.document().ok_or("no document")?;
    let input = document
        .create_element("input")
        .map_err(|err| format!("{err:?}"))?
        .dyn_into::<web_sys::HtmlInputElement>()
        .map_err(|_| "could not create file picker".to_owned())?;
    input.set_type("file");
    input.set_accept(".json,application/json");
    let _ = input.style().set_property("display", "none");
    if let Some(body) = document.body() {
        let _ = body.append_child(&input);
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(target) = event.target() else {
            return;
        };
        let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() else {
            return;
        };
        let Some(files) = input.files() else {
            return;
        };
        let Some(file) = files.get(0) else {
            return;
        };

        let reader = match web_sys::FileReader::new() {
            Ok(reader) => reader,
            Err(_) => {
                let _ = sender.send(Err("Could not open a file reader".to_owned()));
                return;
            }
        };
        let reader_for_load = reader.clone();
        let sender_for_load = sender.clone();
        let on_load = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            match reader_for_load.result() {
                Ok(value) => match value.as_string() {
                    Some(text) => {
                        let _ = sender_for_load.send(Ok(text));
                    }
                    None => {
                        let _ = sender_for_load.send(Err("That file is not text".to_owned()));
                    }
                },
                Err(_) => {
                    let _ = sender_for_load.send(Err("Could not read that file".to_owned()));
                }
            }
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        if reader.read_as_text(&file).is_err() {
            let _ = sender.send(Err("Could not start reading that file".to_owned()));
        }
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
    input.click();
    Ok(receiver)
}
