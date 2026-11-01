//! Metrocraft — transit maps drawn on a real basemap.
//!
//! The map is the interface. Clicking bare ground places a node (a station, or
//! a waypoint once the card says so); the card that follows carries the mode
//! and the lines nearby, and the panel on the right names the node and holds
//! every line.

#[cfg(target_arch = "wasm32")]
use std::sync::mpsc::Receiver;

use eframe::egui::{
    Align, Align2, Button, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Id, Key,
    Layout, Margin, Modifiers, Order, Painter, Panel, Pos2, Rect, Response, RichText, ScrollArea,
    Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use serde::{Deserialize, Serialize};

use crate::bns;
use crate::keys::{self, Action, Keys};
use crate::metrodreamin;
use crate::model::{Elevation, GeoPos, Id as MapId, Line, Mode, Node, Pattern, Project};
use crate::platform;
use crate::theme;
use crate::tiles::{Basemap, TileKey, TileStore, Tone, PREVIEW_TILE};
use crate::view::MapView;
#[cfg(windows)]
use crate::window_menu;

const STORAGE_KEY: &str = "metrocraft-document-v1";
const PANEL_WIDTH: f32 = 248.0;
const LIST_HEIGHT: f32 = 264.0;
const EDGE: f32 = 18.0;
const STATION_HIT: f32 = 10.0;
const WAYPOINT_HIT: f32 = 9.5;
const SEGMENT_HIT: f32 = 6.0;
/// How many lines the node bar offers, nearest first.
const NEARBY_MAX: usize = 3;
/// The node bar: square buttons stacked beside the node.
const CARD_BUTTON: f32 = 22.0;
const CARD_GAP: f32 = 4.0;
const CARD_PAD: f32 = 5.0;
/// A basemap preview in Preferences: its tile, then its name under it.
const PREVIEW_SIDE: f32 = 84.0;
const PREVIEW_NAME: f32 = 15.0;
const PREVIEW_GAP: f32 = 8.0;
/// The settings categories Preferences offers, listed down its left side.
const PREFS_TABS: [&str; 3] = ["Basemap", "Theme", "Keys"];
/// Where `Theme` and `Keys` sit in [`PREFS_TABS`].
const THEME_TAB: usize = 1;
const KEYS_TAB: usize = 2;
/// A row in the Keys tab: one command and the key it answers to.
const KEY_ROW: f32 = 26.0;
/// A stop in the line panel: one row of the route down the panel.
const STATION_ROW: f32 = 24.0;
/// A walking interchange: the white band, with its outline either side.
const TRANSFER_WIDTH: f32 = 8.0;
/// How many nearby stations the panel offers to walk to at once.
const WALK_SUGGESTIONS: usize = 5;
/// How many lines a walk's row names before it runs out of room.
const WALK_CHIPS: usize = 4;
/// The closest two nodes sit on screen before their mark is left off: plain
/// stops go first, then bends, then names. Below that the marks would only pile
/// into a mush, and a map of tens of thousands of nodes would crawl. Sized so
/// the marks drawn stay around ten thousand at most, whatever the map.
const STOP_SPACING: f32 = 11.0;
const BEND_SPACING: f32 = 22.0;
const LABEL_SPACING: f32 = 40.0;
const PREFS_RAIL: f32 = 88.0;
/// Where the launcher keeps the maps it has opened, apart from the map in hand.
#[cfg(not(target_arch = "wasm32"))]
const PROJECTS_KEY: &str = "metrocraft-projects-v1";
#[cfg(not(target_arch = "wasm32"))]
const PROJECTS_MAX: usize = 100;
/// The launcher: a column of maps, each a row of name over the path it lives at.
const LAUNCH_WIDTH: f32 = 440.0;
const LAUNCH_ROW: f32 = 46.0;
/// How many maps `File` → `Open Recent` offers.
const RECENT_MAX: usize = 10;

// --------------------------------------------------------------------- state

/// What a click on bare ground creates.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Place {
    #[default]
    Station,
    Waypoint,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Selection {
    #[default]
    None,
    Station(MapId),
    Waypoint(MapId),
    Line(MapId),
    Transfer(MapId),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Station(MapId),
    Waypoint(MapId),
    /// A line and the index of the segment under the cursor.
    Line(MapId, usize),
    Transfer(MapId),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drag {
    Pan,
    Station(MapId),
    Waypoint(MapId),
}

/// The app opens on the project list, and leaves it for a map.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Projects,
    Editor,
}

#[derive(Clone, Copy, Debug)]
enum MenuAction {
    New,
    Open,
    Save,
    SaveAs,
    Fit,
    Undo,
    Redo,
    Preferences,
    /// Read a map published on MetroDreamin' into the editor.
    Import,
    /// Read a saved Brand New Subway game into the editor.
    ImportGame,
    /// Back to the list of maps.
    Projects,
    /// Give the map in hand another name.
    Rename,
    /// Open the map at this place in the recent list.
    Recent(usize),
    /// Empty the list of maps opened lately; the maps themselves stay put.
    #[cfg(not(target_arch = "wasm32"))]
    ClearRecent,
}

/// What a name being typed is for: a row of the project list, or the map in
/// hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RenameTarget {
    Row(usize),
    Document,
}

/// What the persisted document looks like between sessions.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Document {
    project: Project,
    view: MapView,
    basemap: Basemap,
    keys: Keys,
    theme: theme::Mode,
}

/// An account import: the scan for every map an account has, then one map at a
/// time into the project list — a folder on the desktop, the browser's own
/// store in the browser.
#[derive(Default)]
struct Bulk {
    /// The scan, until it answers with every map the account has.
    listing: Option<metrodreamin::Listing>,
    /// The folder the maps are written to, once it is picked.
    #[cfg(not(target_arch = "wasm32"))]
    folder: Option<std::path::PathBuf>,
    /// The maps still to read.
    queue: std::collections::VecDeque<metrodreamin::Listed>,
    total: usize,
    saved: usize,
    failed: usize,
    /// The map being written to the browser's store: the next one waits for it.
    #[cfg(target_arch = "wasm32")]
    writing: Option<Receiver<Result<platform::StoreReply, String>>>,
}

#[derive(Default)]
struct PanelAction {
    close: bool,
    delete: bool,
    toggle: Option<(MapId, Node)>,
    /// A walk reached from the list of lines, to be shown.
    walk: Option<MapId>,
}

// ----------------------------------------------------------------------- app

pub struct Metrocraft {
    project: Project,
    view: MapView,
    selection: Selection,
    place: Place,
    hover: Option<Hit>,
    hover_pos: Option<Pos2>,
    /// The line and node whose join is being previewed.
    preview: Option<(MapId, Node)>,
    /// The join already drawn, so the frame that needs to redraw it is asked for.
    preview_shown: Option<(MapId, Node)>,
    drag: Option<Drag>,
    drag_moved: bool,
    tiles: TileStore,
    undo: Vec<Project>,
    redo: Vec<Project>,
    error: Option<(String, f64)>,
    now: f64,
    canvas: Rect,
    pending_fit: bool,
    line_query: String,
    /// The line whose colour is being typed as hex, and the text being typed.
    hex_line: Option<MapId>,
    hex_text: String,
    show_waypoints: bool,
    /// `Tools` → `Network stats`: the figures for the network in hand.
    stats_open: bool,
    basemap: Basemap,
    mode: theme::Mode,
    keys: Keys,
    /// The command whose key is being changed, if the user is mid-change.
    listening: Option<Action>,
    preferences: bool,
    /// `File` → `Import from MetroDreamin'`: the link being pasted, and the
    /// read in flight.
    import_open: bool,
    import_url: String,
    import_rx: Option<std::sync::mpsc::Receiver<Result<Project, String>>>,
    /// `File` → `Import from MetroDreamin'` with a profile: every map the
    /// account has, saved into a folder, or into the browser's store.
    bulk: Option<Bulk>,
    prefs_tab: usize,
    lines_open: bool,
    lines_pick: Option<MapId>,
    /// The group whose name is being edited under the line list.
    group_pick: Option<MapId>,
    /// The node whose loop menu is open, where it opened, and on which frame.
    loop_menu: Option<(Node, Pos2, u64)>,
    focus_name: Option<MapId>,
    /// What `Rename…` is renaming, while its one field is open, and the name
    /// being typed in it.
    rename: Option<RenameTarget>,
    rename_text: String,
    /// Whether that field still has to take the keys.
    rename_focus: bool,
    #[cfg(not(target_arch = "wasm32"))]
    path: Option<std::path::PathBuf>,
    /// The maps the launcher offers, most recently opened first.
    #[cfg(not(target_arch = "wasm32"))]
    projects: Vec<std::path::PathBuf>,
    /// The window's own menu bar, once there is a window to put it on.
    #[cfg(windows)]
    menu: Option<window_menu::NativeMenu>,
    screen: Screen,
    #[cfg(target_arch = "wasm32")]
    load_rx: Option<Receiver<Result<String, String>>>,
    /// A Brand New Subway game the browser's file picker is fetching.
    #[cfg(target_arch = "wasm32")]
    game_rx: Option<Receiver<Result<String, String>>>,
    /// The maps the browser keeps, by name.
    #[cfg(target_arch = "wasm32")]
    web_projects: Vec<String>,
    /// A request to the browser's store, until it answers.
    #[cfg(target_arch = "wasm32")]
    store_rx: Option<Receiver<Result<platform::StoreReply, String>>>,
}

