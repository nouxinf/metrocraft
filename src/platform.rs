//! Platform glue for opening and saving maps: files on the desktop, and the
//! browser's own database on the web.

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

    /// Ask for a Brand New Subway saved game, and read it.
    pub fn pick_game() -> Result<Option<(PathBuf, String)>, String> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Brand New Subway game", &["json"])
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
pub use native::{choose_path, open, pick_game, read, save_as, write};

/// A command key pressed while the browser's own text field had the keys.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwallowedKey {
    pub key: eframe::egui::Key,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

/// The desktop hands every key to egui, so there is nothing to watch for.
#[cfg(not(target_arch = "wasm32"))]
pub fn watch_swallowed_keys(_ctx: &eframe::egui::Context) {}

/// What the browser's project store answered.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreReply {
    /// Every name the store holds, in order.
    Names(Vec<String>),
    /// A map, and the name it is filed under.
    Opened(String, String),
    Saved(String),
    /// A map, moved from the name it had to the one it took.
    Renamed(String, String),
    Deleted(String),
}

#[cfg(target_arch = "wasm32")]
pub type StoreResult = std::sync::mpsc::Receiver<Result<StoreReply, String>>;

/// Every name the store holds.
#[cfg(target_arch = "wasm32")]
pub fn store_names(ctx: &eframe::egui::Context) -> StoreResult {
    deliver(ctx, async { Ok(StoreReply::Names(store::names().await?)) })
}

/// One saved map, by name.
#[cfg(target_arch = "wasm32")]
pub fn store_open(name: &str, ctx: &eframe::egui::Context) -> StoreResult {
    let name = name.to_owned();
    deliver(ctx, async move {
        let json = store::read(&name).await?;
        Ok(StoreReply::Opened(name, json))
    })
}

/// Keep a map in the browser, under its name.
#[cfg(target_arch = "wasm32")]
pub fn store_save(name: &str, json: &str, ctx: &eframe::egui::Context) -> StoreResult {
    let name = name.to_owned();
    let json = json.to_owned();
    deliver(ctx, async move {
        store::write(&name, &json).await?;
        Ok(StoreReply::Saved(name))
    })
}

/// Move a map to another name.
#[cfg(target_arch = "wasm32")]
pub fn store_rename(from: &str, to: &str, ctx: &eframe::egui::Context) -> StoreResult {
    let from = from.to_owned();
    let to = to.to_owned();
    deliver(ctx, async move {
        store::rename(&from, &to).await?;
        Ok(StoreReply::Renamed(from, to))
    })
}

/// Take a map out of the store.
#[cfg(target_arch = "wasm32")]
pub fn store_delete(name: &str, ctx: &eframe::egui::Context) -> StoreResult {
    let name = name.to_owned();
    deliver(ctx, async move {
        store::remove(&name).await?;
        Ok(StoreReply::Deleted(name))
    })
}

/// Run one store request and answer with what it said.
#[cfg(target_arch = "wasm32")]
fn deliver(
    ctx: &eframe::egui::Context,
    work: impl std::future::Future<Output = Result<StoreReply, String>> + 'static,
) -> StoreResult {
    let (sender, receiver) = std::sync::mpsc::channel();
    let repaint = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = sender.send(work.await);
        repaint.request_repaint();
    });
    receiver
}

/// The command keys egui never hears. While the browser's text field has the
/// focus — naming a station, say — eframe drops the keys before the app sees
/// them, so they are picked up here and the app reads them back each frame.
#[cfg(target_arch = "wasm32")]
mod swallowed {
    use std::cell::{Cell, RefCell};

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    use super::SwallowedKey;

    thread_local! {
        static KEYS: RefCell<Vec<SwallowedKey>> = const { RefCell::new(Vec::new()) };
        static WATCHING: Cell<bool> = const { Cell::new(false) };
    }

    pub fn keys() -> Vec<SwallowedKey> {
        KEYS.with(|keys| std::mem::take(&mut *keys.borrow_mut()))
    }

