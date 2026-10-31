//! The window's own menu bar, drawn by Windows above the client area rather
//! than inside it, so the map keeps the whole frame.

use std::sync::Mutex;

use eframe::egui;
use muda::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, MenuTheme, PredefinedMenuItem, Submenu,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// What a menu item asks for, in the terms the app's own menu bar uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    New,
    Open,
    Import,
    ImportGame,
    Save,
    SaveAs,
    Rename,
    Projects,
    ClearRecent,
    Undo,
    Redo,
    Preferences,
    Fit,
    Waypoints,
    Stats,
    /// The map at this place in the recent list.
    Recent(usize),
}

/// Every item the bar carries, in the order it is made: its id, its command,
/// and the words it says.
const ITEMS: [(&str, Command, &str); 15] = [
    ("new", Command::New, "New"),
    ("open", Command::Open, "Open…"),
    ("import", Command::Import, "Import from MetroDreamin'…"),
    (
        "import-game",
        Command::ImportGame,
        "Import from Brand New Subway…",
    ),
    ("save", Command::Save, "Save"),
    ("save-as", Command::SaveAs, "Save as…"),
    ("rename", Command::Rename, "Rename…"),
    ("projects", Command::Projects, "Projects"),
    ("clear-recent", Command::ClearRecent, "Clear Recent"),
    ("undo", Command::Undo, "Undo"),
    ("redo", Command::Redo, "Redo"),
    ("preferences", Command::Preferences, "Preferences…"),
    ("fit", Command::Fit, "Fit to map"),
    ("waypoints", Command::Waypoints, "Waypoints"),
    ("stats", Command::Stats, "Network stats"),
];

/// The items marked as on or off.
fn is_check(command: Command) -> bool {
    matches!(command, Command::Waypoints | Command::Stats)
}

/// The items that need a document in hand.
fn needs_map(command: Command) -> bool {
    matches!(
        command,
        Command::Save
            | Command::SaveAs
            | Command::Rename
            | Command::Undo
            | Command::Redo
            | Command::Fit
    )
}

/// The items pressed since the last frame, waiting for it.
static PRESSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

enum Item {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

impl Item {
    fn as_menu_item(&self) -> &dyn IsMenuItem {
        match self {
            Self::Plain(item) => item,
            Self::Check(item) => item,
        }
    }

    fn set_enabled(&self, enabled: bool) {
        match self {
            Self::Plain(item) => item.set_enabled(enabled),
            Self::Check(item) => item.set_enabled(enabled),
        }
    }

    fn set_text(&self, text: &str) {
        match self {
            Self::Plain(item) => item.set_text(text),
            Self::Check(item) => item.set_text(text),
        }
    }

    fn set_checked(&self, checked: bool) {
        if let Self::Check(item) = self {
            item.set_checked(checked);
        }
    }
}

/// One item, and what it has been told to say.
struct Entry {
    command: Command,
    item: Item,
    /// The item's own words, without the key beside them.
    label: &'static str,
    shown: String,
}

pub struct NativeMenu {
    menu: Menu,
    hwnd: isize,
    /// The submenu the recent maps are listed in, and the maps it lists.
    file: Submenu,
    recent: Submenu,
    recent_shown: Vec<String>,
    entries: Vec<Entry>,
    /// The theme the bar was last given, since theming it is heavier than
    /// setting a flag.
    dark: Option<bool>,
}

impl NativeMenu {
    /// Put a menu bar on the window, listing `recent` under `Open Recent`.
    /// `None` where there is no window to give it to, which leaves the app
    /// drawing its own.
    pub fn install(frame: &eframe::Frame, ctx: &egui::Context, recent: &[String]) -> Option<Self> {
        let hwnd = match frame.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => handle.hwnd.get(),
            _ => return None,
        };

        let mut entries: Vec<Entry> = Vec::new();
        for (id, command, label) in ITEMS {
            let item = if is_check(command) {
                Item::Check(CheckMenuItem::with_id(id, label, true, false, None))
            } else {
                Item::Plain(MenuItem::with_id(id, label, true, None))
            };
            entries.push(Entry {
                command,
                item,
                label,
                shown: label.to_owned(),
            });
        }
        let item = |command: Command| -> &dyn IsMenuItem {
            entries
                .iter()
                .find(|entry| entry.command == command)
                .map_or_else(
                    || unreachable!("every command has an item"),
                    |entry| entry.item.as_menu_item(),
                )
        };

        // The maps opened lately, as `Open Recent` lists them. An empty list
        // leaves the submenu greyed, the way Windows shows one.
        let listed: Vec<MenuItem> = recent
            .iter()
            .enumerate()
            .map(|(index, name)| MenuItem::with_id(format!("recent:{index}"), name, true, None))
            .collect();
        let borrowed: Vec<&dyn IsMenuItem> = listed
            .iter()
            .map(|entry| entry as &dyn IsMenuItem)
            .collect();
        let recent_menu =
            Submenu::with_items("Open &Recent", !listed.is_empty(), &borrowed).ok()?;