impl Metrocraft {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let document: Document = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, STORAGE_KEY))
            .unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        let projects = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, PROJECTS_KEY))
            .unwrap_or_default();
        theme::install(&cc.egui_ctx, document.theme);
        let mut project = document.project;
        project.sanitize();
        let app = Self {
            project,
            view: document.view,
            selection: Selection::None,
            place: Place::Station,
            hover: None,
            hover_pos: None,
            preview: None,
            preview_shown: None,
            drag: None,
            drag_moved: false,
            tiles: TileStore::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            error: None,
            now: 0.0,
            canvas: Rect::NOTHING,
            pending_fit: false,
            line_query: String::new(),
            hex_line: None,
            hex_text: String::new(),
            show_waypoints: true,
            stats_open: false,
            basemap: document.basemap,
            mode: document.theme,
            keys: document.keys,
            listening: None,
            preferences: false,
            import_open: false,
            import_url: String::new(),
            import_rx: None,
            bulk: None,
            prefs_tab: 0,
            lines_open: false,
            lines_pick: None,
            group_pick: None,
            loop_menu: None,
            focus_name: None,
            rename: None,
            rename_text: String::new(),
            rename_focus: false,
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
            #[cfg(not(target_arch = "wasm32"))]
            projects,
            #[cfg(windows)]
            menu: None,
            screen: Screen::Projects,
            #[cfg(target_arch = "wasm32")]
            load_rx: None,
            #[cfg(target_arch = "wasm32")]
            game_rx: None,
            // The browser's maps come back a moment later, once the store
            // answers.
            #[cfg(target_arch = "wasm32")]
            web_projects: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            store_rx: Some(platform::store_names(&cc.egui_ctx)),
        };
        platform::watch_swallowed_keys(&cc.egui_ctx);
        app
    }

    // ------------------------------------------------------------- documents

    fn snapshot(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            let current = std::mem::replace(&mut self.project, previous);
            self.redo.push(current);
            self.selection = Selection::None;
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let current = std::mem::replace(&mut self.project, next);
            self.undo.push(current);
            self.selection = Selection::None;
        }
    }

    fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&self.project)
            .map_err(|err| format!("Could not encode this map: {err}"))
    }

    fn file_stem(&self) -> String {
        file_stem(&self.project.name)
    }

    fn apply_json(&mut self, text: &str) -> Result<(), String> {
        let mut project: Project = serde_json::from_str(text)
            .map_err(|err| format!("That is not a Metrocraft map: {err}"))?;
        project.sanitize();
        self.snapshot();
        self.project = project;
        self.selection = Selection::None;
        self.lines_pick = None;
        self.group_pick = None;
        self.loop_menu = None;
        self.request_fit();
        Ok(())
    }

    /// What a fresh map is called. In the browser it must not land on a map
    /// the store already holds.
    #[cfg(not(target_arch = "wasm32"))]
    fn new_map_name(&self) -> String {
        "Untitled".to_owned()
    }

    #[cfg(target_arch = "wasm32")]
    fn new_map_name(&self) -> String {
        free_name("Untitled", &self.web_projects)
    }

    fn new_map(&mut self) {
        if self.project.stations.is_empty()
            && self.project.waypoints.is_empty()
            && self.project.lines.is_empty()
        {
            return;
        }
        self.snapshot();
        self.project = Project::new(self.new_map_name());
        self.selection = Selection::None;
        self.view = MapView::default();
        self.pending_fit = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.path = None;
        }
    }

    /// The maps `Open Recent` offers, most recent first.
    fn recent_names(&self) -> Vec<String> {
        #[cfg(not(target_arch = "wasm32"))]
        let names = self
            .projects
            .iter()
            .take(RECENT_MAX)
            .map(|path| map_name(path))
            .collect();
        #[cfg(target_arch = "wasm32")]
        let names = self.web_projects.iter().take(RECENT_MAX).cloned().collect();
        names
    }

    /// Open one of them, from the list that keeps it.
    fn open_recent(&mut self, index: usize, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = ctx;
            self.open_project(index);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(name) = self.web_projects.get(index).cloned() {
            self.store_rx = Some(platform::store_open(&name, ctx));
        }
    }

    fn request_fit(&mut self) {
        self.pending_fit = true;
    }

    fn apply_fit(&mut self, rect: Rect) {
        if rect.width() < 8.0 || rect.height() < 8.0 {
            return;
        }
        match self.project.bounds() {
            Some((min, max))
                if (max.lon - min.lon).abs() > 1.0e-5 || (max.lat - min.lat).abs() > 1.0e-5 =>
            {
                self.view.fit_bounds(min, max, rect, self.max_zoom());
            }
            Some((min, max)) => {
                let centre = GeoPos::new((min.lon + max.lon) * 0.5, (min.lat + max.lat) * 0.5);
                self.view.focus(centre, rect, 13.5, self.max_zoom());
            }
            None => {
                self.view = MapView::default();
                self.view.clamp_to(rect, self.max_zoom());
            }
        }
    }

    // ------------------------------------------------------------------ nodes

    fn selection_node(&self) -> Option<Node> {
        match self.selection {
            Selection::Station(id) => Some(Node::Station(id)),
            Selection::Waypoint(id) => Some(Node::Waypoint(id)),
            Selection::Line(_) | Selection::Transfer(_) | Selection::None => None,
        }
    }

    /// Switch what clicks create, converting the selected node along with it so
    /// the switch always describes the node it sits next to.
    fn set_place(&mut self, place: Place) {
        self.place = place;
        match (self.selection, place) {
            (Selection::Station(id), Place::Waypoint) => {
                self.snapshot();
                if let Some(waypoint) = self.project.station_to_waypoint(id) {
                    self.selection = Selection::Waypoint(waypoint);
                }
                self.focus_name = None;
            }
            (Selection::Waypoint(id), Place::Station) => {
                self.snapshot();
                if let Some(station) = self.project.waypoint_to_station(id) {
                    self.selection = Selection::Station(station);
                }
            }
            _ => {}
        }
    }

    fn place_node(&mut self, pos: GeoPos) {
        self.snapshot();
        match self.place {
            Place::Station => {
                let id = self.project.add_station(pos);
                self.selection = Selection::Station(id);
                self.focus_name = Some(id);
            }
            Place::Waypoint => {
                let id = self.project.add_waypoint(pos);
                self.selection = Selection::Waypoint(id);
                self.focus_name = None;
            }
        }
    }

    fn delete_selection(&mut self) {
        match self.selection {
            Selection::Station(id) => {
                if self.project.stations.contains_key(&id) {
                    self.snapshot();
                    self.project.remove_station(id);
                }
            }
            Selection::Waypoint(id) => {
                if self.project.waypoints.contains_key(&id) {
                    self.snapshot();
                    self.project.remove_waypoint(id);
                }
            }
            Selection::Line(id) => {
                if self.project.lines.contains_key(&id) {
                    self.snapshot();
                    self.project.remove_line(id);
                }
            }
            Selection::Transfer(id) => {
                if self.project.transfers.contains_key(&id) {
                    self.snapshot();
                    self.project.remove_transfer(id);
                }
            }
            Selection::None => {}
        }
        self.selection = Selection::None;
    }

    fn next_line_name(&self) -> String {
        let mut index = self.project.lines.len() + 1;
        loop {
            let name = format!("Line {index}");
            if !self.project.lines.values().any(|line| line.name == name) {
                return name;
            }
            index += 1;
        }
    }

    fn next_group_name(&self) -> String {
        let mut index = self.project.groups.len() + 1;
        loop {
            let name = format!("Group {index}");
            if !self.project.groups.values().any(|group| group.name == name) {
                return name;
            }
            index += 1;
        }
    }

    fn next_line_color(&self) -> [u8; 3] {
        let palette = theme::PALETTE;
        palette[self.project.lines.len() % palette.len()]
    }

    /// A new line, carrying the selected node when there is one.
    fn create_line(&mut self) {
        self.snapshot();
        let name = self.next_line_name();
        let color = self.next_line_color();
        let id = self.project.add_line(name, color);
        if let Some(node) = self.selection_node() {
            self.project.attach(id, node);
        }
        self.lines_pick = Some(id);
        self.lines_open = true;
    }

    fn toggle_attach(&mut self, line_id: MapId, node: Node) {
        if !self.project.lines.contains_key(&line_id) {
            return;
        }
        self.snapshot();
        self.project.toggle_node(line_id, node);
    }

    // ----------------------------------------------------------------- input

    fn on_click(&mut self, screen: Pos2, rect: Rect) {
        if let Some(hit) = hit_test(&self.project, &self.view, rect, screen) {
            self.selection = match hit {
                Hit::Station(id) => Selection::Station(id),
                Hit::Waypoint(id) => Selection::Waypoint(id),
                Hit::Line(id, _) => {
                    self.lines_pick = Some(id);
                    self.group_pick = None;
                    self.lines_open = true;
                    Selection::Line(id)
                }
                Hit::Transfer(id) => Selection::Transfer(id),
            };
        } else {
            let pos = self.view.screen_to_world(screen, rect).sanitized();
            self.place_node(pos);
        }
        // Keep "what a click creates" in step with the node in hand.
        match self.selection {
            Selection::Station(_) => self.place = Place::Station,
            Selection::Waypoint(_) => self.place = Place::Waypoint,
            Selection::Line(_) | Selection::Transfer(_) | Selection::None => {}
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // A key being changed takes the next press, whatever it is; Escape alone
        // backs out and leaves the binding as it was.
        if let Some(action) = self.listening {
            if let Some(shortcut) = ctx.input(keys::captured) {
                self.listening = None;
                if shortcut.logical_key != Key::Escape || shortcut.modifiers != Modifiers::NONE {
                    self.keys.rebind(action, shortcut);
                }
            }
            return;
        }
        // A field has the keys while a node is being named, and only a command
        // on a modifier is still a command there: the bare keys, and the undo a
        // field does for itself, stay with the field.
        let typing = ctx.egui_wants_keyboard_input();

        let fired: Vec<Action> = ctx.input(|input| {
            keys::ACTIONS
                .into_iter()
                .filter(|action| {
                    !typing
                        || (self.keys.modified(*action)
                            && !matches!(action, Action::Undo | Action::Redo))
                })
                .filter(|action| self.keys.pressed(*action, input))
                .collect()
        });
        for action in fired {
            self.run(action, ctx);
        }

        // The browser keeps the keys its own field holds, so the commands among
        // them arrive here instead of through egui. They are taken every frame,
        // whatever the field is doing, so none of them fires late.
        #[cfg(target_arch = "wasm32")]
        for pressed in platform::swallowed_keys() {
            if !typing {
                continue;
            }
            match self.command_for(pressed) {
                // Undo is the field's while it holds the keys.
                Some(Action::Undo | Action::Redo) | None => {}
                Some(action) => self.run(action, ctx),
            }
        }

        if typing {
            return;
        }
        let (escape, delete) = ctx.input(|input| {
            (
                input.key_pressed(Key::Escape),
                input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace),
            )
        });
        // Escape is the rename field's while it is open.
        if escape && self.rename.is_none() {
            self.selection = Selection::None;
            self.preferences = false;
            self.loop_menu = None;
        }
        if delete {
            self.delete_selection();
        }
    }

    /// The command a key pressed while the browser's field had the keys
    /// answers to.
    #[cfg(target_arch = "wasm32")]
    fn command_for(&self, pressed: platform::SwallowedKey) -> Option<Action> {
        keys::ACTIONS.into_iter().find(|action| {
            self.keys.bind(*action).is_some_and(|shortcut| {
                shortcut.logical_key == pressed.key
                    && shortcut.modifiers.command == pressed.command
                    && shortcut.modifiers.shift == pressed.shift
                    && shortcut.modifiers.alt == pressed.alt
            })
        })
    }

    fn run(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Save => self.save_document(ctx, false),
            Action::SaveAs => self.save_document(ctx, true),
            Action::Open => self.open_document(),
            Action::New => self.new_map(),
            Action::Fit => self.request_fit(),
            Action::Preferences => self.preferences = true,
        }
    }

    fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some((message.into(), self.now + 4.0));
    }

    // --------------------------------------------------------------- basemap

    /// The deepest the camera may go on the basemap in use: the last level it
    /// really serves, plus two levels of magnification.
    fn max_zoom(&self) -> f64 {
        (self.basemap.max_zoom() as f64 + 2.0).min(crate::view::MAX_ZOOM)
    }

    /// Ink and halo, the right way round for the map under them.
    fn map_ink(&self) -> (Color32, Color32) {
        match self.basemap.tone() {
            Tone::Light => (theme::MAP_INK, theme::MAP_HALO),
            Tone::Dark => (theme::MAP_INK_ON_DARK, theme::MAP_HALO_ON_DARK),
        }
    }

    /// What shows through before a basemap's first tiles arrive.
    fn backdrop(&self) -> Color32 {
        match self.basemap.tone() {
            Tone::Light => theme::MAP_BACKDROP,
            Tone::Dark => theme::canvas(),
        }
    }

    fn set_basemap(&mut self, basemap: Basemap) {
        self.basemap = basemap;
    }

    // ---------------------------------------------------------------- canvas

    fn canvas(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        self.canvas = rect;
        if self.pending_fit {
            self.pending_fit = false;
            if !self.view.zoom.is_finite()
                || !self.view.lon.is_finite()
                || !self.view.lat.is_finite()
            {
                self.view = MapView::default();
            }
            self.apply_fit(rect);
        }
        self.view.clamp_to(rect, self.max_zoom());

        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        self.zoom_input(ui, &response, rect);

        let painter = ui.painter().with_clip_rect(rect);
        let pixels_per_point = ui.ctx().pixels_per_point();
        self.paint_tiles(&painter, rect, pixels_per_point);

        self.hover = response
            .hover_pos()
            .filter(|pos| rect.contains(*pos))
            .and_then(|pos| hit_test(&self.project, &self.view, rect, pos));

        let spacing = self.node_spacing(rect);
        self.paint_lines(&painter, rect);
        self.paint_transfers(&painter, rect);
        self.paint_labels(&painter, rect, spacing);
        self.paint_nodes(&painter, rect, spacing);

        // A click while a key is being changed is only there to cancel it.
        if self.listening.is_none() {
            self.handle_pointer(ui, &response, rect);
        }

        if response.hovered() {
            let icon = match self.hover {
                Some(Hit::Station(_) | Hit::Waypoint(_)) if self.drag_moved => CursorIcon::Grabbing,
                Some(Hit::Station(_) | Hit::Waypoint(_)) => CursorIcon::Grab,
                Some(Hit::Line(_, _) | Hit::Transfer(_)) => CursorIcon::PointingHand,
                None => CursorIcon::Crosshair,
            };
            ui.ctx().set_cursor_icon(icon);
        }
    }

    fn zoom_input(&mut self, ui: &Ui, response: &Response, rect: Rect) {
        if !response.hovered() {
            return;
        }
        let (scroll, pinch) = ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
        let Some(cursor) = ui.input(|input| input.pointer.hover_pos()) else {
            return;
        };
        let max_zoom = self.max_zoom();
        if (pinch - 1.0).abs() > 0.001 {
            self.view.zoom_at(pinch as f64, cursor, rect, max_zoom);
        } else if let Some(levels) = zoom_levels(scroll) {
            self.view
                .zoom_at(2.0f64.powf(levels), cursor, rect, max_zoom);
        }
    }

    fn handle_pointer(&mut self, ui: &Ui, response: &Response, rect: Rect) {
        let pointer = ui.input(|input| input.pointer.latest_pos());
        self.hover_pos = pointer;

        if response.drag_started() {
            // Pick the target from where the press began: by the time egui calls
            // it a drag the pointer has already travelled past the threshold.
            let origin = ui.input(|input| input.pointer.press_origin()).or(pointer);
            if let Some(pos) = origin {
                self.drag = Some(match hit_test(&self.project, &self.view, rect, pos) {
                    Some(Hit::Station(id)) => {
                        self.selection = Selection::Station(id);
                        Drag::Station(id)
                    }
                    Some(Hit::Waypoint(id)) => {
                        self.selection = Selection::Waypoint(id);
                        Drag::Waypoint(id)
                    }
                    _ => Drag::Pan,
                });
                self.drag_moved = false;
            }
        }

        if response.dragged() {
            if response.drag_delta() != Vec2::ZERO && !self.drag_moved {
                if matches!(self.drag, Some(Drag::Station(_) | Drag::Waypoint(_))) {
                    self.snapshot();
                }
                self.drag_moved = true;
            }
            match self.drag {
                Some(Drag::Pan) => {
                    let delta = response.drag_delta();
                    if delta != Vec2::ZERO {
                        self.view.pan_pixels(delta, rect);
                    }
                }
                Some(Drag::Station(id)) => {
                    if let Some(pos) = pointer {
                        let pos = self.view.screen_to_world(pos, rect).sanitized();
                        if let Some(station) = self.project.stations.get_mut(&id) {
                            station.pos = pos;
                        }
                    }
                }
                Some(Drag::Waypoint(id)) => {
                    if let Some(pos) = pointer {
                        let pos = self.view.screen_to_world(pos, rect).sanitized();
                        if let Some(waypoint) = self.project.waypoints.get_mut(&id) {
                            waypoint.pos = pos;
                        }
                    }
                }
                None => {}
            }
        }

        if response.drag_stopped() {
            self.drag = None;
            self.drag_moved = false;
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.on_click(pos, rect);
            }
        }
    }

    // --------------------------------------------------------------- painting

    fn paint_tiles(&mut self, painter: &Painter, rect: Rect, pixels_per_point: f32) {
        painter.rect_filled(rect, 0.0, self.backdrop());
        let basemap = self.basemap;
        let zoom = self.view.tile_zoom(pixels_per_point, basemap.max_zoom());
        let count = 1i64 << zoom;
        let (x0, x1, y0, y1) = self.view.visible_tile_range(rect, zoom);
        let centre = rect.center();
        let mut pending: Vec<(f32, TileKey)> = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                // Beyond the antimeridian the basemap ends rather than repeating.
                if x < 0 || x >= count {
                    continue;
                }
                let key = TileKey::new(zoom, x, y);
                let dest = self.view.tile_screen_rect(zoom, x, y, rect);
                let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                if let Some(texture) = self.tiles.ready((basemap, key)) {
                    painter.image(texture.id(), dest, whole, Color32::WHITE);
                } else {
                    if let Some((parent, texture)) = self.tiles.fallback((basemap, key)) {
                        let uv = parent_uv(key, parent);
                        painter.image(texture.id(), dest, uv, Color32::WHITE);
                    }
                    pending.push((dest.center().distance_sq(centre), key));
                }
            }
        }
        // The queue is drained from its end, so the tiles farthest from the
        // middle go in first: the map fills in where the eye already is.
        pending.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, key) in pending {
            self.tiles.ensure((basemap, key));
        }
        // A hair of contrast so lines read cleanly over a lit map.
        if self.basemap.tone() == Tone::Light {
            painter.rect_filled(rect, 0.0, Color32::from_black_alpha(28));
        }
    }

    fn paint_lines(&self, painter: &Painter, rect: Rect) {
        let (ink, _) = self.map_ink();
        // The casing keeps the line apart from whatever the basemap puts under it.
        let casing = match self.basemap.tone() {
            Tone::Light => theme::station_edge(),
            Tone::Dark => theme::MAP_INK_ON_DARK,
        };
        for (id, line) in &self.project.lines {
            if self.project.line_hidden(*id) {
                continue;
            }
            // A line being previewed is drawn where the node would join it,
            // not where it runs now.
            let ghost = self.preview_ghost(*id);
            let drawn = ghost.as_ref().unwrap_or(line);
            let Some(points) = self.line_points(drawn, rect) else {
                continue;
            };
            if points.len() < 2 {
                continue;
            }
            let width = line.width.clamp(2.0, 12.0);
            let color = theme::rgb(line.color);
            // Bends too slight to show inside a stroke as wide as this one are
            // dropped, so a route of thousands of gentle turns is stroked as
            // the handful of stretches it actually reads as.
            let points = thin_points(&points, width * 0.4);
            // The stretch of route under the selection or the pointer, drawn as
            // a wider wash behind the line itself.
            let emphasis = if self.selection == Selection::Line(*id) {
                Some((12.0, 55.0))
            } else if matches!(self.hover, Some(Hit::Line(hovered, _)) if hovered == *id) {
                Some((9.0, 32.0))
            } else {
                None
            };

            // Only the stretches that cross the viewport are stroked: a route
            // of a thousand bends costs what the screen shows, not the route.
            for run in visible_runs(&points, rect) {
                let stretch = &points[run];
                if let Some((pad, alpha)) = emphasis {
                    painter.add(Shape::line(
                        stretch.to_vec(),
                        Stroke::new(width + pad, theme::alpha(ink, alpha as u8)),
                    ));
                }
                paint_run(
                    painter,
                    stretch,
                    Stroke::new(width + 3.0, theme::alpha(casing, 190)),
                    drawn.mode,
                    drawn.pattern,
                    width,
                );
                if ghost.is_some() {
                    painter.add(Shape::line(
                        stretch.to_vec(),
                        Stroke::new(width, theme::alpha(color, 215)),
                    ));
                    // A hairline down the middle, so the route reads as a suggestion.
                    painter.add(Shape::line(
                        stretch.to_vec(),
                        Stroke::new(1.4, theme::alpha(theme::ink(), 200)),
                    ));
                } else {
                    paint_run(
                        painter,
                        stretch,
                        Stroke::new(width, color),
                        drawn.mode,
                        drawn.pattern,
                        width,
                    );
                }
            }
        }
    }

    /// The route a line would take if the hovered node joined it.
    fn preview_ghost(&self, id: MapId) -> Option<Line> {
        let (preview_id, node) = self.preview_shown?;
        if preview_id != id {
            return None;
        }
        let line = self.project.lines.get(&id)?;
        if line.nodes.contains(&node) {
            return None;
        }
        let mut ghost = line.clone();
        let index = self.project.best_index(id, node).min(ghost.nodes.len());
        ghost.nodes.insert(index, node);
        Some(ghost)
    }

    fn line_points(&self, line: &Line, rect: Rect) -> Option<Vec<Pos2>> {
        let mut points = node_screen_points(&self.project, &self.view, line, rect)?;
        if line.closed && points.len() > 2 {
            if line.turnback {
                // A turnback doubles back: the stroke leaves the last node
                // beside the route and runs back along it before joining the
                // start, so the loop reads as a hairpin rather than a crossing.
                let last = points[points.len() - 1];
                let previous = points[points.len() - 2];
                let away = last - previous;
                if away.length() > 1.0 {
                    let side = Vec2::new(-away.y, away.x) / away.length()
                        * (line.width.clamp(2.0, 12.0) * 1.7);
                    points.push(last + side);
                    points.push(previous + side);
                }
            }
            points.push(points[0]);
        }
        Some(points)
    }

    /// The way a walking interchange runs, its shaping waypoints in order.
    fn transfer_points(&self, id: MapId, rect: Rect) -> Option<Vec<Pos2>> {
        let nodes = self.project.transfer_nodes(id)?;
        let points: Vec<Pos2> = nodes
            .iter()
            .filter_map(|node| self.project.node_pos(*node))
            .map(|pos| self.view.world_to_screen(pos, rect))
            .collect();
        (points.len() >= 2).then_some(points)
    }
    /// Walking interchanges, drawn the way MetroDreamin' does: a white band
    /// with a thin dark outline around it, under the stations but over the
    /// lines it crosses.
    fn paint_transfers(&self, painter: &Painter, rect: Rect) {
        for id in self.project.transfers.keys() {
            let Some(points) = self.transfer_points(*id, rect) else {
                continue;
            };
            if self.selection == Selection::Transfer(*id) || self.hover == Some(Hit::Transfer(*id))
            {
                painter.add(Shape::line(
                    points.clone(),
                    Stroke::new(TRANSFER_WIDTH + 10.0, theme::alpha(theme::ink(), 40)),
                ));
            }
            painter.add(Shape::line(
                points.clone(),
                Stroke::new(TRANSFER_WIDTH + 4.0, theme::station_edge()),
            ));
            painter.add(Shape::line(
                points,
                Stroke::new(TRANSFER_WIDTH, Color32::WHITE),
            ));
        }
    }

    fn node_spacing(&self, rect: Rect) -> f32 {
        let Some((min, max)) = self.project.bounds() else {
            return f32::INFINITY;
        };
        let count = self.project.stations.len() + self.project.waypoints.len();
        if count == 0 {
            return f32::INFINITY;
        }
        let span =
            (self.view.world_to_screen(max, rect) - self.view.world_to_screen(min, rect)).abs();
        let span = span.x.max(span.y);
        // A map that covers no ground at all has all the room it needs.
        if span < 1.0 {
            return f32::INFINITY;
        }
        span / (count as f32).sqrt()
    }

    fn paint_labels(&self, painter: &Painter, rect: Rect, spacing: f32) {
        if self.view.zoom < 6.0 || spacing < LABEL_SPACING {
            return;
        }
        let (ink, halo) = self.map_ink();
        let font = medium(11.5);
        let mut placed: Vec<Rect> = Vec::new();
        for station in self.project.stations.values() {
            let anchor = self.view.world_to_screen(station.pos, rect);
            if !rect.expand(40.0).contains(anchor) {
                continue;
            }
            let text_rect = Align2::LEFT_CENTER.anchor_size(
                anchor + Vec2::new(9.0, 0.0),
                painter
                    .layout_no_wrap(station.name.clone(), font.clone(), ink)
                    .size(),
            );
            if placed
                .iter()
                .any(|other| other.intersects(text_rect.expand(3.0)))
            {
                continue;
            }
            placed.push(text_rect.expand(3.0));
            let galley = painter.layout_no_wrap(station.name.clone(), font.clone(), ink);
            painter.galley(
                text_rect.min + Vec2::new(0.0, 1.0),
                galley.clone(),
                theme::alpha(halo, 200),
            );
            painter.galley(text_rect.min, galley, ink);
        }
    }

    fn paint_nodes(&self, painter: &Painter, rect: Rect, spacing: f32) {
        let roles = self.project.node_roles();
        let (ink, halo) = self.map_ink();
        // Only the marks that still read at this zoom: names first, then bends,
        // then plain stops. A station where lines meet or that ends one always
        // stays, so the shape of the network survives being zoomed out.
        let stops = spacing >= STOP_SPACING;
        let bends = spacing >= BEND_SPACING;

        if self.show_waypoints && bends {
            for (id, waypoint) in &self.project.waypoints {
                let pos = self.view.world_to_screen(waypoint.pos, rect);
                if !rect.expand(30.0).contains(pos) {
                    continue;
                }
                let attached = roles.in_use(*id);
                let (half, strength) = if self.selection == Selection::Waypoint(*id) {
                    (4.8, 255)
                } else if self.hover == Some(Hit::Waypoint(*id)) {
                    (4.3, 200)
                } else if attached {
                    (3.9, 235)
                } else {
                    (3.5, 160)
                };
                if self.selection == Selection::Waypoint(*id) {
                    painter.circle_stroke(pos, 7.5, Stroke::new(1.4, theme::accent()));
                }
                // An x, so a bend never reads as a stop, over a halo so it stays
                // legible on top of the line it shapes.
                let back = Stroke::new(3.8, theme::alpha(halo, 215));
                let front = Stroke::new(1.6, theme::alpha(ink, strength));
                for stroke in [back, front] {
                    painter.line_segment(
                        [pos + Vec2::new(-half, -half), pos + Vec2::new(half, half)],
                        stroke,
                    );
                    painter.line_segment(
                        [pos + Vec2::new(half, -half), pos + Vec2::new(-half, half)],
                        stroke,
                    );
                }
            }
        }

        for (id, station) in &self.project.stations {
            let pos = self.view.world_to_screen(station.pos, rect);
            if !rect.expand(30.0).contains(pos) {
                continue;
            }
            let interchange = roles.degree(*id) >= 2;
            let mark = station_mark(interchange, roles.is_terminal(*id));
            let marked =
                self.selection == Selection::Station(*id) || self.hover == Some(Hit::Station(*id));
            if !stops && !marked && mark == StationMark::Stop {
                continue;
            }
            // A buried station is solid, a raised one carries a heavier rim.
            let ink = station_ink(station.elevation);
            let radius = if mark == StationMark::Interchange {
                5.0
            } else {
                3.6
            } * ink.scale;
            // A station where lines meet is a rhombus, so a crossroads never
            // reads as a bigger dot; its corners point up, down and sideways.
            let corner = radius * 1.34;
            if self.selection == Selection::Station(*id) {
                let stroke = Stroke::new(1.3, theme::accent());
                if mark == StationMark::Interchange {
                    painter.add(Shape::closed_line(
                        diamond_points(pos, corner + 4.5),
                        stroke,
                    ));
                } else {
                    painter.circle_stroke(pos, radius + 4.5, stroke);
                }
            } else if self.hover == Some(Hit::Station(*id)) {
                let stroke = Stroke::new(1.0, theme::alpha(theme::ink(), 90));
                if mark == StationMark::Interchange {
                    painter.add(Shape::closed_line(
                        diamond_points(pos, corner + 3.5),
                        stroke,
                    ));
                } else {
                    painter.circle_stroke(pos, radius + 3.5, stroke);
                }
            }
            paint_station_mark(painter, pos, radius, mark, ink);
        }
    }

    // -------------------------------------------------------------- overlays

    fn menu_bar(&mut self, ui: &mut Ui) {
        let mut action: Option<MenuAction> = None;
        // The maps opened lately, most recent first.
        let recent = self.recent_names();
        Panel::top("menu-bar")
            .frame(
                Frame::new()
                    .fill(theme::panel())
                    .inner_margin(Margin::symmetric(6, 0)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().button_padding = Vec2::new(9.0, 4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    ui.menu_button(menu_title("File"), |ui| {
                        ui.set_min_width(190.0);
                        if menu_item(ui, &self.keys, "New", Action::New) {
                            action = Some(MenuAction::New);
                        }
                        if menu_item(ui, &self.keys, "Open…", Action::Open) {
                            action = Some(MenuAction::Open);
                        }
                        // Greyed while no map has been opened yet.
                        ui.add_enabled_ui(!recent.is_empty(), |ui| {
                            ui.menu_button(menu_title("Open Recent"), |ui| {
                                ui.set_min_width(190.0);
                                for (index, name) in recent.iter().enumerate() {
                                    if plain_menu_item(ui, name) {
                                        action = Some(MenuAction::Recent(index));
                                    }
                                }
                            });
                        });
                        if plain_menu_item(ui, "Import from MetroDreamin'…") {
                            action = Some(MenuAction::Import);
                        }
                        if plain_menu_item(ui, "Import from Brand New Subway…") {
                            action = Some(MenuAction::ImportGame);
                        }
                        ui.separator();
                        if menu_item(ui, &self.keys, "Save", Action::Save) {
                            action = Some(MenuAction::Save);
                        }
                        if menu_item(ui, &self.keys, "Save as…", Action::SaveAs) {
                            action = Some(MenuAction::SaveAs);
                        }
                        if plain_menu_item(ui, "Rename…") {
                            action = Some(MenuAction::Rename);
                        }
                        ui.separator();
                        if plain_menu_item(ui, "Projects") {
                            action = Some(MenuAction::Projects);
                        }
                        // The browser's list is the maps themselves, so it has
                        // nothing to forget that is not a map.
                        #[cfg(not(target_arch = "wasm32"))]
                        ui.add_enabled_ui(!recent.is_empty(), |ui| {
                            if plain_menu_item(ui, "Clear Recent") {
                                action = Some(MenuAction::ClearRecent);
                            }
                        });
                    });
                    ui.menu_button(menu_title("Edit"), |ui| {
                        ui.set_min_width(190.0);
                        if menu_item(ui, &self.keys, "Undo", Action::Undo) {
                            action = Some(MenuAction::Undo);
                        }
                        if menu_item(ui, &self.keys, "Redo", Action::Redo) {
                            action = Some(MenuAction::Redo);
                        }
                        ui.separator();
                        if menu_item(ui, &self.keys, "Preferences…", Action::Preferences) {
                            action = Some(MenuAction::Preferences);
                        }
                    });
                    ui.menu_button(menu_title("View"), |ui| {
                        ui.set_min_width(190.0);
                        if menu_item(ui, &self.keys, "Fit to map", Action::Fit) {
                            action = Some(MenuAction::Fit);
                        }
                        ui.checkbox(
                            &mut self.show_waypoints,
                            RichText::new("Waypoints").font(medium(12.5)),
                        );
                    });
                    ui.menu_button(menu_title("Tools"), |ui| {
                        ui.set_min_width(190.0);
                        ui.checkbox(
                            &mut self.stats_open,
                            RichText::new("Network stats").font(medium(12.5)),
                        );
                    });
                });
            });

        self.run_menu(action, ui.ctx());
    }

    /// Do what a menu item asks, whichever bar it came from.
    fn run_menu(&mut self, action: Option<MenuAction>, ctx: &egui::Context) {
        match action {
            Some(MenuAction::New) if self.screen == Screen::Projects => self.new_listed_map(),
            Some(MenuAction::New) => self.new_map(),
            Some(MenuAction::Open) => self.open_document(),
            Some(MenuAction::Save) => self.save_document(ctx, false),
            Some(MenuAction::SaveAs) => self.save_document(ctx, true),
            Some(MenuAction::Fit) => self.request_fit(),
            Some(MenuAction::Undo) => self.undo(),
            Some(MenuAction::Redo) => self.redo(),
            Some(MenuAction::Preferences) => self.preferences = true,
            Some(MenuAction::Import) => self.import_open = true,
            Some(MenuAction::ImportGame) => self.import_game(),
            Some(MenuAction::Rename) => {
                let name = self.project.name.clone();
                self.start_rename(RenameTarget::Document, name);
            }
            Some(MenuAction::Recent(index)) => self.open_recent(index, ctx),
            #[cfg(not(target_arch = "wasm32"))]
            Some(MenuAction::ClearRecent) => self.projects.clear(),
            Some(MenuAction::Projects) => {
                // Leave nothing from the editor open behind the list.
                self.preferences = false;
                self.listening = None;
                self.rename = None;
                self.screen = Screen::Projects;
            }
            None => {}
        }
    }

    /// The window's own menu bar: put up once, then kept in step, its presses
    /// run as the commands the app's own bar runs.
    #[cfg(windows)]
    fn window_menu(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) -> bool {
        use window_menu::Command as Native;

        // The maps opened lately, most recent first, as `Open Recent` lists them.
        let recent = self.recent_names();
        // The keys as `Preferences` → `Keys` has them, so a change shows here.
        let shortcuts: Vec<(Native, String)> = [
            (Native::New, Action::New),
            (Native::Open, Action::Open),
            (Native::Save, Action::Save),
            (Native::SaveAs, Action::SaveAs),
            (Native::Undo, Action::Undo),
            (Native::Redo, Action::Redo),
            (Native::Preferences, Action::Preferences),
            (Native::Fit, Action::Fit),
        ]
        .iter()
        .map(|(command, action)| (*command, self.keys.label(ctx, *action)))
        .collect();

        if self.menu.is_none() {
            self.menu = window_menu::NativeMenu::install(frame, ctx, &recent);
        }

        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        menu.sync_recent(&recent);
        menu.sync_shortcuts(&shortcuts);
        menu.sync(
            self.screen == Screen::Editor,
            self.show_waypoints,
            self.stats_open,
            self.mode == theme::Mode::Dark,
        );
        for command in menu.poll() {
            let action = match command {
                Native::Waypoints => {
                    self.show_waypoints = !self.show_waypoints;
                    continue;
                }
                Native::Stats => {
                    self.stats_open = !self.stats_open;
                    continue;
                }
                Native::New => MenuAction::New,
                Native::Open => MenuAction::Open,
                Native::Import => MenuAction::Import,
                Native::ImportGame => MenuAction::ImportGame,
                Native::Save => MenuAction::Save,
                Native::SaveAs => MenuAction::SaveAs,
                Native::Rename => MenuAction::Rename,
                Native::Projects => MenuAction::Projects,
                Native::Undo => MenuAction::Undo,
                Native::Redo => MenuAction::Redo,
                Native::Preferences => MenuAction::Preferences,
                Native::Fit => MenuAction::Fit,
                Native::Recent(index) => MenuAction::Recent(index),
                Native::ClearRecent => MenuAction::ClearRecent,
            };
            self.run_menu(Some(action), ctx);
        }
        true
    }

    /// Everywhere else the app draws its own menu bar.
    #[cfg(not(windows))]
    fn window_menu(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) -> bool {
        false
    }

    /// The lines nearest the node, as colour swatches.
    fn nearby_lines(&self, node: Node, pos: GeoPos) -> Vec<(MapId, [u8; 3], bool)> {
        let anchor = self.view.world_to_screen(pos, self.canvas);
        let mut found: Vec<(f32, MapId, [u8; 3], bool)> = Vec::new();
        for (id, line) in &self.project.lines {
            if self.project.line_hidden(*id) {
                continue;
            }
            let Some(points) = self.line_points(line, self.canvas) else {
                continue;
            };
            if points.len() < 2 {
                continue;
            }
            let mut closest = f32::MAX;
            for window in points.windows(2) {
                closest = closest.min(point_to_segment(anchor, window[0], window[1]));
            }
            found.push((closest, *id, line.color, line.nodes.contains(&node)));
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found
            .into_iter()
            .take(NEARBY_MAX)
            .map(|(_, id, color, attached)| (id, color, attached))
            .collect()
    }

    fn node_card(&mut self, ctx: &egui::Context) {
        let (node, pos) = match self.selection {
            Selection::Station(id) => match self.project.stations.get(&id) {
                Some(station) => (Node::Station(id), station.pos),
                None => return,
            },
            Selection::Waypoint(id) => match self.project.waypoints.get(&id) {
                Some(waypoint) => (Node::Waypoint(id), waypoint.pos),
                None => return,
            },
            Selection::Line(_) | Selection::Transfer(_) | Selection::None => return,
        };
        if self.canvas.width() < 40.0 {
            return;
        }

        let anchor = self.view.world_to_screen(pos, self.canvas);
        let nearby = self.nearby_lines(node, pos);
        let transfer = match node {
            Node::Waypoint(id) => self.nearby_transfer(id, pos),
            Node::Station(_) => None,
        };
        let loopable = !self.loopable_lines(node).is_empty();
        let items = nearby.len() + usize::from(transfer.is_some()) + usize::from(loopable) + 2;
        let size = Vec2::new(
            CARD_BUTTON + CARD_PAD * 2.0,
            items as f32 * CARD_BUTTON + (items as f32 - 1.0) * CARD_GAP + CARD_PAD * 2.0,
        );
        let mut top_left = anchor + Vec2::new(15.0, -size.y * 0.5);
        if top_left.x + size.x > self.canvas.right() - 8.0 {
            top_left.x = anchor.x - size.x - 15.0;
        }
        let lowest = (self.canvas.bottom() - size.y - 8.0).max(self.canvas.top() + 8.0);
        top_left.y = top_left.y.clamp(self.canvas.top() + 8.0, lowest);

        let mut toggled: Option<MapId> = None;
        let mut walked: Option<MapId> = None;
        let mut convert = false;
        let mut delete = false;
        let mut open_loop = false;

        egui::Area::new(Id::unique("node-card"))
            .order(Order::Foreground)
            .fixed_pos(top_left)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(CARD_PAD as i8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(0.0, CARD_GAP);
                        ui.vertical(|ui| {
                            for (id, color, attached) in &nearby {
                                let response = swatch(ui, *color, *attached);
                                if !*attached && response.hovered() {
                                    self.preview = Some((*id, node));
                                }
                                if response.clicked() {
                                    toggled = Some(*id);
                                }
                            }
                            if let Some((id, attached)) = transfer {
                                if transfer_swatch(ui, attached).clicked() {
                                    walked = Some(id);
                                }
                            }
                            if loopable && loop_button(ui).clicked() {
                                open_loop = true;
                            }
                            let to_waypoint = matches!(node, Node::Station(_));
                            if arrow_button(ui, to_waypoint).clicked() {
                                convert = true;
                            }
                            if trash_button(ui).clicked() {
                                delete = true;
                            }
                        });
                    });
            });

        if let Some(line_id) = toggled {
            self.toggle_attach(line_id, node);
        }
        if let (Some(transfer_id), Node::Waypoint(waypoint)) = (walked, node) {
            let attached = self
                .project
                .transfers
                .get(&transfer_id)
                .is_some_and(|transfer| transfer.via.contains(&waypoint));
            if self.project.transfers.contains_key(&transfer_id) {
                self.snapshot();
                if attached {
                    self.project.detach_from_transfer(transfer_id, waypoint);
                } else {
                    self.project.attach_to_transfer(transfer_id, waypoint);
                }
            }
        }
        if open_loop {
            self.loop_menu = Some((
                node,
                Pos2::new(top_left.x + size.x + 4.0, top_left.y),
                ctx.cumulative_pass_nr(),
            ));
        }
        if convert {
            let to = if matches!(node, Node::Station(_)) {
                Place::Waypoint
            } else {
                Place::Station
            };
            self.set_place(to);
        }
        if delete {
            self.delete_selection();
        }
    }

    /// The walking interchanges a waypoint could shape: the nearest walk with
    /// an end within reach of it, and whether it is already on it.
    fn nearby_transfer(&self, waypoint: MapId, pos: GeoPos) -> Option<(MapId, bool)> {
        let mut best: Option<(f32, MapId, bool)> = None;
        for (id, transfer) in &self.project.transfers {
            let (Some(a), Some(b)) = (
                self.project.stations.get(&transfer.a),
                self.project.stations.get(&transfer.b),
            ) else {
                continue;
            };
            let distance =
                crate::model::distance_km(pos, a.pos).min(crate::model::distance_km(pos, b.pos));
            if distance > crate::model::WALK_KM {
                continue;
            }
            if best.is_none_or(|(closest, _, _)| distance < closest) {
                best = Some((distance, *id, transfer.via.contains(&waypoint)));
            }
        }
        best.map(|(_, id, attached)| (id, attached))
    }

    /// The lines a loop can be made of at this node: those the node is either
    /// the start of, or two or more nodes into, so there is a route to close.
    fn loopable_lines(&self, node: Node) -> Vec<MapId> {
        self.project
            .lines
            .iter()
            .filter(|(id, _)| self.project.loop_index(**id, node).is_some())
            .map(|(id, _)| *id)
            .collect()
    }

    pub(crate) fn loop_menu(&mut self, ctx: &egui::Context) {
        let Some((node, at, opened)) = self.loop_menu else {
            return;
        };
        let lines = self.loopable_lines(node);
        if lines.is_empty() {
            self.loop_menu = None;
            return;
        }
        let mut chosen: Option<MapId> = None;
        let area = egui::Area::new(Id::unique("loop-menu"))
            .order(Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(150.0);
                        ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                        for id in &lines {
                            let Some(line) = self.project.lines.get(id) else {
                                continue;
                            };
                            if line_row(ui, &line.name, line.color, false, false, false).clicked() {
                                chosen = Some(*id);
                            }
                        }
                    });
            });

        let clicked = ctx.input(|input| input.pointer.any_click());
        let outside = clicked && !area.response.contains_pointer();
        let escaped = ctx.input(|input| input.key_pressed(Key::Escape));
        if let Some(id) = chosen {
            self.snapshot();
            self.project.make_loop(id, node);
            self.loop_menu = None;
        } else if escaped || (outside && ctx.cumulative_pass_nr() > opened) {
            self.loop_menu = None;
        }
    }

    fn panel(&mut self, ctx: &egui::Context) {
        if self.canvas.width() < 1.0 {
            return;
        }
        match self.selection {
            Selection::Line(id) => return self.line_panel(ctx, id),
            Selection::Transfer(id) => return self.transfer_panel(ctx, id),
            _ => {}
        }
        let Some(node) = self.selection_node() else {
            return;
        };
        let mut lines: Vec<(MapId, String, [u8; 3], Option<MapId>)> = self
            .project
            .lines
            .iter()
            .map(|(id, line)| (*id, line.name.clone(), line.color, None))
            .collect();
        if let Selection::Station(id) = self.selection {
            // Lines a walk away are listed too, marked as reached on foot.
            for (line_id, walk) in self.project.walked_lines(id) {
                if let Some(line) = self.project.lines.get(&line_id) {
                    lines.push((line_id, line.name.clone(), line.color, Some(walk)));
                }
            }
        }
        let mut action = PanelAction::default();

        egui::Area::new(Id::unique("panel"))
            .fixed_pos(Pos2::new(
                self.canvas.right() - EDGE,
                self.canvas.top() + EDGE,
            ))
            .pivot(Align2::RIGHT_TOP)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(PANEL_WIDTH);
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 8.0);

                        let mut walked = false;
                        match self.selection {
                            Selection::Station(id) => {
                                self.station_header(ui, id, &mut action);
                                self.station_elevation_row(ui, id);
                                panel_divider(ui);
                                walked = self.walks_section(ui, id);
                                if walked {
                                    panel_divider(ui);
                                }
                            }
                            Selection::Waypoint(_) => self.close_row(ui, &mut action),
                            Selection::Line(_) | Selection::Transfer(_) | Selection::None => {}
                        }
                        let listed = self.lines_section(ui, &lines, node, &mut action);
                        if listed || walked {
                            panel_divider(ui);
                        }
                        ui.allocate_ui_with_layout(
                            Vec2::new(PANEL_WIDTH, 20.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                if trash_button(ui).clicked() {
                                    action.delete = true;
                                }
                            },
                        );
                    });
            });

        if action.delete {
            self.delete_selection();
            return;
        }
        if action.close {
            self.selection = Selection::None;
            return;
        }
        if let Some((line_id, node)) = action.toggle {
            self.toggle_attach(line_id, node);
        }
        if let Some(walk) = action.walk {
            if self.project.transfers.contains_key(&walk) {
                self.selection = Selection::Transfer(walk);
            }
        }
    }

    /// Whether the station is above the ground, on it, or below it.
    fn station_elevation_row(&mut self, ui: &mut Ui, id: MapId) {
        let Some(current) = self
            .project
            .stations
            .get(&id)
            .map(|station| station.elevation)
        else {
            return;
        };
        // The station's own mark at the grade it has, so the control shows
        // what it does to the map.
        let roles = self.project.node_roles();
        let mark = station_mark(roles.degree(id) >= 2, roles.is_terminal(id));
        let mut picked: Option<Elevation> = None;
        ui.horizontal(|ui| {
            station_grade_glyph(ui, current, mark);
            ui.menu_button(RichText::new(current.label()).font(medium(11.5)), |ui| {
                ui.set_min_width(112.0);
                for level in Elevation::ALL {
                    if ui
                        .button(RichText::new(level.label()).font(medium(12.0)))
                        .clicked()
                    {
                        picked = Some(level);
                        ui.close();
                    }
                }
            });
        });
        if let Some(level) = picked {
            if level == current {
                return;
            }
            self.snapshot();
            if let Some(station) = self.project.stations.get_mut(&id) {
                station.elevation = level;
            }
        }
    }

    /// Walks from a station: those already made, then every station within a
    /// walk, each naming the lines the walk would open up. Returns whether
    /// anything was drawn.
    fn walks_section(&mut self, ui: &mut Ui, id: MapId) -> bool {
        let partners = self.project.walk_partners(id);
        let nearby = self.project.stations_within_walk(id);
        let gains = nearby
            .iter()
            .any(|(other, _)| !self.project.lines_gained(id, *other).is_empty());
        if partners.is_empty() && !gains {
            return false;
        }

        let mut open: Option<MapId> = None;
        let mut make: Option<MapId> = None;
        for (other, km) in &partners {
            let chips = self.walk_chips(id, *other);
            let made = self.project.transfer_between(id, *other);
            if walk_row(ui, &self.station_name(*other), *km, &chips, true).clicked() {
                open = made;
            }
        }
        for (other, km) in nearby.iter().take(WALK_SUGGESTIONS) {
            let chips = self.walk_chips(id, *other);
            if walk_row(ui, &self.station_name(*other), *km, &chips, false).clicked() {
                make = Some(*other);
            }
        }

        if let Some(other) = make {
            self.walk_to(id, other);
        }
        if let Some(walk) = open {
            self.selection = Selection::Transfer(walk);
        }
        true
    }

    fn station_name(&self, id: MapId) -> String {
        self.project
            .stations
            .get(&id)
            .map(|station| station.name.clone())
            .unwrap_or_default()
    }

    /// The lines a walk would open up, as colours.
    fn walk_chips(&self, station: MapId, other: MapId) -> Vec<[u8; 3]> {
        self.project
            .lines_gained(station, other)
            .into_iter()
            .filter_map(|line| self.project.lines.get(&line).map(|line| line.color))
            .take(WALK_CHIPS)
            .collect()
    }

    fn walk_to(&mut self, a: MapId, b: MapId) {
        if self.project.transfer_between(a, b).is_some() {
            return;
        }
        self.snapshot();
        self.project.add_transfer(a, b);
    }

    fn close_row(&mut self, ui: &mut Ui, action: &mut PanelAction) {
        ui.allocate_ui_with_layout(
            Vec2::new(PANEL_WIDTH, 18.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                if close_button(ui).clicked() {
                    action.close = true;
                }
            },
        );
    }

    fn station_header(&mut self, ui: &mut Ui, id: MapId, action: &mut PanelAction) {
        let Some(mut name) = self.project.stations.get(&id).map(|s| s.name.clone()) else {
            action.close = true;
            return;
        };
        ui.horizontal(|ui| {
            let response = ui.add(name_edit(&mut name, PANEL_WIDTH - 26.0, 16.5));
            if self.focus_name == Some(id) {
                response.request_focus();
                self.focus_name = None;
            }
            if response.changed() {
                if let Some(station) = self.project.stations.get_mut(&id) {
                    station.name = name;
                }
            }
            if close_button(ui).clicked() {
                action.close = true;
            }
        });
    }

    /// The panel for a line: its stops in order down a rail in the line's own
    /// colour, with the lines that can be changed to at each of them.
    fn line_panel(&mut self, ctx: &egui::Context, id: MapId) {
        let Some(line) = self.project.lines.get(&id) else {
            self.selection = Selection::None;
            return;
        };
        let name = line.name.clone();
        let color = line.color;
        let nodes = line.nodes.clone();
        let roles = self.project.node_roles();
        let stops: Vec<StopRow> = nodes
            .iter()
            .filter_map(|node| match node {
                Node::Station(station) => Some(*station),
                Node::Waypoint(_) => None,
            })
            .filter_map(|station| {
                let stop = self.project.stations.get(&station)?;
                let mut others: Vec<[u8; 3]> = self
                    .project
                    .lines
                    .iter()
                    .filter(|(other, _)| **other != id)
                    .filter(|(other, _)| !self.project.line_hidden(**other))
                    .filter(|(_, other)| other.nodes.contains(&Node::Station(station)))
                    .map(|(_, other)| other.color)
                    .collect();
                // Lines a walk away count as a change too, as they do on the map.
                for (other, _) in self.project.walked_lines(station) {
                    if let Some(other) = self.project.lines.get(&other) {
                        if !other.nodes.is_empty() && !others.contains(&other.color) {
                            others.push(other.color);
                        }
                    }
                }
                Some(StopRow {
                    station,
                    name: stop.name.clone(),
                    others,
                    terminal: roles.is_terminal(station),
                    grade: stop.elevation,
                })
            })
            .collect();

        let mut pick: Option<MapId> = None;
        let mut close = false;
        egui::Area::new(Id::unique("line-panel"))
            .fixed_pos(Pos2::new(
                self.canvas.right() - EDGE,
                self.canvas.top() + EDGE,
            ))
            .pivot(Align2::RIGHT_TOP)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(PANEL_WIDTH);
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(ellipsize(
                                    ui.painter(),
                                    &name,
                                    &medium(13.5),
                                    PANEL_WIDTH - 26.0,
                                ))
                                .font(medium(13.5))
                                .color(theme::ink()),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if close_button(ui).clicked() {
                                    close = true;
                                }
                            });
                        });
                        panel_divider(ui);
                        if stops.is_empty() {
                            return;
                        }
                        // The rows are a fixed height, so the list is sized to
                        // them: a scroll area left to its own devices clips the
                        // last one inside a box that is still growing.
                        let height = (stops.len() as f32 * STATION_ROW).min(LIST_HEIGHT);
                        let width = ui.available_width();
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, height),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);
                                        let rail = Stroke::new(3.0, theme::rgb(color));
                                        for (index, row) in stops.iter().enumerate() {
                                            let StopRow {
                                                station,
                                                name: stop,
                                                others,
                                                terminal,
                                                grade,
                                            } = row;
                                            let (rect, response) = ui.allocate_exact_size(
                                                Vec2::new(ui.available_width(), STATION_ROW),
                                                Sense::click(),
                                            );
                                            let painter = ui.painter();
                                            if response.hovered() {
                                                painter.rect_filled(
                                                    rect,
                                                    CornerRadius::same(5),
                                                    theme::raised_hover(),
                                                );
                                            }
                                            let x = rect.left() + 7.0;
                                            let mid = rect.center().y;
                                            if index > 0 {
                                                painter.line_segment(
                                                    [Pos2::new(x, rect.top()), Pos2::new(x, mid)],
                                                    rail,
                                                );
                                            }
                                            if index + 1 < stops.len() {
                                                painter.line_segment(
                                                    [
                                                        Pos2::new(x, mid),
                                                        Pos2::new(x, rect.bottom()),
                                                    ],
                                                    rail,
                                                );
                                            }
                                            // Each stop is drawn at its own grade,
                                            // so a buried station looks buried here too.
                                            let stop_ink = station_ink(*grade);
                                            let mark = Pos2::new(x, mid);
                                            if *terminal {
                                                painter.add(Shape::convex_polygon(
                                                    rounded_square_points(mark, 4.4, 1.8),
                                                    stop_ink.fill,
                                                    Stroke::new(
                                                        stop_ink.weight - 0.5,
                                                        stop_ink.edge,
                                                    ),
                                                ));
                                            } else {
                                                painter.circle(
                                                    mark,
                                                    3.2 * stop_ink.scale,
                                                    stop_ink.fill,
                                                    Stroke::new(
                                                        stop_ink.weight - 0.4,
                                                        stop_ink.edge,
                                                    ),
                                                );
                                            }
                                            let chips = others.len().min(4) as f32;
                                            let room =
                                                (rect.width() - 24.0 - chips * 10.0).max(20.0);
                                            let font = medium(12.0);
                                            painter.text(
                                                Pos2::new(x + 12.0, mid),
                                                Align2::LEFT_CENTER,
                                                ellipsize(painter, stop, &font, room),
                                                font,
                                                theme::ink(),
                                            );
                                            for (slot, chip) in others.iter().take(4).enumerate() {
                                                let cx = rect.right() - 8.0 - slot as f32 * 10.0;
                                                painter.rect_filled(
                                                    Rect::from_center_size(
                                                        Pos2::new(cx, mid),
                                                        Vec2::splat(6.0),
                                                    ),
                                                    CornerRadius::same(1),
                                                    theme::rgb(*chip),
                                                );
                                            }
                                            if response.clicked() {
                                                pick = Some(*station);
                                            }
                                        }
                                    });
                            },
                        );
                    });
            });

        if let Some(station) = pick {
            self.selection = Selection::Station(station);
        } else if close {
            self.selection = Selection::None;
        }
    }

    /// The panel for a walking interchange: the two stations it joins, and the
    /// way to drop it.
    fn transfer_panel(&mut self, ctx: &egui::Context, id: MapId) {
        let Some(transfer) = self.project.transfers.get(&id) else {
            self.selection = Selection::None;
            return;
        };
        let end = |station: MapId| {
            self.project
                .stations
                .get(&station)
                .map_or_else(String::new, |stop| stop.name.clone())
        };
        let label = format!("{} ↔ {}", end(transfer.a), end(transfer.b));
        let mut close = false;
        let mut remove = false;
        egui::Area::new(Id::unique("transfer-panel"))
            .fixed_pos(Pos2::new(
                self.canvas.right() - EDGE,
                self.canvas.top() + EDGE,
            ))
            .pivot(Align2::RIGHT_TOP)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(PANEL_WIDTH);
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 8.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(ellipsize(
                                    ui.painter(),
                                    &label,
                                    &medium(13.0),
                                    PANEL_WIDTH - 26.0,
                                ))
                                .font(medium(13.0))
                                .color(theme::ink()),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if close_button(ui).clicked() {
                                    close = true;
                                }
                            });
                        });
                        panel_divider(ui);
                        ui.allocate_ui_with_layout(
                            Vec2::new(PANEL_WIDTH, 20.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                if trash_button(ui).clicked() {
                                    remove = true;
                                }
                            },
                        );
                    });
            });
        if remove {
            self.delete_selection();
        } else if close {
            self.selection = Selection::None;
        }
    }

    /// The network at a glance: how much of it there is, and how far it runs.
    fn stats(&self, ctx: &egui::Context) {
        if !self.stats_open || self.canvas.width() < 1.0 {
            return;
        }
        let text = format!(
            "{}  {}  {}  {}",
            plural(self.project.stations.len(), "station"),
            plural(self.project.waypoints.len(), "waypoint"),
            plural(self.project.lines.len(), "line"),
            kilometres(self.project.total_length_km()),
        );
        egui::Area::new(Id::unique("stats"))
            .default_pos(Pos2::new(
                self.canvas.left() + EDGE,
                self.canvas.bottom() - 48.0,
            ))
            .order(Order::Middle)
            .movable(true)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(text)
                                .font(medium(11.0))
                                .color(theme::ink_dim()),
                        );
                    });
            });
    }

    /// A movable box for the lines themselves: create one, then edit its name
    /// and colour. Folded to a small tab until it is opened.
    fn lines_box(&mut self, ctx: &egui::Context) {
        if self.canvas.width() < 1.0 {
            return;
        }
        egui::Area::new(Id::unique("lines-box"))
            .default_pos(Pos2::new(
                self.canvas.left() + EDGE,
                self.canvas.top() + EDGE,
            ))
            .order(Order::Middle)
            .movable(true)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(9))
                    .show(ui, |ui| {
                        ui.set_width(if self.lines_open { 214.0 } else { 84.0 });
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 7.0);
                        ui.horizontal(|ui| {
                            if chevron(ui, self.lines_open).clicked() {
                                self.lines_open = !self.lines_open;
                            }
                            ui.label(
                                RichText::new("Lines")
                                    .font(medium(12.5))
                                    .color(theme::ink()),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if plus_button(ui).clicked() {
                                    self.create_line();
                                }
                            });
                        });
                        if self.lines_open {
                            self.lines_body(ui);
                        }
                    });
            });
    }

    fn lines_body(&mut self, ui: &mut Ui) {
        let lines: Vec<(MapId, Option<MapId>, String, [u8; 3])> = self
            .project
            .lines
            .iter()
            .map(|(id, line)| (*id, line.group, line.name.clone(), line.color))
            .collect();
        let groups: Vec<(MapId, String, bool, bool)> = self
            .project
            .groups
            .iter()
            .map(|(id, group)| (*id, group.name.clone(), group.open, group.hidden))
            .collect();
        if lines.is_empty() && groups.is_empty() {
            return;
        }
        ScrollArea::vertical()
            .max_height(160.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                let mut fold: Option<(MapId, bool)> = None;
                for (group_id, group_name, open, hidden) in &groups {
                    let (arrow, heading) = group_row(
                        ui,
                        *group_id,
                        group_name,
                        self.group_pick == Some(*group_id),
                        *open,
                        *hidden,
                    );
                    if arrow.clicked() {
                        fold = Some((*group_id, !*open));
                    }
                    if heading.clicked() {
                        self.group_pick = Some(*group_id);
                        self.lines_pick = None;
                    }
                    if !*open {
                        continue;
                    }
                    for (id, group, name, color) in &lines {
                        if *group != Some(*group_id) {
                            continue;
                        }
                        if line_row(ui, name, *color, false, self.lines_pick == Some(*id), false)
                            .clicked()
                        {
                            self.lines_pick = Some(*id);
                            self.group_pick = None;
                        }
                    }
                }
                if let Some((id, open)) = fold {
                    if let Some(group) = self.project.groups.get_mut(&id) {
                        group.open = open;
                    }
                }
                // Lines whose group has gone, or that were never filed.
                for (id, group, name, color) in &lines {
                    let filed =
                        group.is_some_and(|group| groups.iter().any(|(id, _, _, _)| *id == group));
                    if filed {
                        continue;
                    }
                    if line_row(ui, name, *color, false, self.lines_pick == Some(*id), false)
                        .clicked()
                    {
                        self.lines_pick = Some(*id);
                        self.group_pick = None;
                    }
                }
            });
        // The list ends here; what follows edits the one picked from it.
        let group_picked = self
            .group_pick
            .is_some_and(|id| self.project.groups.contains_key(&id));
        let line_picked = self
            .lines_pick
            .is_some_and(|id| self.project.lines.contains_key(&id));
        if group_picked || line_picked {
            panel_divider(ui);
        }
        match self.group_pick {
            Some(id) if self.project.groups.contains_key(&id) => self.group_editor(ui, id),
            Some(_) => self.group_pick = None,
            None => {}
        }
        if self.group_pick.is_none() {
            match self.lines_pick {
                Some(id) if self.project.lines.contains_key(&id) => self.line_editor(ui, id),
                Some(_) => self.lines_pick = None,
                None => {}
            }
        }
    }

    /// The editor under the list for a group picked by its heading.
    fn group_editor(&mut self, ui: &mut Ui, id: MapId) {
        let Some((mut name, hidden)) = self
            .project
            .groups
            .get(&id)
            .map(|group| (group.name.clone(), group.hidden))
        else {
            self.group_pick = None;
            return;
        };
        let mut remove = false;
        let mut hide = false;
        ui.horizontal(|ui| {
            let response = ui.add(name_edit(&mut name, 148.0, 13.5));
            if response.changed() {
                if let Some(group) = self.project.groups.get_mut(&id) {
                    group.name = name;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if trash_button(ui).clicked() {
                    remove = true;
                }
                if eye_button(ui, hidden).clicked() {
                    hide = true;
                }
            });
        });
        if hide {
            self.snapshot();
            if let Some(group) = self.project.groups.get_mut(&id) {
                group.hidden = !group.hidden;
            }
        }
        if remove {
            self.snapshot();
            self.project.remove_group(id);
            self.group_pick = None;
        }
    }

    fn line_editor(&mut self, ui: &mut Ui, id: MapId) {
        let Some(color) = self.project.lines.get(&id).map(|line| line.color) else {
            return;
        };
        let mut remove = false;
        ui.horizontal(|ui| {
            let mut name = self
                .project
                .lines
                .get(&id)
                .map(|line| line.name.clone())
                .unwrap_or_default();
            let response = ui.add(name_edit(&mut name, 148.0, 13.5));
            if response.changed() {
                if let Some(line) = self.project.lines.get_mut(&id) {
                    line.name = name;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if trash_button(ui).clicked() {
                    remove = true;
                }
            });
        });
        ui.horizontal(|ui| {
            let mut rgb = color;
            let picker = ui.color_edit_button_srgb(&mut rgb);
            if picker.clicked() {
                // One undo step per visit to the picker, not per shade crossed.
                self.snapshot();
            }
            if picker.changed() {
                self.set_line_color(id, rgb);
            }
            let current = self
                .project
                .lines
                .get(&id)
                .map(|line| line.color)
                .unwrap_or(color);
            if self.hex_line != Some(id) {
                self.hex_line = Some(id);
                self.hex_text = hex_string(current);
            }
            let response = ui.add(hex_edit(&mut self.hex_text));
            if response.gained_focus() {
                self.snapshot();
            }
            if response.changed() {
                if let Some(rgb) = parse_hex(&self.hex_text) {
                    self.set_line_color(id, rgb);
                }
            } else if !response.has_focus() && parse_hex(&self.hex_text) != Some(current) {
                // Picked elsewhere or undone: show the colour the line really has.
                self.hex_text = hex_string(current);
            }
        });
        self.line_mode_row(ui, id);
        self.line_pattern_row(ui, id);
        self.line_group_row(ui, id);
        self.line_loop_row(ui, id);
        if remove {
            self.snapshot();
            self.project.remove_line(id);
            if self.selection == Selection::Line(id) {
                self.selection = Selection::None;
            }
            self.lines_pick = None;
        }
    }

    /// What the line carries, and the way to change it.
    fn line_mode_row(&mut self, ui: &mut Ui, id: MapId) {
        let current = self
            .project
            .lines
            .get(&id)
            .map(|line| line.mode)
            .unwrap_or_default();
        let mut choice = None;
        ui.horizontal(|ui| {
            ui.menu_button(RichText::new(current.label()).font(medium(11.5)), |ui| {
                ui.set_min_width(150.0);
                for mode in Mode::ALL {
                    let label = RichText::new(mode.label()).font(medium(12.0));
                    if ui.selectable_label(mode == current, label).clicked() {
                        choice = Some(mode);
                        ui.close();
                    }
                }
            });
        });
        if let Some(mode) = choice {
            self.snapshot();
            if let Some(line) = self.project.lines.get_mut(&id) {
                line.mode = mode;
            }
        }
    }

    /// How the line is stroked, and the way to change it.
    fn line_pattern_row(&mut self, ui: &mut Ui, id: MapId) {
        let current = self
            .project
            .lines
            .get(&id)
            .map(|line| line.pattern)
            .unwrap_or_default();
        let mut choice = None;
        ui.horizontal(|ui| {
            ui.menu_button(RichText::new(current.label()).font(medium(11.5)), |ui| {
                ui.set_min_width(150.0);
                for pattern in Pattern::ALL {
                    let label = RichText::new(pattern.label()).font(medium(12.0));
                    if ui.selectable_label(pattern == current, label).clicked() {
                        choice = Some(pattern);
                        ui.close();
                    }
                }
            });
        });
        if let Some(pattern) = choice {
            self.snapshot();
            if let Some(line) = self.project.lines.get_mut(&id) {
                line.pattern = pattern;
            }
        }
    }

    /// The group the line is filed under, and the way to make a new one.
    fn line_group_row(&mut self, ui: &mut Ui, id: MapId) {
        let current = self
            .project
            .lines
            .get(&id)
            .and_then(|line| line.group)
            .and_then(|group| self.project.groups.get(&group))
            .map(|group| group.name.clone());
        let mut assign: Option<Option<MapId>> = None;
        let mut create = false;
        ui.horizontal(|ui| {
            let label = current.clone().unwrap_or_else(|| "Group".to_owned());
            ui.menu_button(RichText::new(label).font(medium(11.5)), |ui| {
                ui.set_min_width(130.0);
                if ui
                    .button(RichText::new("No group").font(medium(12.0)))
                    .clicked()
                {
                    assign = Some(None);
                    ui.close();
                }
                for (group, data) in &self.project.groups {
                    if ui
                        .button(RichText::new(data.name.clone()).font(medium(12.0)))
                        .clicked()
                    {
                        assign = Some(Some(*group));
                        ui.close();
                    }
                }
                ui.separator();
                if ui
                    .button(RichText::new("New group").font(medium(12.0)))
                    .clicked()
                {
                    create = true;
                    ui.close();
                }
            });
            // The button carries the group's name, so nothing repeats it here.
        });
        if let Some(group) = assign {
            self.snapshot();
            if let Some(line) = self.project.lines.get_mut(&id) {
                line.group = group;
            }
        }
        if create {
            self.snapshot();
            let name = self.next_group_name();
            let group = self.project.add_group(name);
            if let Some(line) = self.project.lines.get_mut(&id) {
                line.group = Some(group);
            }
            self.lines_pick = None;
            self.group_pick = Some(group);
        }
    }

    /// How the line closes, if it does: over itself, or doubling back.
    fn line_loop_row(&mut self, ui: &mut Ui, id: MapId) {
        let (closed, turnback) = self
            .project
            .lines
            .get(&id)
            .map(|line| (line.closed, line.turnback))
            .unwrap_or((false, false));
        let mut choice: Option<Option<bool>> = None;
        ui.horizontal(|ui| {
            let label = if !closed {
                "Loop"
            } else if turnback {
                "Turnback"
            } else {
                "Crossing"
            };
            ui.menu_button(RichText::new(label).font(medium(11.5)), |ui| {
                ui.set_min_width(120.0);
                for (text, value) in [
                    ("Off", None),
                    ("Turnback", Some(true)),
                    ("Crossing", Some(false)),
                ] {
                    if ui.button(RichText::new(text).font(medium(12.0))).clicked() {
                        choice = Some(value);
                        ui.close();
                    }
                }
            });
        });
        if let Some(choice) = choice {
            self.snapshot();
            if let Some(line) = self.project.lines.get_mut(&id) {
                line.closed = choice.is_some();
                line.turnback = choice.unwrap_or(false);
            }
        }
    }

    fn set_line_color(&mut self, id: MapId, rgb: [u8; 3]) {
        let Some(line) = self.project.lines.get_mut(&id) else {
            return;
        };
        if line.color != rgb {
            line.color = rgb;
        }
    }

    fn lines_section(
        &mut self,
        ui: &mut Ui,
        lines: &[(MapId, String, [u8; 3], Option<MapId>)],
        node: Node,
        action: &mut PanelAction,
    ) -> bool {
        if lines.is_empty() {
            return false;
        }
        ui.add(search_edit(&mut self.line_query, PANEL_WIDTH));

        let query = self.line_query.trim().to_lowercase();
        ScrollArea::vertical()
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                for (id, name, color, walk) in lines {
                    if !query.is_empty() && !name.to_lowercase().contains(&query) {
                        continue;
                    }
                    let Some(walk) = walk else {
                        let attached = self.project.has_node(*id, node);
                        let response = line_row(ui, name, *color, attached, false, false);
                        if !attached && response.hovered() {
                            self.preview = Some((*id, node));
                        }
                        if response.clicked() {
                            action.toggle = Some((*id, node));
                        }
                        continue;
                    };
                    // Reached on foot: nothing to attach, but it can be shown.
                    if line_row(ui, name, *color, false, false, true).clicked() {
                        action.walk = Some(*walk);
                    }
                }
            });
        true
    }

    /// Edit → Preferences…: settings, in categories.
    fn preferences(&mut self, ctx: &egui::Context) {
        if !self.preferences {
            // No key waits on a panel that is not there.
            self.listening = None;
            return;
        }
        let mut open = true;
        let mut chosen = None;
        let mut chosen_mode = None;
        // Clear of the Lines tab, which lives in the top-left corner.
        let width = PREFS_RAIL + 16.0 + PREVIEW_SIDE * 3.0 + PREVIEW_GAP * 2.0;
        let viewport = ctx.viewport_rect();
        let start = Pos2::new((viewport.center().x - width * 0.5).max(24.0), 60.0);
        egui::Area::new(Id::unique("preferences"))
            .default_pos(start)
            .order(Order::Foreground)
            .movable(true)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(14))
                    .shadow(ui.visuals().window_shadow)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            let title = RichText::new("Preferences").font(medium(12.5));
                            ui.label(title.color(theme::ink()));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if close_button(ui).clicked() {
                                    open = false;
                                }
                            });
                        });
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 16.0;
                            ui.vertical(|ui| {
                                ui.set_width(PREFS_RAIL);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                for (index, name) in PREFS_TABS.iter().enumerate() {
                                    let selected = self.prefs_tab == index;
                                    let label = RichText::new(*name).font(medium(12.0));
                                    if ui.selectable_label(selected, label).clicked() {
                                        self.prefs_tab = index;
                                    }
                                }
                            });
                            ui.vertical(|ui| match self.prefs_tab {
                                KEYS_TAB => self.keys_tab(ui),
                                THEME_TAB => {
                                    ui.set_width(PREVIEW_SIDE * 3.0 + PREVIEW_GAP * 2.0);
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    for mode in theme::Mode::ALL {
                                        let label = RichText::new(mode.label()).font(medium(12.0));
                                        if ui.selectable_label(self.mode == mode, label).clicked() {
                                            chosen_mode = Some(mode);
                                        }
                                    }
                                }
                                _ => {
                                    ui.spacing_mut().item_spacing.y = PREVIEW_GAP;
                                    for row in Basemap::ALL.chunks(3) {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = PREVIEW_GAP;
                                            for basemap in row {
                                                let current = self.basemap == *basemap;
                                                if preview_cell(
                                                    ui,
                                                    *basemap,
                                                    current,
                                                    &mut self.tiles,
                                                ) {
                                                    chosen = Some(*basemap);
                                                }
                                            }
                                        });
                                    }
                                }
                            });
                        });
                    });
            });
        self.preferences = open;
        // Nor on a category that is out of sight.
        if !self.preferences || self.prefs_tab != KEYS_TAB {
            self.listening = None;
        }
        if let Some(basemap) = chosen {
            self.set_basemap(basemap);
        }
        if let Some(mode) = chosen_mode {
            self.mode = mode;
            theme::apply(ctx, mode);
            ctx.request_repaint();
        }
    }

    /// Preferences → Keys: one row per command, the key at its right. Clicking
    /// a row takes the next key pressed; Escape backs out.
    fn keys_tab(&mut self, ui: &mut Ui) {
        ui.set_width(PREVIEW_SIDE * 3.0 + PREVIEW_GAP * 2.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        let mut clicked = false;
        for action in keys::ACTIONS {
            let listening = self.listening == Some(action);
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), KEY_ROW), Sense::click());
            let painter = ui.painter();
            if listening {
                painter.rect_filled(rect, CornerRadius::same(6), theme::raised());
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(6),
                    Stroke::new(1.0, theme::accent()),
                    StrokeKind::Inside,
                );
            } else if response.hovered() {
                painter.rect_filled(rect, CornerRadius::same(6), theme::raised_hover());
            }
            painter.text(
                rect.left_center() + Vec2::new(8.0, 0.0),
                Align2::LEFT_CENTER,
                action.label(),
                medium(12.0),
                theme::ink(),
            );
            let key = if listening {
                "…".to_owned()
            } else {
                let key = self.keys.label(ui.ctx(), action);
                if key.is_empty() {
                    "—".to_owned()
                } else {
                    key
                }
            };
            painter.text(
                rect.right_center() - Vec2::new(8.0, 0.0),
                Align2::RIGHT_CENTER,
                key,
                medium(11.5),
                if listening {
                    theme::accent()
                } else {
                    theme::ink_faint()
                },
            );
            if response.clicked() {
                self.listening = Some(action);
                clicked = true;
            }
        }
        // A click anywhere else puts the change back.
        if !clicked && ui.input(|input| input.pointer.any_click()) {
            self.listening = None;
        }
        ui.add_space(2.0);
        if ui
            .button(RichText::new("Defaults").font(medium(11.5)))
            .clicked()
        {
            self.keys.reset();
        }
    }

    fn attribution(&self, ctx: &egui::Context) {
        let (credit, link) = self.basemap.attribution();
        // Ink that suits the map under it, so the credit reads on any basemap.
        let (ink, _) = self.map_ink();
        egui::Area::new(Id::unique("attribution"))
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-EDGE + 2.0, -12.0))
            // Under the panels and windows that can cover it.
            .order(Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(
                        RichText::new("©")
                            .font(medium(10.5))
                            .color(theme::alpha(ink, 210)),
                    );
                    ui.hyperlink_to(
                        RichText::new(credit)
                            .font(medium(10.5))
                            .color(theme::alpha(ink, 210)),
                        link,
                    );
                });
            });
    }

    fn hint(&self, ctx: &egui::Context) {
        if !self.project.stations.is_empty() || !self.project.waypoints.is_empty() {
            return;
        }
        egui::Area::new(Id::unique("hint"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -22.0))
            .order(Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::overlay())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(255))
                    .inner_margin(Margin::symmetric(14, 7))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Click to place a station")
                                .font(medium(11.5))
                                .color(theme::ink_dim()),
                        );
                    });
            });
    }

    fn error_toast(&self, ctx: &egui::Context) {
        let Some((message, until)) = &self.error else {
            return;
        };
        if self.now > *until {
            return;
        }
        egui::Area::new(Id::unique("error"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -22.0))
            .order(Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(255))
                    .inner_margin(Margin::symmetric(14, 7))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(message)
                                .font(medium(11.5))
                                .color(theme::ink()),
                        );
                    });
            });
        // Come back when it goes, so nothing has to wait for a mouse move.
        ctx.request_repaint_after_secs((until - self.now).max(0.0) as f32);
    }

    // ---------------------------------------------------------------- projects

    /// The list the app opens on: the maps it has kept, where they live, and
    /// the way to any other one.
    fn launcher(&mut self, ctx: &egui::Context) {
        let mut open: Option<usize> = None;
        let mut forget: Option<usize> = None;
        let mut rename_row: Option<(usize, String)> = None;
        let mut browse = false;
        let mut create = false;

        // A row is a name, and — on the desktop — the file it lives in.
        #[cfg(not(target_arch = "wasm32"))]
        let rows: Vec<(String, Option<String>)> = self
            .projects
            .iter()
            .map(|path| (map_name(path), Some(path.display().to_string())))
            .collect();
        #[cfg(target_arch = "wasm32")]
        let rows: Vec<(String, Option<String>)> = self
            .web_projects
            .iter()
            .map(|name| (name.clone(), None))
            .collect();

        egui::Area::new(Id::unique("projects"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(LAUNCH_WIDTH);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new("Metrocraft")
                        .font(medium(13.0))
                        .color(theme::ink()),
                );
                ui.add_space(14.0);
                // A whole account can land in the list at once, so it scrolls.
                let room = (ctx.content_rect().height() - 220.0).max(LAUNCH_ROW * 3.0);
                ScrollArea::vertical()
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        for (index, (name, place)) in rows.iter().enumerate() {
                            let (rect, response) = ui
                                .allocate_exact_size(Vec2::new(width, LAUNCH_ROW), Sense::click());
                            // Read from the pointer rather than the row, so the
                            // marks stay while the pointer is over one of them.
                            let hovered = ui.is_rect_visible(rect)
                                && ctx
                                    .pointer_latest_pos()
                                    .is_some_and(|pos| rect.contains(pos));
                            let painter = ui.painter();
                            if hovered {
                                painter.rect_filled(
                                    rect,
                                    CornerRadius::same(8),
                                    theme::raised_hover(),
                                );
                            }
                            // A name stands alone where there is no file under it.
                            let top = match place {
                                Some(_) => rect.top() + 8.0,
                                None => rect.center().y - 8.0,
                            };
                            paint_line(
                                painter,
                                Pos2::new(rect.left() + 12.0, top),
                                name,
                                medium(12.5),
                                theme::ink(),
                                width - 72.0,
                            );
                            if let Some(place) = place {
                                paint_line(
                                    painter,
                                    rect.left_top() + Vec2::new(12.0, 26.0),
                                    place,
                                    medium(11.0),
                                    theme::ink_faint(),
                                    width - 72.0,
                                );
                            }

                            // Two marks under the cursor: another name, and off
                            // the list.
                            let mut on_mark = false;
                            if hovered {
                                let edit = Rect::from_center_size(
                                    Pos2::new(rect.right() - 40.0, rect.center().y),
                                    Vec2::splat(18.0),
                                );
                                let cross = Rect::from_center_size(
                                    Pos2::new(rect.right() - 16.0, rect.center().y),
                                    Vec2::splat(18.0),
                                );
                                let stroke = Stroke::new(1.1, theme::ink());
                                painter_pencil(painter, edit.center(), stroke);
                                painter_cross(painter, cross.center(), 3.5, stroke);
                                let hit = ui.interact(
                                    edit,
                                    ui.scope_id().with(("rename", index)),
                                    Sense::click(),
                                );
                                if hit.clicked() {
                                    rename_row = Some((index, name.clone()));
                                    on_mark = true;
                                }
                                let hit = ui.interact(
                                    cross,
                                    ui.scope_id().with(("forget", index)),
                                    Sense::click(),
                                );
                                if hit.clicked() {
                                    forget = Some(index);
                                    on_mark = true;
                                }
                            }
                            if !on_mark && response.clicked() {
                                open = Some(index);
                            }
                        }
                    });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("Open…").font(medium(12.0)))
                        .clicked()
                    {
                        browse = true;
                    }
                    if ui
                        .button(RichText::new("New…").font(medium(12.0)))
                        .clicked()
                    {
                        create = true;
                    }
                    if ui
                        .button(RichText::new("Import…").font(medium(12.0)))
                        .clicked()
                    {
                        self.import_open = true;
                    }
                });
            });

        if let Some(index) = forget {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.projects.remove(index);
            }
            #[cfg(target_arch = "wasm32")]
            if let Some(name) = self.web_projects.get(index).cloned() {
                self.web_projects.remove(index);
                self.store_rx = Some(platform::store_delete(&name, ctx));
            }
        } else if let Some((index, name)) = rename_row {
            self.start_rename(RenameTarget::Row(index), name);
        } else if let Some(index) = open {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.open_project(index);
            }
            #[cfg(target_arch = "wasm32")]
            if let Some(name) = self.web_projects.get(index).cloned() {
                self.store_rx = Some(platform::store_open(&name, ctx));
            }
        } else if browse {
            self.open_document();
        } else if create {
            #[cfg(not(target_arch = "wasm32"))]
            self.new_project();
            #[cfg(target_arch = "wasm32")]
            self.new_browser_project();
        }
    }

    /// Open the one field `Rename…` works in, on a row or on the map in hand.
    fn start_rename(&mut self, target: RenameTarget, name: String) {
        self.rename_text = name;
        self.rename = Some(target);
        self.rename_focus = true;
    }

    /// Give a map its new name, moving whatever holds it along: a file, or a
    /// record in the browser's store.
    fn commit_rename(&mut self, target: RenameTarget, typed: &str, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = ctx;
        let Some(name) = clean_name(typed) else {
            return self.set_error("A map needs a name.");
        };
        match target {
            RenameTarget::Document => {
                #[cfg(not(target_arch = "wasm32"))]
                self.rename_document(name);
                #[cfg(target_arch = "wasm32")]
                self.rename_document(name, ctx);
            }
            RenameTarget::Row(index) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.rename_row(index, name);
                #[cfg(target_arch = "wasm32")]
                self.rename_row(index, name, ctx);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn rename_row(&mut self, index: usize, name: String) {
        let Some(path) = self.projects.get(index).cloned() else {
            return;
        };
        match move_map_file(&path, &name) {
            Ok(target) => {
                self.projects[index] = target.clone();
                // The map in hand keeps saving into its own file.
                if self.path.as_ref() == Some(&path) {
                    self.path = Some(target);
                }
            }
            Err(err) => self.set_error(err),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn rename_row(&mut self, index: usize, name: String, ctx: &egui::Context) {
        let Some(old) = self.web_projects.get(index).cloned() else {
            return;
        };
        if old == name {
            return;
        }
        if self.web_projects.contains(&name) {
            return self.set_error("A map with that name is already there.");
        }
        self.store_rx = Some(platform::store_rename(&old, &name, ctx));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn rename_document(&mut self, name: String) {
        let Some(path) = self.path.clone() else {
            // A map with no file yet just takes the name; `Save` asks where it goes.
            self.project.name = name;
            return;
        };
        match move_map_file(&path, &name) {
            Ok(target) => {
                self.project.name = name;
                if let Some(row) = self.projects.iter().position(|old| *old == path) {
                    self.projects[row] = target.clone();
                }
                self.path = Some(target);
            }
            Err(err) => self.set_error(err),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn rename_document(&mut self, name: String, ctx: &egui::Context) {
        let old = self.project.name.clone();
        if old == name {
            return;
        }
        if self.web_projects.contains(&name) {
            return self.set_error("A map with that name is already there.");
        }
        // A map the browser already keeps moves with its name; one never saved
        // is only named, and lands under that name on the first `Save`.
        if self.web_projects.contains(&old) {
            self.store_rx = Some(platform::store_rename(&old, &name, ctx));
        }
        self.project.name = name;
    }

    /// `Rename…`: one name, one field.
    fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(target) = self.rename else {
            return;
        };
        let mut close = ctx.input(|input| input.key_pressed(Key::Escape));
        let mut submit = false;
        let width = 260.0;
        egui::Area::new(Id::unique("rename"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(14))
                    .shadow(ui.visuals().window_shadow)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            let mut output = TextEdit::singleline(&mut self.rename_text)
                                .id(Id::unique("rename-name"))
                                .frame(
                                    Frame::new()
                                        .fill(theme::raised())
                                        .corner_radius(CornerRadius::same(7))
                                        .inner_margin(Margin::symmetric(8, 5)),
                                )
                                .font(medium(12.0))
                                .text_color(theme::ink())
                                .margin(Margin::symmetric(0, 0))
                                .desired_width(width - 30.0)
                                .show(ui);
                            // The name comes up chosen, so typing replaces it.
                            if self.rename_focus {
                                output.response.request_focus();
                                let end =
                                    egui::text::CCursor::new(self.rename_text.chars().count());
                                output.state.cursor.set_char_range(Some(
                                    egui::text::CCursorRange::two(egui::text::CCursor::new(0), end),
                                ));
                                output.state.store(ui.ctx(), output.response.id);
                                self.rename_focus = false;
                            }
                            let field = output.response;
                            if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))
                            {
                                submit = true;
                            }
                            if close_button(ui).clicked() {
                                close = true;
                            }
                        });
                    });
            });
        if close {
            self.rename = None;
            self.rename_text.clear();
        } else if submit {
            self.rename = None;
            let name = std::mem::take(&mut self.rename_text);
            self.commit_rename(target, &name, ctx);
        }
    }

    /// A new map from the project list, made the way that list makes them.
    fn new_listed_map(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.new_project();
        #[cfg(target_arch = "wasm32")]
        self.new_browser_project();
    }

    /// A map of the browser's own, made ready to edit.
    #[cfg(target_arch = "wasm32")]
    fn new_browser_project(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.selection = Selection::None;
        self.project = Project::new(free_name("Untitled", &self.web_projects));
        self.view = MapView::default();
        self.request_fit();
        self.screen = Screen::Editor;
    }

    /// Take up a map the browser kept, once it has been read.
    #[cfg(target_arch = "wasm32")]
    fn open_web_project(&mut self, name: String) {
        self.undo.clear();
        self.redo.clear();
        self.selection = Selection::None;
        // The name the store keeps a map under is the map's name, so `Save`
        // lands on the same record even after the row was renamed.
        self.project.name = name.clone();
        self.remember_web_project(name);
        self.screen = Screen::Editor;
    }

    #[cfg(target_arch = "wasm32")]
    fn remember_web_project(&mut self, name: String) {
        if !self.web_projects.contains(&name) {
            self.web_projects.push(name);
            self.web_projects.sort_by_key(|name| name.to_lowercase());
        }
    }

    /// Open a map the launcher lists.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_project(&mut self, index: usize) {
        let Some(path) = self.projects.get(index).cloned() else {
            return;
        };
        match platform::read(&path) {
            Ok(text) => match self.apply_json(&text) {
                Ok(()) => {
                    // Nothing from before the map was opened is worth undoing
                    // into, so the launch starts the history.
                    self.undo.clear();
                    self.redo.clear();
                    self.enter_project(path);
                }
                Err(err) => self.set_error(err),
            },
            Err(err) => self.set_error(err),
        }
    }

    /// Take up a map: keep it at the head of the list and go to it.
    #[cfg(not(target_arch = "wasm32"))]
    fn enter_project(&mut self, path: std::path::PathBuf) {
        self.remember_project(path.clone());
        self.path = Some(path);
        self.screen = Screen::Editor;
    }

    /// Put a map at the head of the list the launcher offers, without leaving
    /// the screen in hand: an account import lands here a map at a time.
    #[cfg(not(target_arch = "wasm32"))]
    fn remember_project(&mut self, path: std::path::PathBuf) {
        self.projects.retain(|old| *old != path);
        self.projects.insert(0, path);
        self.projects.truncate(PROJECTS_MAX);
    }

    /// A new map, once it has a file of its own to live in.
    #[cfg(not(target_arch = "wasm32"))]
    fn new_project(&mut self) {
        let path = match platform::choose_path("untitled.json") {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(err) => return self.set_error(err),
        };
        let name = path.file_stem().map_or_else(
            || "Untitled".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let project = Project::new(name);
        let json = match serde_json::to_string_pretty(&project) {
            Ok(json) => json,
            Err(err) => return self.set_error(format!("Could not encode this map: {err}")),
        };
        if let Err(err) = platform::write(&path, &json) {
            return self.set_error(err);
        }
        self.undo.clear();
        self.redo.clear();
        self.selection = Selection::None;
        self.project = project;
        self.view = MapView::default();
        self.request_fit();
        self.enter_project(path);
    }

    // ----------------------------------------------------------------- files

    #[cfg(not(target_arch = "wasm32"))]
    fn open_document(&mut self) {
        match platform::open() {
            Ok(Some((path, contents))) => match self.apply_json(&contents) {
                Ok(()) => self.enter_project(path),
                Err(err) => self.set_error(err),
            },
            Ok(None) => {}
            Err(err) => self.set_error(err),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn open_document(&mut self) {
        match platform::pick_json_file() {
            Ok(receiver) => self.load_rx = Some(receiver),
            Err(err) => self.set_error(err),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_document(&mut self, _ctx: &egui::Context, save_as: bool) {
        let json = match self.to_json() {
            Ok(json) => json,
            Err(err) => return self.set_error(err),
        };

        if !save_as {
            if let Some(path) = self.path.clone() {
                match platform::write(&path, &json) {
                    Ok(()) => return,
                    Err(err) => return self.set_error(err),
                }
            }
        }

        let default_name = format!("{}.json", self.file_stem());
        match platform::save_as(&default_name, &json) {
            Ok(Some(path)) => self.enter_project(path),
            Ok(None) => {}
            Err(err) => self.set_error(err),
        }
    }

    /// The browser keeps its maps itself, so saving is a request to it rather
    /// than a file. `Save as…` still hands out a copy for use elsewhere.
    #[cfg(target_arch = "wasm32")]
    fn save_document(&mut self, ctx: &egui::Context, save_as: bool) {
        let json = match self.to_json() {
            Ok(json) => json,
            Err(err) => return self.set_error(err),
        };
        if save_as {
            let filename = format!("{}.json", self.file_stem());
            if let Err(err) = platform::download_json(&filename, &json) {
                self.set_error(err);
            }
            return;
        }
        let name = self.project.name.clone();
        self.store_rx = Some(platform::store_save(&name, &json, ctx));
    }

    /// Collect what the browser's project store answered.
    #[cfg(target_arch = "wasm32")]
    fn poll_store(&mut self) {
        let Some(receiver) = &self.store_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(platform::StoreReply::Names(names))) => {
                self.store_rx = None;
                self.web_projects = names;
                self.web_projects.sort_by_key(|name| name.to_lowercase());
            }
            Ok(Ok(platform::StoreReply::Opened(name, json))) => {
                self.store_rx = None;
                match self.apply_json(&json) {
                    Ok(()) => self.open_web_project(name),
                    Err(err) => self.set_error(err),
                }
            }
            Ok(Ok(platform::StoreReply::Saved(name))) => {
                self.store_rx = None;
                self.remember_web_project(name);
            }
            Ok(Ok(platform::StoreReply::Renamed(from, to))) => {
                self.store_rx = None;
                if let Some(row) = self.web_projects.iter().position(|old| *old == from) {
                    self.web_projects[row] = to.clone();
                    self.web_projects.sort_by_key(|name| name.to_lowercase());
                }
                // The map in hand follows its record.
                if self.project.name == from {
                    self.project.name = to;
                }
            }
            Ok(Ok(platform::StoreReply::Deleted(name))) => {
                self.store_rx = None;
                self.web_projects.retain(|old| *old != name);
            }
            Ok(Err(err)) => {
                self.store_rx = None;
                self.set_error(err);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.store_rx = None,
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn poll_load(&mut self) {
        let Some(receiver) = &self.load_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(text)) => {
                self.load_rx = None;
                // A file brought in from disk is a map to edit, not a row in
                // the list.
                match self.apply_json(&text) {
                    Ok(()) => self.screen = Screen::Editor,
                    Err(err) => self.set_error(err),
                }
            }
            Ok(Err(err)) => {
                self.load_rx = None;
                self.set_error(err);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.load_rx = None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn poll_load(&mut self) {}

    // ---------------------------------------------------------------- imports

    /// Collect a MetroDreamin' read once it lands.
    fn poll_import(&mut self, ctx: &egui::Context) {
        // Only the browser's store needs the context; a file write does not.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = ctx;
        let Some(receiver) = &self.import_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(project)) => {
                self.import_rx = None;
                // A map of an account joins the project list, not the screen.
                #[cfg(target_arch = "wasm32")]
                if self.bulk.is_some() {
                    self.write_import(project, ctx);
                    return;
                }
                #[cfg(not(target_arch = "wasm32"))]
                if self.bulk.is_some() {
                    self.write_import(project);
                    return;
                }
                self.close_import();
                self.adopt(project);
            }
            Ok(Err(err)) => {
                self.import_rx = None;
                if let Some(bulk) = self.bulk.as_mut() {
                    bulk.failed += 1;
                    return;
                }
                self.set_error(err);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.import_rx = None,
        }
    }

    /// Read what was pasted: one map into the editor, or a whole account into
    /// the project list.
    fn start_import(&mut self, ctx: &egui::Context) {
        if account_link(&self.import_url) {
            self.start_bulk(ctx);
            return;
        }
        match metrodreamin::start(&self.import_url, ctx.clone()) {
            Ok(import) => self.import_rx = Some(import.receiver),
            Err(err) => self.set_error(err),
        }
    }

    /// The work an import is doing, if any: what the button says.
    fn import_busy(&self) -> Option<&'static str> {
        if self
            .bulk
            .as_ref()
            .is_some_and(|bulk| bulk.listing.is_some())
        {
            Some("Scanning…")
        } else if self.import_rx.is_some() || self.bulk.is_some() {
            Some("Importing…")
        } else {
            None
        }
    }

    /// How far an account import has come, once its maps are being read.
    fn bulk_progress(&self) -> Option<(f32, String)> {
        let bulk = self.bulk.as_ref()?;
        // Nothing to count until the scan has said how many maps there are.
        if bulk.listing.is_some() {
            return None;
        }
        // The desktop writes into a folder it has to be told about first.
        #[cfg(not(target_arch = "wasm32"))]
        bulk.folder.as_ref()?;
        let done = bulk.saved + bulk.failed;
        Some((
            done as f32 / bulk.total.max(1) as f32,
            format!("{done} / {}", bulk.total),
        ))
    }

    /// Close the import dialog, whatever it was doing.
    fn close_import(&mut self) {
        self.bulk = None;
        self.import_open = false;
        self.import_url.clear();
        self.import_rx = None;
    }

    /// Every map an account has, asked for once, then watched for.
    fn start_bulk(&mut self, ctx: &egui::Context) {
        match metrodreamin::start_listing(&self.import_url, ctx.clone()) {
            Ok(listing) => {
                self.bulk = Some(Bulk {
                    listing: Some(listing),
                    ..Bulk::default()
                });
            }
            Err(err) => self.set_error(err),
        }
    }

    /// Keep an account import moving: ask where its maps should live, then read
    /// them one at a time, writing each into the project list as it lands.
    fn poll_bulk(&mut self, ctx: &egui::Context) {
        let Some(mut bulk) = self.bulk.take() else {
            return;
        };

        // The scan answers once, with every map the account has.
        if let Some(listing) = bulk.listing.as_ref() {
            match listing.receiver.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.bulk = Some(bulk);
                    return;
                }
                Ok(Ok(maps)) if maps.is_empty() => {
                    self.set_error("No maps found for that account.");
                    self.close_import();
                    return;
                }
                Ok(Ok(maps)) => {
                    // The desktop asks where the maps should live; the browser
                    // knows: its own store.
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let Some(folder) = rfd::FileDialog::new().pick_folder() else {
                            // Cancelled: nothing is written, and the link stays put.
                            self.bulk = None;
                            return;
                        };
                        bulk.folder = Some(folder);
                    }
                    bulk.listing = None;
                    bulk.total = maps.len();
                    bulk.queue = maps.into();
                }
                Ok(Err(err)) => {
                    self.set_error(err);
                    self.close_import();
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.set_error("The account could not be read.");
                    self.close_import();
                    return;
                }
            }
        }

        // The browser puts one map in its store at a time, and the next one
        // waits for that to be answered.
        #[cfg(target_arch = "wasm32")]
        if let Some(writing) = bulk.writing.as_ref() {
            match writing.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.bulk = Some(bulk);
                    return;
                }
                Ok(Ok(reply)) => {
                    bulk.writing = None;
                    bulk.saved += 1;
                    if let platform::StoreReply::Saved(name) = reply {
                        self.remember_web_project(name);
                    }
                }
                Ok(Err(err)) => {
                    bulk.writing = None;
                    bulk.failed += 1;
                    self.set_error(err);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    bulk.writing = None;
                    bulk.failed += 1;
                }
            }
        }
        // A map still being written is work in flight like any other.
        let writing = Self::writing(&bulk);

        // Then one map at a time, through the read that takes one map apart.
        if self.import_rx.is_none() && !writing {
            if let Some(next) = bulk.queue.pop_front() {
                match metrodreamin::start(&next.id, ctx.clone()) {
                    Ok(import) => self.import_rx = Some(import.receiver),
                    Err(_) => bulk.failed += 1,
                }
            }
        }

        // Nothing left to read and nothing in flight: the import is done.
        if bulk.queue.is_empty() && self.import_rx.is_none() && !writing {
            let failed = bulk.failed;
            self.close_import();
            if failed == 1 {
                self.set_error("1 map could not be imported.");
            } else if failed > 1 {
                self.set_error(format!("{failed} maps could not be imported."));
            }
            return;
        }

        self.bulk = Some(bulk);
    }

    /// Whether a map is on its way into the browser's store.
    #[cfg(target_arch = "wasm32")]
    fn writing(bulk: &Bulk) -> bool {
        bulk.writing.is_some()
    }

    /// The desktop writes a map before the next one is read, so nothing waits.
    #[cfg(not(target_arch = "wasm32"))]
    fn writing(_: &Bulk) -> bool {
        false
    }

    /// Write one map of an account import into the folder, and list it.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_import(&mut self, project: Project) {
        let Some(folder) = self.bulk.as_ref().and_then(|bulk| bulk.folder.clone()) else {
            return;
        };
        let written = serde_json::to_string_pretty(&project)
            .map_err(|err| format!("Could not encode this map: {err}"))
            .and_then(|json| save_into(&folder, &project.name, &json));
        let saved = written.is_ok();
        if let Some(bulk) = self.bulk.as_mut() {
            if saved {
                bulk.saved += 1;
            } else {
                bulk.failed += 1;
            }
        }
        if let Ok(path) = written {
            self.remember_project(path);
        }
    }

    /// Put one map of an account import into the browser's store, under a name
    /// no saved map is using, and list it.
    #[cfg(target_arch = "wasm32")]
    fn write_import(&mut self, mut project: Project, ctx: &egui::Context) {
        let name = free_name(&project.name, &self.web_projects);
        project.name = name.clone();
        let json = match serde_json::to_string_pretty(&project) {
            Ok(json) => json,
            Err(err) => {
                if let Some(bulk) = self.bulk.as_mut() {
                    bulk.failed += 1;
                }
                self.set_error(format!("Could not encode this map: {err}"));
                return;
            }
        };
        if let Some(bulk) = self.bulk.as_mut() {
            bulk.writing = Some(platform::store_save(&name, &json, ctx));
        }
    }

    /// Take an imported map as the document in hand. It has no file of its own
    /// yet, so `Save` asks where it should live.
    fn adopt(&mut self, mut project: Project) {
        project.sanitize();
        self.snapshot();
        self.project = project;
        self.selection = Selection::None;
        self.lines_pick = None;
        self.group_pick = None;
        self.loop_menu = None;
        self.request_fit();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.path = None;
        }
        self.screen = Screen::Editor;
    }

    /// `File` → `Import from Brand New Subway…`: a saved game becomes the map in
    /// hand, named after the file it came from.
    #[cfg(not(target_arch = "wasm32"))]
    fn import_game(&mut self) {
        match platform::pick_game() {
            Ok(Some((path, text))) => {
                let name = path.file_stem().map_or_else(
                    || "Brand New Subway".to_owned(),
                    |stem| stem.to_string_lossy().into_owned(),
                );
                match bns::convert(&name, &text) {
                    Ok(project) => self.adopt(project),
                    Err(err) => self.set_error(err),
                }
            }
            Ok(None) => {}
            Err(err) => self.set_error(err),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn import_game(&mut self) {
        match platform::pick_json_file() {
            Ok(receiver) => self.game_rx = Some(receiver),
            Err(err) => self.set_error(err),
        }
    }

    /// Collect a game the browser's file picker handed over.
    #[cfg(target_arch = "wasm32")]
    fn poll_game(&mut self) {
        let Some(receiver) = &self.game_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(text)) => {
                self.game_rx = None;
                match bns::convert("Brand New Subway", &text) {
                    Ok(project) => self.adopt(project),
                    Err(err) => self.set_error(err),
                }
            }
            Ok(Err(err)) => {
                self.game_rx = None;
                self.set_error(err);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.game_rx = None,
        }
    }

    /// `File` → `Import from MetroDreamin'…`: one link, one button.
    fn import_dialog(&mut self, ctx: &egui::Context) {
        if !self.import_open {
            return;
        }
        let mut open = true;
        let mut submit = false;
        let width = 328.0;
        egui::Area::new(Id::unique("import"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::panel_solid())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .corner_radius(CornerRadius::same(14))
                    .shadow(ui.visuals().window_shadow)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            let title =
                                RichText::new("Import from MetroDreamin'").font(medium(12.5));
                            ui.label(title.color(theme::ink()));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if close_button(ui).clicked() {
                                    open = false;
                                }
                            });
                        });
                        ui.add_space(10.0);
                        let field = ui.add(
                            TextEdit::singleline(&mut self.import_url)
                                .frame(
                                    Frame::new()
                                        .fill(theme::raised())
                                        .corner_radius(CornerRadius::same(7))
                                        .inner_margin(Margin::symmetric(8, 5)),
                                )
                                .hint_text(
                                    RichText::new("Map or profile link")
                                        .font(medium(12.0))
                                        .color(theme::ink_ghost()),
                                )
                                .font(medium(12.0))
                                .text_color(theme::ink())
                                .margin(Margin::symmetric(0, 0))
                                .desired_width(width),
                        );
                        if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                            submit = true;
                        }
                        // An account lands a map at a time: the bar says how far.
                        if let Some((progress, count)) = self.bulk_progress() {
                            ui.add_space(10.0);
                            ui.scope(|ui| {
                                let visuals = ui.visuals_mut();
                                visuals.extreme_bg_color = theme::raised();
                                visuals.selection.bg_fill = theme::accent();
                                visuals.override_text_color = Some(theme::ink());
                                ui.add(
                                    eframe::egui::ProgressBar::new(progress)
                                        .desired_width(width)
                                        .corner_radius(4)
                                        .animate(false)
                                        .text(count),
                                );
                            });
                        }
                        ui.add_space(8.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let busy = self.import_busy();
                            let ready = busy.is_none() && !self.import_url.trim().is_empty();
                            let button = Button::new(
                                RichText::new(busy.unwrap_or("Import")).font(medium(12.0)),
                            );
                            if ui.add_enabled(ready, button).clicked() {
                                submit = true;
                            }
                        });
                    });
            });
        if !open {
            self.close_import();
        } else if submit && self.import_busy().is_none() {
            self.start_import(ctx);
        }
    }
}