    pub fn watch(ctx: &eframe::egui::Context) {
        if WATCHING.with(Cell::get) {
            return;
        }
        WATCHING.with(|watching| watching.set(true));
        let Some(window) = web_sys::window() else {
            return;
        };
        let repaint = ctx.clone();
        let listener = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                let Some(document) = web_sys::window().and_then(|window| window.document()) else {
                    return;
                };
                // With the canvas in hand egui already has the key.
                let canvas = document.get_element_by_id("the_canvas_id");
                if document.active_element().is_some_and(|active| {
                    active.is_same_node(canvas.as_deref().map(|node| node as &web_sys::Node))
                }) {
                    return;
                }
                let command = event.ctrl_key() || event.meta_key();
                let alt = event.alt_key();
                // A bare key is typing, and belongs to whatever is being typed.
                if !command && !alt {
                    return;
                }
                let Some(key) = eframe::egui::Key::from_name(&event.key()) else {
                    return;
                };
                KEYS.with(|keys| {
                    keys.borrow_mut().push(SwallowedKey {
                        key,
                        command,
                        shift: event.shift_key(),
                        alt,
                    });
                });
                repaint.request_repaint();
            },
        );
        if window
            .add_event_listener_with_callback(
                "keydown",
                listener.as_ref().unchecked_ref::<js_sys::Function>(),
            )
            .is_err()
        {
            return;
        }
        listener.forget();
    }
}

#[cfg(target_arch = "wasm32")]
pub fn swallowed_keys() -> Vec<SwallowedKey> {
    swallowed::keys()
}

#[cfg(target_arch = "wasm32")]
pub fn watch_swallowed_keys(ctx: &eframe::egui::Context) {
    swallowed::watch(ctx);
}

/// The browser's own vault of maps, an IndexedDB store keyed by name.
#[cfg(target_arch = "wasm32")]
mod store {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{IdbDatabase, IdbTransaction, IdbTransactionMode};

    const DATABASE: &str = "metrocraft";
    const STORE: &str = "projects";