        let (before_save, before_projects) = (
            PredefinedMenuItem::separator(),
            PredefinedMenuItem::separator(),
        );
        let file = Submenu::with_items(
            "&File",
            true,
            &[
                item(Command::New),
                item(Command::Open),
                &recent_menu,
                item(Command::Import),
                item(Command::ImportGame),
                &before_save,
                item(Command::Save),
                item(Command::SaveAs),
                item(Command::Rename),
                &before_projects,
                item(Command::Projects),
                item(Command::ClearRecent),
            ],
        )
        .ok()?;
        let edit_separator = PredefinedMenuItem::separator();
        let edit = Submenu::with_items(
            "&Edit",
            true,
            &[
                item(Command::Undo),
                item(Command::Redo),
                &edit_separator,
                item(Command::Preferences),
            ],
        )
        .ok()?;
        let view = Submenu::with_items(
            "&View",
            true,
            &[item(Command::Fit), item(Command::Waypoints)],
        )
        .ok()?;
        let tools = Submenu::with_items("&Tools", true, &[item(Command::Stats)]).ok()?;

        let menu = Menu::new();
        menu.append_items(&[&file, &edit, &view, &tools]).ok()?;

        // SAFETY: the handle is the window the app draws in, and it outlives the
        // menu bar it is given.
        unsafe { menu.init_for_hwnd(hwnd) }.ok()?;

        // A press arrives on the window's own thread, so it waits here for the
        // frame that acts on it.
        MenuEvent::set_event_handler(Some({
            let ctx = ctx.clone();
            move |event: MenuEvent| {
                if let Ok(mut pressed) = PRESSED.lock() {
                    pressed.push(event.id.0.clone());
                }
                ctx.request_repaint();
            }
        }));

        Some(Self {
            menu,
            hwnd,
            file,
            recent_shown: recent.to_vec(),
            recent: recent_menu,
            entries,
            dark: None,
        })
    }

    /// List these maps under `Open Recent`, putting up a fresh submenu when
    /// they have changed: items cannot be taken out of a menu put up in place.
    pub fn sync_recent(&mut self, names: &[String]) {
        if self.recent_shown == names {
            return;
        }
        self.recent_shown = names.to_vec();

        let items: Vec<MenuItem> = names
            .iter()
            .enumerate()
            .map(|(index, name)| MenuItem::with_id(format!("recent:{index}"), name, true, None))
            .collect();
        let listed: Vec<&dyn IsMenuItem> =
            items.iter().map(|entry| entry as &dyn IsMenuItem).collect();
        // An empty list leaves the submenu greyed, the way Windows shows one.
        let Ok(recent) = Submenu::with_items("Open &Recent", !items.is_empty(), &listed) else {
            return;
        };

        // The old list goes, and the new one takes its place under `Open…`.
        let _ = self.file.remove(&self.recent);
        let _ = self.file.insert(&recent, 2);
        self.recent = recent;
    }

    /// Put each command's key beside it, as `Preferences` → `Keys` has it.
    pub fn sync_shortcuts(&mut self, keys: &[(Command, String)]) {
        for (command, shortcut) in keys {
            let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.command == *command)
            else {
                continue;
            };
            let text = if shortcut.is_empty() {
                entry.label.to_owned()
            } else {
                // A tab is how a Windows menu is told where the key goes.
                format!("{}\t{shortcut}", entry.label)
            };
            if entry.shown != text {
                entry.item.set_text(&text);
                entry.shown = text;
            }
        }
    }

    /// The items pressed since this was last asked.
    pub fn poll(&self) -> Vec<Command> {
        let Ok(mut pressed) = PRESSED.lock() else {
            return Vec::new();
        };
        let ids = std::mem::take(&mut *pressed);
        ids.iter().filter_map(|id| command(id)).collect()
    }

    /// Keep the bar in step with the app: what can be used, what is ticked, and
    /// which way the app is themed.
    pub fn sync(&mut self, editing: bool, waypoints: bool, stats: bool, dark: bool) {
        // The theme goes first: theming the bar hands it back as its items were
        // first made, so anything set before it would be undone.
        if self.dark != Some(dark) {
            self.dark = Some(dark);
            let theme = if dark {
                MenuTheme::Dark
            } else {
                MenuTheme::Light
            };
            // SAFETY: the handle is the window's own, and the menu is on it.
            if let Err(err) = unsafe { self.menu.set_theme_for_hwnd(self.hwnd, theme) } {
                log::warn!("could not theme the menu bar: {err}");
            }
        }

        // Said every frame, so that whatever rewrote the bar is answered.
        for entry in &self.entries {
            if needs_map(entry.command) {
                entry.item.set_enabled(editing);
            }
        }
        // Nothing to clear while nothing has been opened.
        for entry in &self.entries {
            if entry.command == Command::ClearRecent {
                entry.item.set_enabled(!self.recent_shown.is_empty());
            }
        }
        for entry in &self.entries {
            match entry.command {
                Command::Waypoints => entry.item.set_checked(waypoints),
                Command::Stats => entry.item.set_checked(stats),
                _ => {}
            }
        }
    }
}

fn command(id: &str) -> Option<Command> {
    if let Some((_, command, _)) = ITEMS.iter().find(|(item_id, _, _)| *item_id == id) {
        return Some(*command);
    }
    Some(Command::Recent(id.strip_prefix("recent:")?.parse().ok()?))
}