impl eframe::App for Metrocraft {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|input| input.time);
        if let Some((_, until)) = &self.error {
            if self.now > *until {
                self.error = None;
            }
        }

        self.poll_load();
        self.poll_import(&ctx);
        #[cfg(target_arch = "wasm32")]
        self.poll_game();
        self.poll_bulk(&ctx);
        #[cfg(target_arch = "wasm32")]
        self.poll_store();
        // A window on Windows carries the menu bar; elsewhere the app draws one.
        let carried = self.window_menu(&ctx, frame);
        // The list stands on its own until a map is open.
        if self.screen == Screen::Projects {
            self.launcher(&ctx);
            self.rename_dialog(&ctx);
            self.import_dialog(&ctx);
            self.error_toast(&ctx);
            return;
        }
        self.handle_keys(&ctx);

        if !carried {
            self.menu_bar(ui);
        }
        self.canvas(ui);
        // The credit belongs to the map, so it sits under anything drawn over it.
        self.attribution(&ctx);

        self.preview = None;
        self.lines_box(&ctx);
        self.node_card(&ctx);
        self.loop_menu(&ctx);
        self.panel(&ctx);
        self.preferences(&ctx);
        self.import_dialog(&ctx);
        self.rename_dialog(&ctx);
        self.stats(&ctx);
        if self.preview != self.preview_shown {
            self.preview_shown = self.preview;
            ctx.request_repaint();
        }
        self.hint(&ctx);
        self.error_toast(&ctx);

        self.tiles.pump(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            STORAGE_KEY,
            &Document {
                project: self.project.clone(),
                view: self.view,
                basemap: self.basemap,
                keys: self.keys,
                theme: self.mode,
            },
        );
        #[cfg(not(target_arch = "wasm32"))]
        eframe::set_value(storage, PROJECTS_KEY, &self.projects);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.screen == Screen::Projects {
            return theme::panel().to_normalized_gamma_f32();
        }
        theme::canvas().to_normalized_gamma_f32()
    }
}