    fn describe(err: JsValue) -> String {
        err.as_string()
            .or_else(|| {
                err.dyn_ref::<web_sys::DomException>()
                    .map(|err| err.message())
            })
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "The browser refused to store that map.".to_owned())
    }

    /// One request as a promise that answers with its result.
    fn promise(request: &web_sys::IdbRequest) -> js_sys::Promise {
        js_sys::Promise::new(&mut |resolve, reject| {
            let resolve = resolve.clone();
            let source = request.clone();
            let success = Closure::once_into_js(move || {
                let _ = resolve.call1(&JsValue::NULL, &source.result().unwrap_or(JsValue::NULL));
            });
            request.set_onsuccess(Some(success.unchecked_ref()));

            let reject = reject.clone();
            let source = request.clone();
            let error = Closure::once_into_js(move || {
                let reason = source
                    .error()
                    .ok()
                    .flatten()
                    .map(|err| err.message())
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "The browser refused to store that map.".to_owned());
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&reason));
            });
            request.set_onerror(Some(error.unchecked_ref()));
        })
    }
    async fn open_at(version: Option<u32>) -> Result<IdbDatabase, String> {
        let factory = web_sys::window()
            .ok_or_else(|| "no browser window".to_owned())?
            .indexed_db()
            .map_err(describe)?
            .ok_or_else(|| "This browser keeps no project store.".to_owned())?;

        let request = match version {
            Some(version) => factory.open_with_u32(DATABASE, version),
            None => factory.open(DATABASE),
        }
        .map_err(describe)?;

        // The store is made the first time the database is opened, and again if
        // the database was ever made without it.
        let upgrading = request.clone();
        let upgrade = Closure::once_into_js(move || {
            if let Ok(value) = upgrading.result() {
                if let Some(database) = value.dyn_ref::<IdbDatabase>() {
                    if !database.object_store_names().contains(STORE) {
                        let _ = database.create_object_store(STORE);
                    }
                }
            }
        });
        request.set_onupgradeneeded(Some(upgrade.unchecked_ref()));

        JsFuture::from(promise(&request)).await.map_err(describe)?;
        request
            .result()
            .map_err(describe)?
            .dyn_into::<IdbDatabase>()
            .map_err(|_| "The project store did not open.".to_owned())
    }

    async fn open_database() -> Result<IdbDatabase, String> {
        let database = open_at(None).await?;
        if database.object_store_names().contains(STORE) {
            return Ok(database);
        }
        // A database without the store is one this app never made: opening it a
        // version higher makes the upgrade add the store, keeping what is there.
        let version = database.version() as u32 + 1;
        database.close();
        open_at(Some(version)).await
    }

    /// The store for one request, with the connection and the transaction kept
    /// alive for as long as the request runs.
    type Session = (IdbDatabase, IdbTransaction, web_sys::IdbObjectStore);

    async fn ready(mode: IdbTransactionMode) -> Result<Session, String> {
        let database = open_database().await?;
        let transaction = database
            .transaction_with_str_and_mode(STORE, mode)
            .map_err(describe)?;
        let store = transaction.object_store(STORE).map_err(describe)?;
        Ok((database, transaction, store))
    }

    pub async fn names() -> Result<Vec<String>, String> {
        let (database, transaction, store) = ready(IdbTransactionMode::Readonly).await?;
        let request = store.get_all_keys().map_err(describe)?;
        let keys = JsFuture::from(promise(&request)).await.map_err(describe)?;
        let keys = keys
            .dyn_into::<js_sys::Array>()
            .map_err(|_| "The project list could not be read.".to_owned())?;
        let names: Vec<String> = keys.iter().filter_map(|key| key.as_string()).collect();
        drop(transaction);
        database.close();
        Ok(names)
    }

    pub async fn read(name: &str) -> Result<String, String> {
        let (database, transaction, store) = ready(IdbTransactionMode::Readonly).await?;
        let request = store.get(&JsValue::from_str(name)).map_err(describe)?;
        let value = JsFuture::from(promise(&request)).await.map_err(describe)?;
        drop(transaction);
        database.close();
        value
            .as_string()
            .ok_or_else(|| "That map could not be read.".to_owned())
    }

    pub async fn write(name: &str, json: &str) -> Result<(), String> {
        let (database, transaction, store) = ready(IdbTransactionMode::Readwrite).await?;
        let request = store
            .put_with_key(&JsValue::from_str(json), &JsValue::from_str(name))
            .map_err(describe)?;
        JsFuture::from(promise(&request)).await.map_err(describe)?;
        drop(transaction);
        database.close();
        Ok(())
    }

    /// Move a map to another name, keeping what it holds.
    pub async fn rename(from: &str, to: &str) -> Result<(), String> {
        let (database, transaction, store) = ready(IdbTransactionMode::Readwrite).await?;
        let request = store.get(&JsValue::from_str(from)).map_err(describe)?;
        let value = JsFuture::from(promise(&request)).await.map_err(describe)?;
        // A map never saved has nothing to move: its name is all there is.
        if value.is_undefined() || value.is_null() {
            drop(transaction);
            database.close();
            return Ok(());
        }
        // The copy goes in first, so a rename that half fails leaves the map.
        let request = store
            .put_with_key(&value, &JsValue::from_str(to))
            .map_err(describe)?;
        JsFuture::from(promise(&request)).await.map_err(describe)?;
        let request = store.delete(&JsValue::from_str(from)).map_err(describe)?;
        JsFuture::from(promise(&request)).await.map_err(describe)?;
        drop(transaction);
        database.close();
        Ok(())
    }

    pub async fn remove(name: &str) -> Result<(), String> {
        let (database, transaction, store) = ready(IdbTransactionMode::Readwrite).await?;
        let request = store.delete(&JsValue::from_str(name)).map_err(describe)?;
        JsFuture::from(promise(&request)).await.map_err(describe)?;
        drop(transaction);
        database.close();
        Ok(())
    }
}

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