/// Zoom levels one wheel notch is worth.
const ZOOM_PER_NOTCH: f64 = 0.32;
/// A notch arrives as 40 points from a desktop mouse wheel and as about 100
/// in a browser. Dividing by the notch size keeps the two the same speed, and
/// keeping it linear means the step a notch gives does not depend on how many
/// frames the wheel delta is smoothed over.
#[cfg(target_arch = "wasm32")]
const SCROLL_PER_NOTCH: f64 = 100.0;
#[cfg(not(target_arch = "wasm32"))]
const SCROLL_PER_NOTCH: f64 = 40.0;

fn zoom_levels(scroll: f32) -> Option<f64> {
    (scroll.abs() > 0.01).then(|| scroll as f64 / SCROLL_PER_NOTCH * ZOOM_PER_NOTCH)
}

// -------------------------------------------------------------------- widgets

/// One basemap in the Preferences grid: the same tile of the world, with its
/// name under it, outlined while it is the one in use.
fn preview_cell(ui: &mut Ui, basemap: Basemap, current: bool, tiles: &mut TileStore) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(PREVIEW_SIDE, PREVIEW_SIDE + PREVIEW_NAME),
        Sense::click(),
    );
    let thumb = Rect::from_min_size(rect.min, Vec2::splat(PREVIEW_SIDE));
    let painter = ui.painter();
    let id = (basemap, PREVIEW_TILE);
    let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));

    painter.rect_filled(thumb, CornerRadius::same(4), theme::raised());
    if let Some(texture) = tiles.ready(id) {
        painter.image(texture.id(), thumb, whole, Color32::WHITE);
    } else {
        tiles.ensure(id);
    }

    let stroke = if current {
        Stroke::new(1.8, theme::accent())
    } else if response.hovered() {
        Stroke::new(1.0, theme::ink_dim())
    } else {
        Stroke::new(1.0, theme::hairline())
    };
    painter.rect_stroke(thumb, CornerRadius::same(4), stroke, StrokeKind::Outside);

    let name = ellipsize(painter, basemap.name(), &medium(11.0), PREVIEW_SIDE);
    painter.text(
        Pos2::new(thumb.center().x, thumb.max.y + 2.0),
        Align2::CENTER_TOP,
        name,
        medium(11.0),
        if current {
            theme::ink()
        } else {
            theme::ink_dim()
        },
    );

    response.clicked()
}

fn menu_title(label: &str) -> RichText {
    RichText::new(label).font(medium(12.5))
}

/// A menu row with its shortcut set quietly to the right.
fn menu_item(ui: &mut Ui, keys: &Keys, label: &str, action: Action) -> bool {
    let shortcut = RichText::new(keys.label(ui.ctx(), action))
        .font(medium(11.0))
        .color(theme::ink_faint());
    ui.add(Button::new(RichText::new(label).font(medium(12.5))).shortcut_text(shortcut))
        .clicked()
}

/// A menu row for something no key answers to.
fn plain_menu_item(ui: &mut Ui, label: &str) -> bool {
    ui.add(Button::new(RichText::new(label).font(medium(12.5))))
        .clicked()
}

/// One line of text, cut off with an ellipsis rather than wrapped, so that a
/// long path cannot spill out of its row.
fn paint_line(painter: &Painter, pos: Pos2, text: &str, font: FontId, color: Color32, width: f32) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    painter.galley(pos, galley, color);
}

fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("Inter Medium".into()))
}

fn name_edit(text: &mut String, width: f32, size: f32) -> TextEdit<'_> {
    TextEdit::singleline(text)
        .frame(Frame::new())
        .font(medium(size))
        .text_color(theme::ink())
        .margin(Margin::symmetric(0, 2))
        .desired_width(width)
}

fn search_edit(text: &mut String, width: f32) -> TextEdit<'_> {
    TextEdit::singleline(text)
        .frame(
            Frame::new()
                .fill(theme::raised())
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::symmetric(8, 4)),
        )
        .hint_text(
            RichText::new("Search")
                .font(medium(12.0))
                .color(theme::ink_ghost()),
        )
        .font(medium(12.0))
        .text_color(theme::ink())
        .margin(Margin::symmetric(0, 0))
        .desired_width(width)
}

/// One half of the station/waypoint switch.
/// A square button in the node bar, with its hover bed.
fn square_button(ui: &mut Ui) -> (Rect, Response) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CARD_BUTTON), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), theme::raised_hover());
    }
    (rect, response)
}

/// A nearby line, reduced to its colour. Outlined when the node is already on
/// it, so one square both shows the line and says whether it is joined.
fn swatch(ui: &mut Ui, color: [u8; 3], attached: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CARD_BUTTON), Sense::click());
    let painter = ui.painter();
    let fill = Rect::from_center_size(rect.center(), Vec2::splat(15.0));
    painter.rect_filled(fill, CornerRadius::same(1), theme::rgb(color));
    if attached {
        painter.rect_stroke(
            fill.expand(1.5),
            CornerRadius::same(2),
            Stroke::new(1.2, theme::ink()),
            StrokeKind::Outside,
        );
    } else if response.hovered() {
        painter.rect_stroke(
            fill.expand(1.5),
            CornerRadius::same(2),
            Stroke::new(1.0, theme::ink_dim()),
            StrokeKind::Outside,
        );
    }
    response
}

/// A walking interchange, reduced to how it is drawn: white, dark outline.
fn transfer_swatch(ui: &mut Ui, attached: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CARD_BUTTON), Sense::click());
    let painter = ui.painter();
    let fill = Rect::from_center_size(rect.center(), Vec2::splat(15.0));
    painter.rect_filled(fill, CornerRadius::same(1), Color32::WHITE);
    painter.rect_stroke(
        fill,
        CornerRadius::same(1),
        Stroke::new(2.0, theme::station_edge()),
        StrokeKind::Inside,
    );
    if attached {
        painter.rect_stroke(
            fill.expand(1.5),
            CornerRadius::same(2),
            Stroke::new(1.2, theme::ink()),
            StrokeKind::Outside,
        );
    } else if response.hovered() {
        painter.rect_stroke(
            fill.expand(1.5),
            CornerRadius::same(2),
            Stroke::new(1.0, theme::ink_dim()),
            StrokeKind::Outside,
        );
    }
    response
}

/// Two stations a walk apart, as a small mark of the route between them.
fn painter_walk(painter: &Painter, centre: Pos2, color: Color32) {
    let (a, b) = (centre + Vec2::new(-4.2, 0.0), centre + Vec2::new(4.2, 0.0));
    painter.line_segment([a, b], Stroke::new(1.2, color));
    painter.circle_filled(a, 2.2, color);
    painter.circle_filled(b, 2.2, color);
}

/// A station a walk away, how long it takes, and the lines the walk opens up.
/// Made walks carry the same white band they are drawn with on the map.
fn walk_row(ui: &mut Ui, name: &str, km: f32, chips: &[[u8; 3]], made: bool) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(5), theme::raised_hover());
    }
    let centre = Pos2::new(rect.left() + 8.0, rect.center().y);
    if made {
        let band = Rect::from_center_size(centre, Vec2::new(9.0, 9.0));
        painter.rect_filled(band, CornerRadius::same(1), Color32::WHITE);
        painter.rect_stroke(
            band,
            CornerRadius::same(1),
            Stroke::new(1.4, theme::station_edge()),
            StrokeKind::Inside,
        );
    } else {
        painter_walk(painter, centre, theme::ink_faint());
    }

    let minutes = format!("{} min", crate::model::walk_minutes(km));
    let time_font = medium(11.0);
    let time_width = painter
        .layout_no_wrap(minutes.clone(), time_font.clone(), theme::ink_faint())
        .size()
        .x;
    let right = rect.right() - 4.0;
    painter.text(
        Pos2::new(right, rect.center().y),
        Align2::RIGHT_CENTER,
        minutes,
        time_font,
        theme::ink_faint(),
    );
    let mut x = right - time_width - 7.0;
    for color in chips.iter().rev() {
        painter.circle_filled(Pos2::new(x - 3.0, rect.center().y), 3.0, theme::rgb(*color));
        x -= 7.5;
    }

    let font = medium(12.0);
    let start = rect.left() + 18.0;
    let room = (x - start - 4.0).max(24.0);
    painter.text(
        Pos2::new(start, rect.center().y),
        Align2::LEFT_CENTER,
        ellipsize(painter, name, &font, room),
        font,
        theme::ink_dim(),
    );
    response
}

/// Make a loop of the line at this node.
fn loop_button(ui: &mut Ui) -> Response {
    let (rect, response) = square_button(ui);
    let color = if response.hovered() {
        theme::ink()
    } else {
        theme::ink_dim()
    };
    let stroke = Stroke::new(1.2, color);
    let painter = ui.painter();
    let centre = rect.center();
    let radius = 4.4;
    let points: Vec<Pos2> = (0..=20)
        .map(|step| {
            let angle = -std::f32::consts::FRAC_PI_2
                + 0.7
                + step as f32 * ((std::f32::consts::TAU - 1.4) / 20.0);
            centre + Vec2::new(angle.cos() * radius, angle.sin() * radius)
        })
        .collect();
    painter.add(Shape::line(points, stroke));
    // The head sits where the gap is, pointing the way round.
    let tip = centre + Vec2::new(0.0, -radius);
    painter.add(Shape::convex_polygon(
        vec![
            tip + Vec2::new(0.0, -1.0),
            tip + Vec2::new(-3.4, 1.6),
            tip + Vec2::new(2.4, 3.0),
        ],
        color,
        Stroke::NONE,
    ));
    response
}

/// A group heading in the line list; clicking it renames the group.
/// A group's heading: the fold, then its name. Clicking the fold opens or
/// shuts the group; the name picks it to be renamed.
fn group_row(
    ui: &mut Ui,
    id: MapId,
    name: &str,
    active: bool,
    open: bool,
    hidden: bool,
) -> (Response, Response) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
    let fold = ui.interact(
        Rect::from_min_size(rect.min, Vec2::new(16.0, rect.height())),
        ui.scope_id().with(("group-fold", id)),
        Sense::click(),
    );
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, CornerRadius::same(5), theme::raised());
    } else if response.hovered() || fold.hovered() {
        painter.rect_filled(rect, CornerRadius::same(5), theme::raised_hover());
    }

    let centre = Pos2::new(rect.left() + 8.0, rect.center().y);
    let arm = 3.0;
    let points = if open {
        vec![
            centre + Vec2::new(-arm, -arm * 0.6),
            centre + Vec2::new(arm, -arm * 0.6),
            centre + Vec2::new(0.0, arm * 0.9),
        ]
    } else {
        vec![
            centre + Vec2::new(-arm * 0.6, -arm),
            centre + Vec2::new(-arm * 0.6, arm),
            centre + Vec2::new(arm * 0.9, 0.0),
        ]
    };
    painter.add(Shape::convex_polygon(
        points,
        theme::ink_faint(),
        Stroke::NONE,
    ));

    let font = medium(11.0);
    let color = if hidden {
        theme::ink_faint()
    } else if active || response.hovered() {
        theme::ink_dim()
    } else {
        theme::ink_faint()
    };
    painter.text(
        Pos2::new(rect.left() + 17.0, rect.center().y),
        Align2::LEFT_CENTER,
        ellipsize(painter, name, &font, rect.width() - 25.0),
        font,
        color,
    );
    (fold, response)
}

/// Station ⇄ waypoint. The arrow points at what a click makes of this node.
fn arrow_button(ui: &mut Ui, to_waypoint: bool) -> Response {
    let (rect, response) = square_button(ui);
    let color = if response.hovered() {
        theme::ink()
    } else {
        theme::ink_dim()
    };
    let stroke = Stroke::new(1.4, color);
    let centre = rect.center();
    let dir = if to_waypoint { -1.0 } else { 1.0 };
    let tip = centre + Vec2::new(0.0, 5.0 * dir);
    let tail = centre - Vec2::new(0.0, 5.0 * dir);
    let painter = ui.painter();
    painter.line_segment([tail, tip], stroke);
    painter.line_segment([tip, tip + Vec2::new(-3.6, -3.6 * dir)], stroke);
    painter.line_segment([tip, tip + Vec2::new(3.6, -3.6 * dir)], stroke);
    response
}

fn trash_button(ui: &mut Ui) -> Response {
    let (rect, response) = square_button(ui);
    let color = if response.hovered() {
        theme::danger()
    } else {
        theme::ink_dim()
    };
    let stroke = Stroke::new(1.3, color);
    let centre = rect.center();
    let at = |dx: f32, dy: f32| centre + Vec2::new(dx, dy);
    let painter = ui.painter();
    painter.line_segment([at(-4.6, -3.6), at(4.6, -3.6)], stroke);
    painter.line_segment([at(-1.6, -3.6), at(-1.6, -5.2)], stroke);
    painter.line_segment([at(1.6, -3.6), at(1.6, -5.2)], stroke);
    painter.line_segment([at(-1.6, -5.2), at(1.6, -5.2)], stroke);
    painter.line_segment([at(-3.6, -3.6), at(-3.0, 5.0)], stroke);
    painter.line_segment([at(3.6, -3.6), at(3.0, 5.0)], stroke);
    painter.line_segment([at(-3.0, 5.0), at(3.0, 5.0)], stroke);
    painter.line_segment([at(-1.1, -1.6), at(-0.9, 3.4)], stroke);
    painter.line_segment([at(1.1, -1.6), at(0.9, 3.4)], stroke);
    response
}

fn plus_button(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), theme::raised_hover());
    }
    let color = if response.hovered() {
        theme::ink()
    } else {
        theme::ink_dim()
    };
    let stroke = Stroke::new(1.2, color);
    let centre = rect.center();
    painter.line_segment(
        [centre + Vec2::new(-4.5, 0.0), centre + Vec2::new(4.5, 0.0)],
        stroke,
    );
    painter.line_segment(
        [centre + Vec2::new(0.0, -4.5), centre + Vec2::new(0.0, 4.5)],
        stroke,
    );
    response
}

fn chevron(ui: &mut Ui, open: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
    let color = if response.hovered() {
        theme::ink()
    } else {
        theme::ink_faint()
    };
    let centre = rect.center();
    let points = if open {
        vec![
            centre + Vec2::new(-3.8, -1.7),
            centre + Vec2::new(3.8, -1.7),
            centre + Vec2::new(0.0, 2.6),
        ]
    } else {
        vec![
            centre + Vec2::new(-1.7, -3.8),
            centre + Vec2::new(2.6, 0.0),
            centre + Vec2::new(-1.7, 3.8),
        ]
    };
    ui.painter()
        .add(Shape::convex_polygon(points, color, Stroke::NONE));
    response
}

fn close_button(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::click());
    let color = if response.hovered() {
        theme::ink()
    } else {
        theme::ink_faint()
    };
    let centre = rect.center();
    let stroke = Stroke::new(1.1, color);
    painter_cross(ui.painter(), centre, 3.5, stroke);
    response
}

/// The mark that says a name can be changed: a pen.
fn painter_pencil(painter: &Painter, centre: Pos2, stroke: Stroke) {
    // The nib, filled, at the foot of the pen.
    painter.add(Shape::convex_polygon(
        vec![
            centre + Vec2::new(-5.0, 5.0),
            centre + Vec2::new(-4.3, 1.7),
            centre + Vec2::new(-1.7, 4.3),
        ],
        stroke.color,
        Stroke::NONE,
    ));
    // The shaft, outlined so the mark stays light.
    let across = Vec2::new(1.0, 1.0);
    let near = Vec2::new(-3.6, 3.0);
    let far = Vec2::new(3.1, -3.5);
    painter.add(Shape::convex_polygon(
        vec![
            centre + near + across,
            centre + far + across,
            centre + far - across,
            centre + near - across,
        ],
        Color32::TRANSPARENT,
        stroke,
    ));
    // The cap that closes it.
    painter.line_segment([centre + far + across, centre + far - across], stroke);
}

fn painter_cross(painter: &Painter, centre: Pos2, radius: f32, stroke: Stroke) {
    painter.line_segment(
        [
            centre + Vec2::new(-radius, -radius),
            centre + Vec2::splat(radius),
        ],
        stroke,
    );
    painter.line_segment(
        [
            centre + Vec2::new(radius, -radius),
            centre + Vec2::new(-radius, radius),
        ],
        stroke,
    );
}

fn line_row(
    ui: &mut Ui,
    name: &str,
    color: [u8; 3],
    attached: bool,
    active: bool,
    on_foot: bool,
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, CornerRadius::same(6), theme::raised());
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), theme::raised_hover());
    }
    let bar = Rect::from_center_size(
        Pos2::new(rect.left() + 7.0, rect.center().y),
        Vec2::new(4.0, 14.0),
    );
    painter.rect_filled(bar, CornerRadius::same(2), theme::rgb(color));
    let text_color = if active || response.hovered() {
        theme::ink()
    } else {
        theme::ink_dim()
    };
    let start = if on_foot {
        painter_walk(
            painter,
            Pos2::new(rect.left() + 19.0, rect.center().y),
            theme::ink_faint(),
        );
        29.0
    } else {
        18.0
    };
    let font = medium(12.5);
    let room = rect.width() - start - 16.0;
    painter.text(
        Pos2::new(rect.left() + start, rect.center().y),
        Align2::LEFT_CENTER,
        ellipsize(painter, name, &font, room),
        font,
        text_color,
    );
    if attached {
        painter.circle_filled(
            Pos2::new(rect.right() - 8.0, rect.center().y),
            3.0,
            theme::ink(),
        );
    }
    response
}

fn ellipsize(painter: &Painter, text: &str, font: &FontId, width: f32) -> String {
    if painter
        .layout_no_wrap(text.to_owned(), font.clone(), theme::ink())
        .size()
        .x
        <= width
    {
        return text.to_owned();
    }
    let mut clipped: String = text.to_owned();
    while !clipped.is_empty() {
        clipped.pop();
        let candidate = format!("{}…", clipped.trim_end());
        if painter
            .layout_no_wrap(candidate.clone(), font.clone(), theme::ink())
            .size()
            .x
            <= width
        {
            return candidate;
        }
    }
    "…".to_owned()
}

fn hex_edit(text: &mut String) -> TextEdit<'_> {
    TextEdit::singleline(text)
        .frame(
            Frame::new()
                .fill(theme::raised())
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::symmetric(8, 4)),
        )
        .font(medium(12.0))
        .text_color(theme::ink())
        .char_limit(7)
        .desired_width(f32::INFINITY)
}

fn hex_string(color: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

/// `#rrggbb` or `rrggbb`, with or without surrounding space.
fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |range: std::ops::Range<usize>| u8::from_str_radix(&digits[range], 16).ok();
    Some([byte(0..2)?, byte(2..4)?, byte(4..6)?])
}

// ------------------------------------------------------------------ geometry

fn hit_test(project: &Project, view: &MapView, rect: Rect, screen: Pos2) -> Option<Hit> {
    let mut nearest: Option<(f32, Hit)> = None;
    for (id, station) in &project.stations {
        let distance = view.world_to_screen(station.pos, rect).distance(screen);
        if distance <= STATION_HIT && nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, Hit::Station(*id)));
        }
    }
    if let Some((_, hit)) = nearest {
        return Some(hit);
    }

    for (id, waypoint) in &project.waypoints {
        let distance = view.world_to_screen(waypoint.pos, rect).distance(screen);
        if distance <= WAYPOINT_HIT && nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, Hit::Waypoint(*id)));
        }
    }
    if let Some((_, hit)) = nearest {
        return Some(hit);
    }

    // A walking interchange is drawn over the lines, so it is picked first.
    for id in project.transfers.keys() {
        let Some(nodes) = project.transfer_nodes(*id) else {
            continue;
        };
        let points: Vec<Pos2> = nodes
            .iter()
            .filter_map(|node| project.node_pos(*node))
            .map(|pos| view.world_to_screen(pos, rect))
            .collect();
        if points
            .windows(2)
            .any(|pair| point_to_segment(screen, pair[0], pair[1]) <= SEGMENT_HIT)
        {
            return Some(Hit::Transfer(*id));
        }
    }

    for (id, line) in &project.lines {
        if project.line_hidden(*id) {
            continue;
        }
        let Some(points) = node_screen_points(project, view, line, rect) else {
            continue;
        };
        let mut count = points.len().saturating_sub(1);
        if line.closed && points.len() > 2 {
            count = points.len();
        }
        for segment in 0..count {
            let (a, b) = (points[segment], points[(segment + 1) % points.len()]);
            let distance = point_to_segment(screen, a, b);
            if distance <= SEGMENT_HIT && nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, Hit::Line(*id, segment)));
            }
        }
    }
    nearest.map(|(_, hit)| hit)
}

/// A route with the bends a stroke `tolerance` px wide could not show taken
/// out. Both ends stay, and every point kept is further than `tolerance` from
/// the straight run between its neighbours.
fn thin_points(points: &[Pos2], tolerance: f32) -> Vec<Pos2> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let mut kept = Vec::with_capacity(points.len());
    kept.push(first);
    for index in 1..points.len().saturating_sub(1) {
        let previous = *kept.last().expect("the first point is always kept");
        let next = points[index + 1];
        if point_to_segment(points[index], previous, next) > tolerance {
            kept.push(points[index]);
        }
    }
    if points.len() > 1 {
        kept.push(points[points.len() - 1]);
    }
    kept
}

/// Stroke a stretch of route: solid, dashed, dotted or stamped with a shape as
/// the line's own pattern says, or, left to the mode, the way its mode runs -
/// solid for rail, dashes for road, a chain of dots for water and cable, long
/// dashes in the air.
fn paint_run(
    painter: &Painter,
    points: &[Pos2],
    stroke: Stroke,
    mode: Mode,
    pattern: Pattern,
    width: f32,
) {
    let scale = width.clamp(1.0, 12.0);
    match pattern {
        Pattern::Solid => {
            painter.add(Shape::line(points.to_vec(), stroke));
        }
        Pattern::Dashed => dash(painter, points, stroke, scale * 3.0, scale * 2.0),
        Pattern::Dotted => dots(painter, points, stroke, scale * 1.8, 0.34),
        Pattern::Circle | Pattern::Diamond | Pattern::Plus | Pattern::Star | Pattern::Heart => {
            stamp(painter, points, stroke, pattern, scale);
        }
        Pattern::Auto => match mode {
            Mode::Bus => dash(painter, points, stroke, scale * 1.6, scale * 1.1),
            Mode::Brt => dash(painter, points, stroke, scale * 3.0, scale * 1.5),
            Mode::Air => dash(painter, points, stroke, scale * 5.0, scale * 2.6),
            Mode::Ferry => dots(painter, points, stroke, scale * 2.4, 0.46),
            Mode::Gondola => dots(painter, points, stroke, scale * 1.7, 0.34),
            _ => {
                painter.add(Shape::line(points.to_vec(), stroke));
            }
        },
    }
}

fn dash(painter: &Painter, points: &[Pos2], stroke: Stroke, dash: f32, gap: f32) {
    for shape in Shape::dashed_line(points, stroke, dash, gap) {
        painter.add(shape);
    }
}

fn dots(painter: &Painter, points: &[Pos2], stroke: Stroke, spacing: f32, radius: f32) {
    let radius = (stroke.width * radius).max(0.5);
    for shape in Shape::dotted_line(points, stroke.color, spacing, radius) {
        painter.add(shape);
    }
}

/// Stamp a pattern along a stretch: a chain of shapes in the stroke's colour,
/// spaced by the line's own width. Both passes of a line - the casing beneath
/// it and its colour - come through here, so their shapes sit on each other.
fn stamp(painter: &Painter, points: &[Pos2], stroke: Stroke, pattern: Pattern, scale: f32) {
    let radius = stroke.width * 0.62;
    for centre in spaced_points(points, scale * 2.6) {
        match pattern {
            Pattern::Circle => {
                painter.circle_filled(centre, radius, stroke.color);
            }
            Pattern::Diamond => {
                painter.add(Shape::convex_polygon(
                    diamond_points(centre, radius),
                    stroke.color,
                    Stroke::NONE,
                ));
            }
            Pattern::Plus => paint_fan(painter, centre, &plus_points(centre, radius), stroke.color),
            Pattern::Star => paint_fan(painter, centre, &star_points(centre, radius), stroke.color),
            _ => paint_fan(painter, centre, &heart_points(centre, radius), stroke.color),
        }
    }
}

/// Points every `spacing` along a route, starting half a step in so the chain
/// begins inside the route rather than on its very end.
fn spaced_points(points: &[Pos2], spacing: f32) -> Vec<Pos2> {
    let mut spaced = Vec::new();
    if spacing <= f32::EPSILON {
        return spaced;
    }
    let mut walked = 0.0;
    let mut next = spacing * 0.5;
    for pair in points.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let step = to - from;
        let length = step.length();
        if length <= f32::EPSILON {
            continue;
        }
        while next <= walked + length {
            spaced.push(from + step * ((next - walked) / length));
            next += spacing;
        }
        walked += length;
    }
    spaced
}

/// Fill a shape that is star-shaped about `centre` - a plus, a star, a heart -
/// as a fan of triangles, since egui only fills a polygon in one piece while
/// it stays convex.
fn paint_fan(painter: &Painter, centre: Pos2, outline: &[Pos2], color: Color32) {
    for (index, point) in outline.iter().enumerate() {
        painter.add(Shape::convex_polygon(
            vec![centre, *point, outline[(index + 1) % outline.len()]],
            color,
            Stroke::NONE,
        ));
    }
}

fn plus_points(centre: Pos2, radius: f32) -> Vec<Pos2> {
    let arm = radius * 0.42;
    vec![
        centre + Vec2::new(-arm, -radius),
        centre + Vec2::new(arm, -radius),
        centre + Vec2::new(arm, -arm),
        centre + Vec2::new(radius, -arm),
        centre + Vec2::new(radius, arm),
        centre + Vec2::new(arm, arm),
        centre + Vec2::new(arm, radius),
        centre + Vec2::new(-arm, radius),
        centre + Vec2::new(-arm, arm),
        centre + Vec2::new(-radius, arm),
        centre + Vec2::new(-radius, -arm),
        centre + Vec2::new(-arm, -arm),
    ]
}

fn star_points(centre: Pos2, radius: f32) -> Vec<Pos2> {
    let mut points = Vec::with_capacity(10);
    let step = std::f32::consts::PI / 5.0;
    for index in 0..10 {
        let angle = -std::f32::consts::FRAC_PI_2 + index as f32 * step;
        let reach = if index % 2 == 0 {
            radius
        } else {
            radius * 0.46
        };
        let (sin, cos) = angle.sin_cos();
        points.push(centre + Vec2::new(cos * reach, sin * reach));
    }
    points
}

fn heart_points(centre: Pos2, radius: f32) -> Vec<Pos2> {
    // The heart curve, sampled evenly and scaled to `radius` across, with its
    // point hanging down the way a heart is drawn.
    const STEPS: usize = 14;
    let mut points = Vec::with_capacity(STEPS);
    for index in 0..STEPS {
        let t = index as f32 / STEPS as f32 * std::f32::consts::TAU;
        let (sin, cos) = t.sin_cos();
        let x = 16.0 * sin * sin * sin;
        let y = 13.0 * cos - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
        points.push(centre + Vec2::new(x, -y - 6.0) * (radius / 16.0));
    }
    points
}

/// The stretches of a route that cross `rect`, each with the point beyond
/// either end, so a stroke enters and leaves the viewport where it should. A
/// route that wanders off screen and back comes out as several stretches.
fn visible_runs(points: &[Pos2], rect: Rect) -> Vec<std::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for index in 0..points.len().saturating_sub(1) {
        let segment = Rect::from_two_pos(points[index], points[index + 1]);
        match (segment.intersects(rect), start) {
            (true, None) => start = Some(index),
            (false, Some(from)) => {
                runs.push(from..index + 1);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        runs.push(from..points.len());
    }
    runs
}

fn node_screen_points(
    project: &Project,
    view: &MapView,
    line: &Line,
    rect: Rect,
) -> Option<Vec<Pos2>> {
    let points: Vec<Pos2> = line
        .nodes
        .iter()
        .filter_map(|node| project.node_pos(*node))
        .map(|pos| view.world_to_screen(pos, rect))
        .collect();
    (points.len() >= 2).then_some(points)
}

/// A rounded square turned onto its diagonal: the terminus mark. Points run
/// clockwise from the first corner's arc.
/// How a station is drawn: a rhombus where lines meet, a turned square at the
/// end of a line, a dot otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StationMark {
    Interchange,
    Terminus,
    Stop,
}

fn station_mark(interchange: bool, terminus: bool) -> StationMark {
    if interchange {
        // A crossroads is the stronger mark: lines pass through it.
        StationMark::Interchange
    } else if terminus {
        StationMark::Terminus
    } else {
        StationMark::Stop
    }
}

/// One stop in the line panel's route: the station, the lines to change to,
/// whether the line ends there, and how high the station sits.
struct StopRow {
    station: MapId,
    name: String,
    others: Vec<[u8; 3]>,
    terminal: bool,
    grade: Elevation,
}

/// A station's mark at its grade, small, beside the grade control.
fn station_grade_glyph(ui: &mut Ui, elevation: Elevation, mark: StationMark) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(15.0, 15.0), Sense::hover());
    let ink = station_ink(elevation);
    let radius = if mark == StationMark::Interchange {
        4.2
    } else {
        3.2
    } * ink.scale;
    paint_station_mark(ui.painter(), rect.center(), radius, mark, ink);
}

/// Whether a group's lines show on the map: an eye, struck through once they do
/// not.
fn eye_button(ui: &mut Ui, hidden: bool) -> Response {
    let (rect, response) = square_button(ui);
    let painter = ui.painter();
    let color = if hidden {
        theme::ink_faint()
    } else if response.hovered() {
        theme::ink()
    } else {
        theme::ink_dim()
    };
    let centre = rect.center();
    let (w, h) = (5.4, 3.2);
    let steps = 12;
    let mut upper = Vec::with_capacity(steps + 1);
    let mut lower = Vec::with_capacity(steps + 1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = -w + 2.0 * w * t;
        let y = h * (std::f32::consts::PI * t).sin();
        upper.push(centre + Vec2::new(x, -y));
        lower.push(centre + Vec2::new(x, y));
    }
    let stroke = Stroke::new(1.2, color);
    painter.add(Shape::line(upper, stroke));
    painter.add(Shape::line(lower, stroke));
    painter.circle_filled(centre, 1.7, color);
    if hidden {
        // A line through it, so a hidden group reads at a glance.
        painter.line_segment(
            [
                centre + Vec2::new(-w - 0.8, h + 0.8),
                centre + Vec2::new(w + 0.8, -h - 0.8),
            ],
            Stroke::new(1.2, color),
        );
    }
    response
}

/// A hairline across a panel, between one section and the next.
fn panel_divider(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 7.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(1.0, theme::hairline()),
    );
}

/// How a station's mark is inked: a station below grade is solid, one above
/// grade carries a heavier rim, and one at grade is the plain white mark.
#[derive(Clone, Copy)]
struct StationInk {
    fill: Color32,
    edge: Color32,
    weight: f32,
    scale: f32,
}

fn station_ink(elevation: Elevation) -> StationInk {
    match elevation {
        Elevation::Above => StationInk {
            fill: theme::station(),
            edge: theme::station_edge(),
            weight: 2.8,
            scale: 1.1,
        },
        Elevation::At => StationInk {
            fill: theme::station(),
            edge: theme::station_edge(),
            weight: 1.8,
            scale: 1.0,
        },
        Elevation::Below => StationInk {
            fill: theme::ink(),
            edge: theme::station(),
            weight: 1.4,
            scale: 0.92,
        },
    }
}

/// The mark itself, at `radius` from `centre`, in the grade's ink. Shared by
/// the map and the panel, so both show a station the same way.
fn paint_station_mark(
    painter: &Painter,
    centre: Pos2,
    radius: f32,
    mark: StationMark,
    ink: StationInk,
) {
    let edge = Stroke::new(
        ink.weight
            + if mark == StationMark::Interchange {
                0.2
            } else {
                0.0
            },
        ink.edge,
    );
    match mark {
        StationMark::Interchange => {
            painter.add(Shape::convex_polygon(
                diamond_points(centre, radius * 1.34),
                ink.fill,
                edge,
            ));
        }
        // A line's ends are drawn as a rounded square on the diagonal, so a
        // terminus never reads as a stop passed through.
        StationMark::Terminus => {
            painter.add(Shape::convex_polygon(
                rounded_square_points(centre, radius * 1.05, radius * 0.42),
                ink.fill,
                edge,
            ));
        }
        StationMark::Stop => {
            painter.circle(centre, radius, ink.fill, edge);
        }
    }
}

/// A rhombus: four sharp corners, one on each axis of `centre`.
fn diamond_points(centre: Pos2, corner: f32) -> Vec<Pos2> {
    vec![
        centre + Vec2::new(0.0, -corner),
        centre + Vec2::new(corner, 0.0),
        centre + Vec2::new(0.0, corner),
        centre + Vec2::new(-corner, 0.0),
    ]
}

fn rounded_square_points(centre: Pos2, half: f32, corner: f32) -> Vec<Pos2> {
    const STEPS: usize = 4;
    let corner = corner.clamp(0.5, half);
    // The corner arcs sit in from each corner of the square, so their centres
    // lie a diagonal's worth out from the middle.
    let inner = (half - corner).max(0.0) * std::f32::consts::SQRT_2;
    let mut points = Vec::with_capacity(4 * (STEPS + 1));
    let quarter = std::f32::consts::FRAC_PI_2;
    for side in 0..4 {
        let around = std::f32::consts::FRAC_PI_4 + side as f32 * quarter;
        let (sin, cos) = around.sin_cos();
        let (cx, cy) = (inner * cos, inner * sin);
        for step in 0..=STEPS {
            let angle = around - quarter * 0.5 + step as f32 * quarter / STEPS as f32;
            let (sin, cos) = angle.sin_cos();
            let (x, y) = (cx + corner * cos, cy + corner * sin);
            // Turn the whole shape by 45°, then put it on the node.
            let turned = std::f32::consts::FRAC_PI_4.sin_cos();
            points.push(Pos2::new(
                centre.x + x * turned.1 - y * turned.0,
                centre.y + x * turned.0 + y * turned.1,
            ));
        }
    }
    points
}

fn point_to_segment(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let length_squared = ab.length_sq();
    if length_squared <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).dot(ab) / length_squared).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}

fn parent_uv(key: TileKey, parent: TileKey) -> Rect {
    let levels = key.z.saturating_sub(parent.z).max(1) as u32;
    let range = 1i64 << levels;
    let scale = 1.0 / range as f32;
    let local_x = key.x.rem_euclid(range) as f32;
    let local_y = key.y.rem_euclid(range) as f32;
    Rect::from_min_max(
        Pos2::new(local_x * scale, local_y * scale),
        Pos2::new((local_x + 1.0) * scale, (local_y + 1.0) * scale),
    )
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// A length in kilometres, to a precision that suits how far it runs.
fn kilometres(km: f32) -> String {
    // An empty sum is a negative zero, which has no place on a map.
    let km = if km.is_finite() { km.max(0.0) } else { 0.0 };
    if km >= 100.0 {
        format!("{km:.0} km")
    } else if km >= 10.0 {
        format!("{km:.1} km")
    } else {
        format!("{km:.2} km")
    }
}

/// Whether what was pasted names an account rather than a single map.
fn account_link(text: &str) -> bool {
    text.contains("/user/")
        || (metrodreamin::system_id(text).is_err() && metrodreamin::user_id(text).is_some())
}

/// Write an imported map into a folder, beside any map already there.
#[cfg(not(target_arch = "wasm32"))]
fn save_into(
    folder: &std::path::Path,
    name: &str,
    json: &str,
) -> Result<std::path::PathBuf, String> {
    let stem = file_stem(name);
    let mut path = folder.join(format!("{stem}.json"));
    let mut copy = 2;
    while path.exists() {
        path = folder.join(format!("{stem} ({copy}).json"));
        copy += 1;
    }
    platform::write(&path, json)?;
    Ok(path)
}

/// A name no saved map is using, by numbering it until one is free.
#[cfg(any(target_arch = "wasm32", test))]
fn free_name(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|name| name == base) {
        return base.to_owned();
    }
    let mut copy = 2;
    loop {
        let candidate = format!("{base} {copy}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        copy += 1;
    }
}

/// A name a map can be kept under: trimmed, and not a path in itself.
fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return None;
    }
    let capped: String = name.chars().take(120).collect();
    Some(capped.trim_end().to_owned())
}

/// A map's name as the list shows it: the file it lives in, less the extension.
#[cfg(not(target_arch = "wasm32"))]
fn map_name(path: &std::path::Path) -> String {
    path.file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
}

/// A name as a file system will take it, otherwise kept as it was typed.
#[cfg(not(target_arch = "wasm32"))]
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c| c == ' ' || c == '.');
    if cleaned.is_empty() {
        "map".to_owned()
    } else {
        cleaned.to_owned()
    }
}

/// Move a map's file to the name it now has, and answer with where it went.
#[cfg(not(target_arch = "wasm32"))]
fn move_map_file(path: &std::path::Path, name: &str) -> Result<std::path::PathBuf, String> {
    if !path.exists() {
        return Err("That map's file is no longer there.".to_owned());
    }
    let target = path.with_file_name(format!("{}.json", safe_file_name(name)));
    if target == path {
        return Ok(target);
    }
    // A file already at that name is another map, unless it is this one under
    // another case.
    if target.exists() && target.canonicalize().ok() != path.canonicalize().ok() {
        return Err("A map with that name is already there.".to_owned());
    }
    std::fs::rename(path, &target).map_err(|err| format!("Could not rename that map: {err}"))?;
    Ok(target)
}

fn file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_ascii_lowercase();
    if cleaned.is_empty() {
        "metrocraft-map".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
impl Metrocraft {
    fn editing(project: Project) -> Self {
        Self {
            project,
            view: MapView::default(),
            selection: Selection::None,
            place: Place::Station,
            hover: None,
            hover_pos: None,
            preview: None,
            preview_shown: None,
            drag: None,
            drag_moved: false,
            tiles: TileStore::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            error: None,
            now: 0.0,
            canvas: Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0)),
            pending_fit: false,
            line_query: String::new(),
            hex_line: None,
            hex_text: String::new(),
            show_waypoints: true,
            stats_open: false,
            basemap: Basemap::default(),
            mode: theme::Mode::Dark,
            keys: Keys::default(),
            listening: None,
            preferences: false,
            import_open: false,
            import_url: String::new(),
            import_rx: None,
            bulk: None,
            prefs_tab: 0,
            lines_open: false,
            lines_pick: None,
            group_pick: None,
            loop_menu: None,
            focus_name: None,
            rename: None,
            rename_text: String::new(),
            rename_focus: false,
            path: None,
            projects: Vec::new(),
            #[cfg(windows)]
            menu: None,
            screen: Screen::Editor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))
    }

    #[test]
    fn clicking_bare_ground_places_a_station_then_a_waypoint() {
        let mut app = Metrocraft::editing(Project::new("test"));
        let rect = app.canvas;
        let spot = Pos2::new(320.0, 300.0);

        app.on_click(spot, rect);
        assert_eq!(app.project.stations.len(), 1);
        assert!(matches!(app.selection, Selection::Station(_)));
        assert_eq!(app.place, Place::Station);

        // The switch turns the node in hand into a waypoint and keeps going.
        app.set_place(Place::Waypoint);
        assert!(app.project.stations.is_empty());
        assert_eq!(app.project.waypoints.len(), 1);
        assert!(matches!(app.selection, Selection::Waypoint(_)));

        app.on_click(spot + Vec2::new(70.0, 40.0), rect);
        assert_eq!(app.project.waypoints.len(), 2);
        assert!(app.project.stations.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_map_in_the_list_opens_from_its_file() {
        let path = std::env::temp_dir().join("metrocraft-launcher-test.json");
        let mut on_disk = Project::new("from-disk");
        on_disk.add_named_station("A", GeoPos::new(-0.1, 51.5));
        std::fs::write(&path, serde_json::to_string(&on_disk).unwrap()).unwrap();

        let mut app = Metrocraft::editing(Project::new("other"));
        app.screen = Screen::Projects;
        app.projects = vec![path.clone()];
        app.open_project(0);
        assert_eq!(app.project.stations.len(), 1);
        assert_eq!(app.project.name, "from-disk");
        assert_eq!(app.screen, Screen::Editor);
        assert_eq!(app.path.as_ref(), Some(&path));
        assert!(app.pending_fit);
        assert!(app.error.is_none());
        let _ = std::fs::remove_file(&path);

        // A row whose file has gone says so, and stays on the list.
        let missing = std::env::temp_dir().join("metrocraft-not-here-4f2a.json");
        app.screen = Screen::Projects;
        app.projects = vec![missing];
        app.open_project(0);
        assert_eq!(app.screen, Screen::Projects);
        assert!(app.error.is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn opening_a_map_keeps_the_list_newest_first() {
        let mut app = Metrocraft::editing(Project::new("test"));
        app.enter_project(std::path::PathBuf::from("/maps/one.json"));
        app.enter_project(std::path::PathBuf::from("/maps/two.json"));
        app.enter_project(std::path::PathBuf::from("/maps/one.json"));
        assert_eq!(
            app.projects,
            vec![
                std::path::PathBuf::from("/maps/one.json"),
                std::path::PathBuf::from("/maps/two.json"),
            ]
        );
        assert_eq!(app.path, Some(std::path::PathBuf::from("/maps/one.json")));
        assert_eq!(app.screen, Screen::Editor);

        for index in 0..PROJECTS_MAX + 3 {
            app.enter_project(std::path::PathBuf::from(format!("/maps/{index}.json")));
        }
        assert_eq!(app.projects.len(), PROJECTS_MAX);
    }

    #[test]
    fn a_new_line_carries_the_selected_node() {
        let mut app = Metrocraft::editing(Project::new("test"));
        let rect = app.canvas;
        app.on_click(Pos2::new(320.0, 300.0), rect);
        app.create_line();

        let line_id = *app.project.lines.keys().next().unwrap();
        assert_eq!(app.project.lines.get(&line_id).unwrap().nodes.len(), 1);
        // The node stays in hand, and the box opens on the line just made.
        assert!(matches!(app.selection, Selection::Station(_)));
        assert_eq!(app.lines_pick, Some(line_id));
        assert!(app.lines_open);
    }

    fn scene() -> (Project, MapView) {
        let mut project = Project::new("test");
        let a = project.add_named_station("A", GeoPos::new(-0.1, 51.5));
        let b = project.add_named_station("B", GeoPos::new(-0.05, 51.52));
        let line = project.add_line("Line", [235, 87, 87]);
        project.lines.get_mut(&line).unwrap().nodes = vec![Node::Station(a), Node::Station(b)];
        let view = MapView {
            lon: -0.075,
            lat: 51.51,
            zoom: 14.0,
        };
        (project, view)
    }

    #[test]
    fn a_wheel_notch_is_worth_a_third_of_a_zoom_level() {
        // One notch, whichever number of points this platform calls a notch.
        assert!((zoom_levels(SCROLL_PER_NOTCH as f32).unwrap() - 0.32).abs() < 1e-6);
        assert!((zoom_levels(-SCROLL_PER_NOTCH as f32).unwrap() + 0.32).abs() < 1e-6);
        // Linear, so a notch split over several frames still adds up to one
        // notch, and three notches are three times one.
        let steps: f64 = [0.4, 0.3, 0.3]
            .into_iter()
            .map(|part| zoom_levels(SCROLL_PER_NOTCH as f32 * part).unwrap())
            .sum();
        assert!((steps - 0.32).abs() < 1e-6, "{steps}");
        assert!(zoom_levels(0.0).is_none());
    }

    #[test]
    fn hex_colours_round_trip_and_reject_junk() {
        assert_eq!(hex_string([0x2a, 0xa9, 0xe0]), "#2AA9E0");
        assert_eq!(parse_hex("#2AA9E0"), Some([0x2a, 0xa9, 0xe0]));
        assert_eq!(parse_hex("2aa9e0"), Some([0x2a, 0xa9, 0xe0]));
        assert_eq!(parse_hex("  #2AA9E0 "), Some([0x2a, 0xa9, 0xe0]));
        assert_eq!(parse_hex("#2AA9E"), None);
        assert_eq!(parse_hex("#GGGGGG"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn clicking_a_station_hits_it() {
        let (project, view) = scene();
        let station = project.stations.values().next().unwrap();
        let screen = view.world_to_screen(station.pos, rect());
        assert_eq!(
            hit_test(&project, &view, rect(), screen),
            Some(Hit::Station(
                project.stations.keys().next().copied().unwrap()
            ))
        );
        let empty = rect().left_top() + Vec2::new(24.0, 24.0);
        assert!(hit_test(&project, &view, rect(), empty).is_none());
    }

    #[test]
    fn clicking_a_line_hits_the_right_segment() {
        let (project, view) = scene();
        let ids: Vec<MapId> = project.stations.keys().copied().collect();
        let a = view.world_to_screen(project.stations[&ids[0]].pos, rect());
        let b = view.world_to_screen(project.stations[&ids[1]].pos, rect());
        let middle = a + (b - a) * 0.5;
        let line_id = *project.lines.keys().next().unwrap();
        assert_eq!(
            hit_test(&project, &view, rect(), middle),
            Some(Hit::Line(line_id, 0))
        );
    }

    #[test]
    fn point_to_segment_clamps_at_the_ends() {
        let a = Pos2::new(0.0, 0.0);
        let b = Pos2::new(10.0, 0.0);
        assert!((point_to_segment(Pos2::new(5.0, 4.0), a, b) - 4.0).abs() < 1.0e-4);
        assert!((point_to_segment(Pos2::new(-3.0, 0.0), a, b) - 3.0).abs() < 1.0e-4);
        assert!((point_to_segment(Pos2::new(17.0, 0.0), a, b) - 7.0).abs() < 1.0e-4);
        assert!((point_to_segment(Pos2::new(3.0, 5.0), a, a) - 5.830_952).abs() < 1.0e-3);
    }

    #[test]
    fn parent_tiles_map_to_the_right_uv_quadrant() {
        let child = TileKey::new(5, 9, 12);
        let parent = child.parent().unwrap();
        let uv = parent_uv(child, parent);
        assert!((uv.min.x - 0.5).abs() < 1.0e-6);
        assert!((uv.min.y - 0.0).abs() < 1.0e-6);
        assert!((uv.max.x - 1.0).abs() < 1.0e-6);
        assert!((uv.max.y - 0.5).abs() < 1.0e-6);

        let grandparent = parent.parent().unwrap();
        let uv = parent_uv(child, grandparent);
        assert!((uv.min.x - 0.25).abs() < 1.0e-6);
        assert!((uv.min.y - 0.0).abs() < 1.0e-6);
        assert!((uv.max.x - 0.5).abs() < 1.0e-6);
        assert!((uv.max.y - 0.25).abs() < 1.0e-6);
    }

    #[test]
    fn a_walk_is_offered_only_where_it_opens_up_a_line() {
        let mut project = Project::new("test");
        let here = project.add_named_station("A", GeoPos::new(0.0, 51.5));
        let near = project.add_named_station("B", GeoPos::new(0.004, 51.5));
        let same = project.add_named_station("C", GeoPos::new(0.0, 51.504));
        let couple = project.add_named_station("D", GeoPos::new(0.02, 51.5));
        let one = project.add_line("One", [220, 60, 60]);
        let two = project.add_line("Two", [60, 120, 220]);
        project.attach(one, Node::Station(here));
        project.attach(one, Node::Station(same));
        project.attach(one, Node::Station(couple));
        project.attach(two, Node::Station(near));
        let mut app = Metrocraft::editing(project);
        app.selection = Selection::Station(here);

        // Both neighbours are within a walk; only one of them gets us anywhere.
        let mut offered: Vec<MapId> = app
            .project
            .stations_within_walk(here)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        offered.sort_unstable();
        let mut both = vec![near, same];
        both.sort_unstable();
        assert_eq!(offered, both);
        assert_eq!(app.project.lines_gained(here, near), vec![two]);
        assert!(app.project.lines_gained(here, same).is_empty());

        app.walk_to(here, near);
        let walk = app.project.transfer_between(here, near).unwrap();
        // The station stays picked, so another walk can follow straight away.
        assert_eq!(app.selection, Selection::Station(here));
        assert!(app
            .project
            .stations_within_walk(here)
            .iter()
            .all(|(id, _)| *id != near));
        // Its line is now reachable on foot, and listed as such.
        assert!(app
            .project
            .walked_lines(here)
            .iter()
            .any(|(line, via)| *line == two && *via == walk));
        assert!(app
            .walk_chips(here, near)
            .contains(&app.project.lines.get(&two).map(|line| line.color).unwrap()));

        // And it undoes in one step.
        app.undo();
        assert!(app.project.transfers.is_empty());

        // A station with nobody within a walk is offered nothing.
        let mut project = Project::new("test");
        let alone = project.add_station(GeoPos::new(0.0, 0.0));
        let app = Metrocraft::editing(project);
        assert!(app.project.stations_within_walk(alone).is_empty());
        assert!(app.nearby_transfer(alone, GeoPos::new(0.0, 0.0)).is_none());
    }

    #[test]
    fn a_waypoint_offered_a_walk_hangs_off_it() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(0.0, 51.5));
        let b = project.add_station(GeoPos::new(0.006, 51.5));
        let walk = project.add_transfer(a, b).unwrap();
        let aside = project.add_waypoint(GeoPos::new(0.003, 51.501));
        let app = Metrocraft::editing(project.clone());

        assert_eq!(
            app.nearby_transfer(aside, GeoPos::new(0.003, 51.501)),
            Some((walk, false))
        );

        let mut app = Metrocraft::editing(project);
        app.project.attach_to_transfer(walk, aside);
        let pos = app.project.waypoints[&aside].pos;
        assert_eq!(app.nearby_transfer(aside, pos), Some((walk, true)));
    }

    #[test]
    fn the_loop_menu_offers_the_lines_a_node_can_loop() {
        let mut project = Project::new("test");
        let ids: Vec<MapId> = (0..4)
            .map(|step| project.add_station(GeoPos::new(step as f32 * 0.01, 0.0)))
            .collect();
        let line = project.add_line("Line", [1, 2, 3]);
        project.lines.get_mut(&line).unwrap().nodes =
            ids.iter().map(|id| Node::Station(*id)).collect();
        let app = Metrocraft::editing(project);

        assert_eq!(app.loopable_lines(Node::Station(ids[3])), vec![line]);
        assert_eq!(app.loopable_lines(Node::Station(ids[0])), vec![line]);
        // One stop in, there is no route behind the node to close.
        assert!(app.loopable_lines(Node::Station(ids[1])).is_empty());
    }

    #[test]
    fn a_crossroads_is_a_rhombus_and_takes_the_mark_over_an_end() {
        assert_eq!(station_mark(true, false), StationMark::Interchange);
        assert_eq!(station_mark(true, true), StationMark::Interchange);
        assert_eq!(station_mark(false, true), StationMark::Terminus);
        assert_eq!(station_mark(false, false), StationMark::Stop);

        let centre = Pos2::new(4.0, 9.0);
        let points = diamond_points(centre, 6.0);
        assert_eq!(points.len(), 4);
        assert_eq!(points[0], Pos2::new(4.0, 3.0));
        assert_eq!(points[1], Pos2::new(10.0, 9.0));
        assert_eq!(points[2], Pos2::new(4.0, 15.0));
        assert_eq!(points[3], Pos2::new(-2.0, 9.0));
    }

    #[test]
    fn a_terminus_mark_is_a_square_turned_onto_its_diagonal() {
        let points = rounded_square_points(Pos2::new(0.0, 0.0), 5.0, 2.0);
        assert_eq!(points.len(), 20);
        // Turned onto its diagonal, a corner sits on each axis, a full
        // circumradius out, and no point strays further than that.
        let circumradius = (5.0 - 2.0) * std::f32::consts::SQRT_2 + 2.0;
        assert!(points
            .iter()
            .all(|p| p.to_vec2().length() <= circumradius + 0.01));
        assert!(
            points
                .iter()
                .any(|p| p.x.abs() < 0.01 && (p.y - circumradius).abs() < 0.05),
            "a corner is not on the axis: {points:?}"
        );
        // Every vertex is out on a corner arc, never inside the square's sides.
        assert!(points.iter().all(|p| p.to_vec2().length() > 5.5));
    }

    #[test]
    fn stats_are_worded_and_measured_to_the_size_of_the_network() {
        assert_eq!(plural(0, "station"), "0 stations");
        assert_eq!(plural(1, "station"), "1 station");
        assert_eq!(plural(3, "line"), "3 lines");
        assert_eq!(kilometres(0.0), "0.00 km");
        assert_eq!(kilometres(2.5), "2.50 km");
        assert_eq!(kilometres(42.55), "42.5 km");
        assert_eq!(kilometres(1234.0), "1234 km");
        assert_eq!(kilometres(f32::NAN), "0.00 km");
        // An empty sum arrives as a negative zero.
        assert_eq!(kilometres(-0.0), "0.00 km");
    }

    /// A map at the size MetroDreamin' will nearly take, for sizing the paint
    /// path: routes that wander a metro's worth of ground the way a line along
    /// streets does, calling at a station every few bends.
    fn huge_project(stations: usize, waypoints: usize, lines: usize) -> Project {
        let mut project = Project::new("huge");
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32) / ((1u64 << 31) as f32)
        };
        let total = stations + waypoints;
        let (mut placed_s, mut placed_w) = (0, 0);
        for index in 0..lines {
            let id = project.add_line(format!("Line {}", index + 1), [200, 80, 60]);
            let mut nodes = Vec::new();
            let mut lon = -118.5 + next() * 0.44;
            let mut lat = 33.92 + next() * 0.36;
            let mut heading = next() * std::f32::consts::TAU;
            for step in 0..total / lines.max(1) {
                heading += (next() - 0.5) * 0.15;
                lon += heading.cos() * 0.0016;
                lat += heading.sin() * 0.0013;
                if !(-118.5..=-118.0).contains(&lon) || !(33.9..=34.3).contains(&lat) {
                    heading += std::f32::consts::PI;
                    lon = lon.clamp(-118.5, -118.0);
                    lat = lat.clamp(33.9, 34.3);
                }
                let pos = GeoPos::new(lon, lat);
                let stop = step % 4 == 0 && placed_s < stations;
                if stop {
                    placed_s += 1;
                    nodes.push(Node::Station(project.add_station(pos)));
                } else if placed_w < waypoints {
                    placed_w += 1;
                    nodes.push(Node::Waypoint(project.add_waypoint(pos)));
                } else {
                    nodes.push(Node::Station(project.add_station(pos)));
                }
            }
            if let Some(line) = project.lines.get_mut(&id) {
                line.nodes = nodes;
            }
        }
        project
    }

    /// One frame of the canvas's vector work, in milliseconds and shapes.
    fn frame_cost(app: &Metrocraft) -> (f64, usize) {
        let ctx = egui::Context::default();
        let run = |ctx: &egui::Context| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let spacing = app.node_spacing(app.canvas);
            app.paint_lines(&painter, app.canvas);
            app.paint_transfers(&painter, app.canvas);
            app.paint_labels(&painter, app.canvas, spacing);
            app.paint_nodes(&painter, app.canvas, spacing);
        };
        // Warm: the first frame builds the font atlas.
        let mut first = ctx.run_ui(egui::RawInput::default(), |ctx| run(ctx));
        first.textures_delta.clear();
        let start = std::time::Instant::now();
        let frames = 10;
        let mut shapes = 0;
        let mut meshed = 0;
        for _ in 0..frames {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ctx| run(ctx));
            shapes = out.shapes.len();
            out.textures_delta.clear();
            let meshes = ctx.tessellate(std::mem::take(&mut out.shapes), 1.0);
            meshed = meshes.len();
        }
        let _ = meshed;
        (
            start.elapsed().as_secs_f64() * 1000.0 / frames as f64,
            shapes,
        )
    }

    #[test]
    #[ignore = "sizing, not correctness"]
    fn a_huge_map_stays_inside_a_frame() {
        let project = huge_project(12_000, 38_000, 40);
        let mut app = Metrocraft::editing(project);
        let rect = app.canvas;
        let (min, max) = (GeoPos::new(-118.5, 33.9), GeoPos::new(-118.0, 34.3));
        app.view.fit_bounds(min, max, rect, 19.0);
        let (whole_map, shapes) = frame_cost(&app);
        println!(
            "zoomed out: {whole_map:.2} ms, {shapes} shapes, zoom {:.2}",
            app.view.zoom
        );
        for zoom in [11.0, 12.0, 13.0, 14.0, 16.0] {
            app.view = MapView::default();
            app.view.focus(GeoPos::new(-118.25, 34.1), rect, zoom, 19.0);
            let (cost, shapes) = frame_cost(&app);
            println!("zoom {zoom:>4}: {cost:.2} ms, {shapes} shapes");
        }
    }

    /// The marks `paint_nodes` puts on the canvas, as a count of shapes.
    fn node_shapes(app: &Metrocraft) -> usize {
        let ctx = egui::Context::default();
        let spacing = app.node_spacing(app.canvas);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            app.paint_nodes(&painter, app.canvas, spacing);
        });
        out.textures_delta.clear();
        out.shapes.len()
    }

    #[test]
    fn a_crowded_map_drops_the_marks_it_cannot_show() {
        let mut app = Metrocraft::editing(huge_project(4_000, 8_000, 12));
        let rect = app.canvas;
        let (min, max) = (GeoPos::new(-118.5, 33.9), GeoPos::new(-118.0, 34.3));
        app.view.fit_bounds(min, max, rect, 19.0);
        // Over the whole map the marks would sit a pixel or two apart, so only
        // the stations that hold the network's shape are drawn.
        assert!(app.node_spacing(rect) < STOP_SPACING);
        let crowded = node_shapes(&app);
        assert!(crowded < 200, "{crowded} marks drawn over the whole map");
    }

    #[test]
    fn a_route_is_stroked_only_where_it_can_be_seen() {
        let rect = rect();
        let points = vec![
            Pos2::new(-400.0, 300.0),
            Pos2::new(-200.0, 300.0),
            Pos2::new(100.0, 300.0),
            Pos2::new(300.0, 300.0),
            Pos2::new(400.0, -300.0),
            Pos2::new(500.0, -300.0),
            Pos2::new(600.0, 400.0),
            Pos2::new(700.0, 400.0),
        ];
        let runs = visible_runs(&points, rect);
        // A route that wanders off the top of the map and comes back is stroked
        // as two stretches, each carrying the point just past its edge.
        assert_eq!(runs, vec![1..5, 5..8], "{runs:?}");
    }

    #[test]
    fn a_route_keeps_only_the_bends_a_stroke_can_show() {
        let points = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 0.1),
            Pos2::new(20.0, 0.3),
            Pos2::new(30.0, -0.1),
            Pos2::new(40.0, 0.0),
        ];
        // A stroke 2px wide cannot show a fraction of a pixel of bend.
        let kept = thin_points(&points, 2.0);
        assert_eq!(kept, vec![points[0], points[4]]);
        // A hairline keeps them all.
        assert_eq!(thin_points(&points, 0.01), points);
    }

    #[test]
    fn file_stem_is_safe_for_downloads() {
        assert_eq!(file_stem("My & Map"), "my---map");
        assert_eq!(file_stem("   "), "metrocraft-map");
    }

    #[test]
    fn a_name_is_kept_but_never_a_path() {
        assert_eq!(clean_name("  Paris Metro "), Some("Paris Metro".to_owned()));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name(".."), None);
        assert_eq!(clean_name("maps/paris"), None);
        assert_eq!(clean_name("maps\\paris"), None);

        assert_eq!(safe_file_name("Berlin: 1989?/x"), "Berlin- 1989--x");
        assert_eq!(safe_file_name("Map. "), "Map");
        assert_eq!(safe_file_name(".."), "map");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn clearing_the_recent_list_leaves_the_maps_alone() {
        let folder = std::env::temp_dir().join(format!(
            "metrocraft-recent-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let map = folder.join("one.json");
        std::fs::write(&map, "{}").unwrap();

        let mut app = Metrocraft::editing(Project::new("one"));
        app.projects = vec![map.clone()];
        assert_eq!(app.recent_names(), vec!["one".to_owned()]);

        app.run_menu(Some(MenuAction::ClearRecent), &egui::Context::default());
        assert!(app.projects.is_empty());
        assert!(app.recent_names().is_empty());
        assert!(map.exists());
        assert_eq!(std::fs::read_to_string(&map).unwrap(), "{}");

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_rename_moves_the_file_and_the_map_in_hand_with_it() {
        let folder = std::env::temp_dir().join(format!(
            "metrocraft-rename-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let old = folder.join("paris.json");
        std::fs::write(&old, "{}").unwrap();

        let mut app = Metrocraft::editing(Project::new("paris"));
        app.path = Some(old.clone());
        app.projects = vec![old.clone()];

        app.rename_row(0, "Paris Metro".to_owned());
        let renamed = folder.join("Paris Metro.json");
        assert!(renamed.exists());
        assert!(!old.exists());
        assert_eq!(std::fs::read_to_string(&renamed).unwrap(), "{}");
        assert_eq!(app.projects[0], renamed);
        assert_eq!(app.path.as_ref(), Some(&renamed));

        // A name another map already has is refused, and nothing moves.
        let other = folder.join("Berlin.json");
        std::fs::write(&other, "{}").unwrap();
        app.projects.push(other.clone());
        app.rename_row(0, "Berlin".to_owned());
        assert!(app.error.is_some());
        assert!(renamed.exists());
        assert!(other.exists());

        // The map in hand is renamed along with its own file.
        app.error = None;
        app.rename_document("Lyon".to_owned());
        assert_eq!(app.project.name, "Lyon");
        let lyon = folder.join("Lyon.json");
        assert_eq!(app.path.as_ref(), Some(&lyon));
        assert!(lyon.exists());
        assert!(!renamed.exists());
        assert_eq!(app.projects[0], lyon);

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_new_map_is_named_around_the_ones_already_saved() {
        let taken = ["Untitled".to_owned(), "Untitled 2".to_owned()];
        assert_eq!(free_name("Untitled", &taken), "Untitled 3");
        assert_eq!(free_name("Paris", &taken), "Paris");
        assert_eq!(free_name("Untitled", &[]), "Untitled");
    }

    #[test]
    fn a_profile_link_is_an_account_and_a_view_link_is_a_map() {
        assert!(account_link(
            "https://metrodreamin.com/user/3MAkSMqXSDX8ZM8a8vrJiFDYJoa2"
        ));
        assert!(account_link("3MAkSMqXSDX8ZM8a8vrJiFDYJoa2"));
        assert!(!account_link(
            "https://metrodreamin.com/view/M01kRDA4SjNieFptWnFzd0NIcWhVM2kzenoxMnww"
        ));
        assert!(!account_link(""));
    }

    #[test]
    fn an_import_lands_beside_the_maps_already_in_the_folder() {
        let folder = std::env::temp_dir().join(format!(
            "metrocraft-import-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();

        let first = save_into(&folder, "Current LA Metro", "{}").unwrap();
        let second = save_into(&folder, "Current LA Metro", "{}").unwrap();
        assert_eq!(first.file_name().unwrap(), "current-la-metro.json");
        assert_eq!(second.file_name().unwrap(), "current-la-metro (2).json");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{}");
        assert!(second.exists());

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_map_of_an_account_lands_in_its_folder_and_the_list() {
        let folder = std::env::temp_dir().join(format!(
            "metrocraft-account-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();

        let mut app = Metrocraft::editing(Project::new("test"));
        app.bulk = Some(Bulk {
            folder: Some(folder.clone()),
            total: 2,
            ..Bulk::default()
        });
        app.write_import(Project::new("Current LA Metro"));
        assert_eq!(
            app.bulk_progress().map(|(_, count)| count),
            Some("1 / 2".to_owned())
        );
        app.write_import(Project::new("Current LA Metro"));

        let bulk = app.bulk.as_ref().expect("the import is still running");
        assert_eq!((bulk.saved, bulk.failed), (2, 0));
        assert_eq!(app.projects.len(), 2);
        assert!(folder.join("current-la-metro.json").exists());
        assert!(folder.join("current-la-metro (2).json").exists());

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_account_lands_in_the_project_list() {
        let mut app = Metrocraft::editing(Project::new("test"));
        for number in 0..20 {
            app.remember_project(std::path::PathBuf::from(format!("C:/maps/{number}.json")));
        }
        // Most recent first, and an account of twenty is all still there.
        assert_eq!(app.projects.len(), 20);
        assert_eq!(app.projects[0], std::path::PathBuf::from("C:/maps/19.json"));

        // Opening one already in the list moves it to the head, once.
        app.remember_project(std::path::PathBuf::from("C:/maps/5.json"));
        assert_eq!(app.projects.len(), 20);
        assert_eq!(app.projects[0], std::path::PathBuf::from("C:/maps/5.json"));
    }
}
