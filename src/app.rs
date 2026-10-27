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

use crate::keys::{self, Action, Keys};
use crate::model::{GeoPos, Id as MapId, Line, Node, Project};
use crate::platform;
use crate::theme;
use crate::tiles::{Basemap, TileKey, TileStore, Tone, PREVIEW_TILE};
use crate::view::MapView;

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
const PREFS_TABS: [&str; 2] = ["Basemap", "Keys"];
/// Where `Keys` sits in [`PREFS_TABS`].
const KEYS_TAB: usize = 1;
/// A row in the Keys tab: one command and the key it answers to.
const KEY_ROW: f32 = 26.0;
const PREFS_RAIL: f32 = 88.0;
/// Where the launcher keeps the maps it has opened, apart from the map in hand.
#[cfg(not(target_arch = "wasm32"))]
const PROJECTS_KEY: &str = "metrocraft-projects-v1";
#[cfg(not(target_arch = "wasm32"))]
const PROJECTS_MAX: usize = 12;
/// The launcher: a column of maps, each a row of name over the path it lives at.
#[cfg(not(target_arch = "wasm32"))]
const LAUNCH_WIDTH: f32 = 440.0;
#[cfg(not(target_arch = "wasm32"))]
const LAUNCH_ROW: f32 = 46.0;

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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Station(MapId),
    Waypoint(MapId),
    /// A line and the index of the segment under the cursor.
    Line(MapId, usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drag {
    Pan,
    Station(MapId),
    Waypoint(MapId),
}

/// The app opens on the project list, and leaves it for a map.
#[cfg(not(target_arch = "wasm32"))]
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
    /// Back to the list of maps, which the browser has no use for.
    #[cfg(not(target_arch = "wasm32"))]
    Projects,
}

/// What the persisted document looks like between sessions.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Document {
    project: Project,
    view: MapView,
    basemap: Basemap,
    keys: Keys,
}

#[derive(Default)]
struct PanelAction {
    close: bool,
    delete: bool,
    toggle: Option<(MapId, Node)>,
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
    basemap: Basemap,
    keys: Keys,
    /// The command whose key is being changed, if the user is mid-change.
    listening: Option<Action>,
    preferences: bool,
    prefs_tab: usize,
    lines_open: bool,
    lines_pick: Option<MapId>,
    focus_name: Option<MapId>,
    #[cfg(not(target_arch = "wasm32"))]
    path: Option<std::path::PathBuf>,
    /// The maps the launcher offers, most recently opened first.
    #[cfg(not(target_arch = "wasm32"))]
    projects: Vec<std::path::PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    screen: Screen,
    #[cfg(target_arch = "wasm32")]
    load_rx: Option<Receiver<Result<String, String>>>,
}

impl Metrocraft {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);

        let document: Document = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, STORAGE_KEY))
            .unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        let projects = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, PROJECTS_KEY))
            .unwrap_or_default();
        let mut project = document.project;
        project.sanitize();
        Self {
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
            basemap: document.basemap,
            keys: document.keys,
            listening: None,
            preferences: false,
            prefs_tab: 0,
            lines_open: false,
            lines_pick: None,
            focus_name: None,
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
            #[cfg(not(target_arch = "wasm32"))]
            projects,
            #[cfg(not(target_arch = "wasm32"))]
            screen: Screen::Projects,
            #[cfg(target_arch = "wasm32")]
            load_rx: None,
        }
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
        self.request_fit();
        Ok(())
    }

    fn new_map(&mut self) {
        if self.project.stations.is_empty()
            && self.project.waypoints.is_empty()
            && self.project.lines.is_empty()
        {
            return;
        }
        self.snapshot();
        self.project = Project::new("Untitled");
        self.selection = Selection::None;
        self.view = MapView::default();
        self.pending_fit = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.path = None;
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
            Selection::Line(_) | Selection::None => None,
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
                    self.lines_open = true;
                    Selection::Line(id)
                }
            };
        } else {
            let pos = self.view.screen_to_world(screen, rect).sanitized();
            self.place_node(pos);
        }
        // Keep "what a click creates" in step with the node in hand.
        match self.selection {
            Selection::Station(_) => self.place = Place::Station,
            Selection::Waypoint(_) => self.place = Place::Waypoint,
            Selection::Line(_) | Selection::None => {}
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
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let fired: Vec<Action> = ctx.input(|input| {
            keys::ACTIONS
                .into_iter()
                .filter(|action| self.keys.pressed(*action, input))
                .collect()
        });
        for action in fired {
            match action {
                Action::Undo => self.undo(),
                Action::Redo => self.redo(),
                Action::Save => self.save_document(false),
                Action::SaveAs => self.save_document(true),
                Action::Open => self.open_document(),
                Action::New => self.new_map(),
                Action::Fit => self.request_fit(),
                Action::Preferences => self.preferences = true,
            }
        }

        let (escape, delete) = ctx.input(|input| {
            (
                input.key_pressed(Key::Escape),
                input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace),
            )
        });
        if escape {
            self.selection = Selection::None;
            self.preferences = false;
        }
        if delete {
            self.delete_selection();
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
            Tone::Dark => theme::CANVAS,
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

        self.paint_lines(&painter, rect);
        self.paint_labels(&painter, rect);
        self.paint_nodes(&painter, rect);

        // A click while a key is being changed is only there to cancel it.
        if self.listening.is_none() {
            self.handle_pointer(ui, &response, rect);
        }

        if response.hovered() {
            let icon = match self.hover {
                Some(Hit::Station(_) | Hit::Waypoint(_)) if self.drag_moved => CursorIcon::Grabbing,
                Some(Hit::Station(_) | Hit::Waypoint(_)) => CursorIcon::Grab,
                Some(Hit::Line(_, _)) => CursorIcon::PointingHand,
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
            Tone::Light => theme::STATION_EDGE,
            Tone::Dark => theme::MAP_INK_ON_DARK,
        };
        for (id, line) in &self.project.lines {
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

            if self.selection == Selection::Line(*id) {
                painter.add(Shape::line(
                    points.clone(),
                    Stroke::new(width + 12.0, theme::alpha(ink, 55)),
                ));
            } else if matches!(self.hover, Some(Hit::Line(hovered, _)) if hovered == *id) {
                painter.add(Shape::line(
                    points.clone(),
                    Stroke::new(width + 9.0, theme::alpha(ink, 32)),
                ));
            }

            painter.add(Shape::line(
                points.clone(),
                Stroke::new(width + 3.0, theme::alpha(casing, 190)),
            ));
            if ghost.is_some() {
                painter.add(Shape::line(
                    points.clone(),
                    Stroke::new(width, theme::alpha(color, 215)),
                ));
                // A hairline down the middle, so the route reads as a suggestion.
                painter.add(Shape::line(
                    points,
                    Stroke::new(1.4, theme::alpha(theme::INK, 200)),
                ));
            } else {
                painter.add(Shape::line(points, Stroke::new(width, color)));
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
            points.push(points[0]);
        }
        Some(points)
    }

    fn paint_labels(&self, painter: &Painter, rect: Rect) {
        if self.view.zoom < 6.0 {
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

    fn paint_nodes(&self, painter: &Painter, rect: Rect) {
        let degree = self.project.station_degree();
        let (ink, halo) = self.map_ink();

        if self.show_waypoints {
            for (id, waypoint) in &self.project.waypoints {
                let pos = self.view.world_to_screen(waypoint.pos, rect);
                if !rect.expand(30.0).contains(pos) {
                    continue;
                }
                let attached = self
                    .project
                    .lines
                    .values()
                    .any(|line| line.nodes.contains(&Node::Waypoint(*id)));
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
                    painter.circle_stroke(pos, 7.5, Stroke::new(1.4, theme::ACCENT));
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
            let interchange = degree.get(id).copied().unwrap_or(0) >= 2;
            let radius = if interchange { 5.0 } else { 3.6 };
            if self.selection == Selection::Station(*id) {
                painter.circle_stroke(pos, radius + 4.5, Stroke::new(1.3, theme::ACCENT));
            } else if self.hover == Some(Hit::Station(*id)) {
                painter.circle_stroke(
                    pos,
                    radius + 3.5,
                    Stroke::new(1.0, theme::alpha(theme::INK, 90)),
                );
            }
            painter.circle(
                pos,
                radius,
                theme::STATION,
                Stroke::new(1.8, theme::STATION_EDGE),
            );
        }
    }

    // -------------------------------------------------------------- overlays

    fn menu_bar(&mut self, ui: &mut Ui) {
        let mut action: Option<MenuAction> = None;
        Panel::top("menu-bar")
            .frame(
                Frame::new()
                    .fill(theme::PANEL)
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
                        ui.separator();
                        if menu_item(ui, &self.keys, "Save", Action::Save) {
                            action = Some(MenuAction::Save);
                        }
                        if menu_item(ui, &self.keys, "Save as…", Action::SaveAs) {
                            action = Some(MenuAction::SaveAs);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            ui.separator();
                            if plain_menu_item(ui, "Projects") {
                                action = Some(MenuAction::Projects);
                            }
                        }
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
                });
            });

        match action {
            Some(MenuAction::New) => self.new_map(),
            Some(MenuAction::Open) => self.open_document(),
            Some(MenuAction::Save) => self.save_document(false),
            Some(MenuAction::SaveAs) => self.save_document(true),
            Some(MenuAction::Fit) => self.request_fit(),
            Some(MenuAction::Undo) => self.undo(),
            Some(MenuAction::Redo) => self.redo(),
            Some(MenuAction::Preferences) => self.preferences = true,
            #[cfg(not(target_arch = "wasm32"))]
            Some(MenuAction::Projects) => {
                // Leave nothing from the editor open behind the list.
                self.preferences = false;
                self.listening = None;
                self.screen = Screen::Projects;
            }
            None => {}
        }
    }

    /// The lines nearest the node, as colour swatches.
    fn nearby_lines(&self, node: Node, pos: GeoPos) -> Vec<(MapId, [u8; 3], bool)> {
        let anchor = self.view.world_to_screen(pos, self.canvas);
        let mut found: Vec<(f32, MapId, [u8; 3], bool)> = Vec::new();
        for (id, line) in &self.project.lines {
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
            Selection::Line(_) | Selection::None => return,
        };
        if self.canvas.width() < 40.0 {
            return;
        }

        let anchor = self.view.world_to_screen(pos, self.canvas);
        let nearby = self.nearby_lines(node, pos);
        let items = nearby.len() + 2;
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
        let mut convert = false;
        let mut delete = false;

        egui::Area::new(Id::unique("node-card"))
            .order(Order::Foreground)
            .fixed_pos(top_left)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::PANEL_SOLID)
                    .stroke(Stroke::new(1.0, theme::HAIRLINE))
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

    fn panel(&mut self, ctx: &egui::Context) {
        let Some(node) = self.selection_node() else {
            return;
        };
        if self.canvas.width() < 1.0 {
            return;
        }
        let lines: Vec<(MapId, String, [u8; 3])> = self
            .project
            .lines
            .iter()
            .map(|(id, line)| (*id, line.name.clone(), line.color))
            .collect();
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
                    .fill(theme::PANEL_SOLID)
                    .stroke(Stroke::new(1.0, theme::HAIRLINE))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(PANEL_WIDTH);
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 8.0);

                        match self.selection {
                            Selection::Station(id) => self.station_header(ui, id, &mut action),
                            Selection::Waypoint(_) => self.close_row(ui, &mut action),
                            Selection::Line(_) | Selection::None => {}
                        }
                        self.lines_section(ui, &lines, node, &mut action);
                        ui.add_space(2.0);
                        if danger_button(ui, "Delete").clicked() {
                            action.delete = true;
                        }
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
                    .fill(theme::PANEL_SOLID)
                    .stroke(Stroke::new(1.0, theme::HAIRLINE))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(9))
                    .show(ui, |ui| {
                        ui.set_width(if self.lines_open { 214.0 } else { 84.0 });
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 7.0);
                        ui.horizontal(|ui| {
                            if chevron(ui, self.lines_open).clicked() {
                                self.lines_open = !self.lines_open;
                            }
                            ui.label(RichText::new("Lines").font(medium(12.5)).color(theme::INK));
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
        let lines: Vec<(MapId, String, [u8; 3])> = self
            .project
            .lines
            .iter()
            .map(|(id, line)| (*id, line.name.clone(), line.color))
            .collect();
        if lines.is_empty() {
            return;
        }
        ScrollArea::vertical()
            .max_height(160.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                for (id, name, color) in &lines {
                    let picked = self.lines_pick == Some(*id);
                    if line_row(ui, name, *color, false, picked).clicked() {
                        self.lines_pick = Some(*id);
                    }
                }
            });
        match self.lines_pick {
            Some(id) if self.project.lines.contains_key(&id) => self.line_editor(ui, id),
            Some(_) => self.lines_pick = None,
            None => {}
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
        if remove {
            self.snapshot();
            self.project.remove_line(id);
            if self.selection == Selection::Line(id) {
                self.selection = Selection::None;
            }
            self.lines_pick = None;
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
        lines: &[(MapId, String, [u8; 3])],
        node: Node,
        action: &mut PanelAction,
    ) {
        if lines.is_empty() {
            return;
        }
        ui.add(search_edit(&mut self.line_query, PANEL_WIDTH));

        let query = self.line_query.trim().to_lowercase();
        ScrollArea::vertical()
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                for (id, name, color) in lines {
                    if !query.is_empty() && !name.to_lowercase().contains(&query) {
                        continue;
                    }
                    let attached = self.project.has_node(*id, node);
                    let response = line_row(ui, name, *color, attached, false);
                    if !attached && response.hovered() {
                        self.preview = Some((*id, node));
                    }
                    if response.clicked() {
                        action.toggle = Some((*id, node));
                    }
                }
            });
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
                    .fill(theme::PANEL_SOLID)
                    .stroke(Stroke::new(1.0, theme::HAIRLINE))
                    .corner_radius(CornerRadius::same(14))
                    .shadow(ui.visuals().window_shadow)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            let title = RichText::new("Preferences").font(medium(12.5));
                            ui.label(title.color(theme::INK));
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
                painter.rect_filled(rect, CornerRadius::same(6), theme::RAISED);
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(6),
                    Stroke::new(1.0, theme::ACCENT),
                    StrokeKind::Inside,
                );
            } else if response.hovered() {
                painter.rect_filled(rect, CornerRadius::same(6), theme::RAISED_HOVER);
            }
            painter.text(
                rect.left_center() + Vec2::new(8.0, 0.0),
                Align2::LEFT_CENTER,
                action.label(),
                medium(12.0),
                theme::INK,
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
                    theme::ACCENT
                } else {
                    theme::INK_FAINT
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
                            .font(medium(10.0))
                            .color(theme::INK_FAINT),
                    );
                    ui.hyperlink_to(
                        RichText::new(credit)
                            .font(medium(10.0))
                            .color(theme::INK_DIM),
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
                    .fill(theme::OVERLAY)
                    .stroke(Stroke::new(1.0, theme::HAIRLINE))
                    .corner_radius(CornerRadius::same(255))
                    .inner_margin(Margin::symmetric(14, 7))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Click to place a station")
                                .font(medium(11.5))
                                .color(theme::INK_DIM),
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
                    .fill(theme::PANEL_SOLID)
                    .stroke(Stroke::new(1.0, theme::HAIRLINE))
                    .corner_radius(CornerRadius::same(255))
                    .inner_margin(Margin::symmetric(14, 7))
                    .show(ui, |ui| {
                        ui.label(RichText::new(message).font(medium(11.5)).color(theme::INK));
                    });
            });
        // Come back when it goes, so nothing has to wait for a mouse move.
        ctx.request_repaint_after_secs((until - self.now).max(0.0) as f32);
    }

    // ---------------------------------------------------------------- projects

    /// The list the app opens on: the maps it has opened, where they live on
    /// disk, and the way to any other one.
    #[cfg(not(target_arch = "wasm32"))]
    fn launcher(&mut self, ctx: &egui::Context) {
        let mut open: Option<usize> = None;
        let mut forget: Option<usize> = None;
        let mut browse = false;
        let mut create = false;

        egui::Area::new(Id::unique("projects"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(LAUNCH_WIDTH);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new("Metrocraft")
                        .font(medium(13.0))
                        .color(theme::INK),
                );
                ui.add_space(14.0);
                for (index, path) in self.projects.iter().enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(LAUNCH_WIDTH, LAUNCH_ROW), Sense::click());
                    let painter = ui.painter();
                    if response.hovered() {
                        painter.rect_filled(rect, CornerRadius::same(8), theme::RAISED_HOVER);
                    }
                    let name = path
                        .file_stem()
                        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
                    paint_line(
                        painter,
                        rect.left_top() + Vec2::new(12.0, 8.0),
                        &name,
                        medium(12.5),
                        theme::INK,
                        LAUNCH_WIDTH - 44.0,
                    );
                    paint_line(
                        painter,
                        rect.left_top() + Vec2::new(12.0, 26.0),
                        &path.display().to_string(),
                        medium(11.0),
                        theme::INK_FAINT,
                        LAUNCH_WIDTH - 44.0,
                    );

                    // The cross that takes a map off the list, under the cursor.
                    let cross = Rect::from_center_size(
                        Pos2::new(rect.right() - 16.0, rect.center().y),
                        Vec2::splat(18.0),
                    );
                    if ctx
                        .pointer_latest_pos()
                        .is_some_and(|pos| cross.contains(pos))
                    {
                        painter_cross(painter, cross.center(), 3.5, Stroke::new(1.1, theme::INK));
                        let hit = ui.interact(
                            cross,
                            ui.scope_id().with(("forget", index)),
                            Sense::click(),
                        );
                        if hit.clicked() {
                            forget = Some(index);
                        }
                    } else if response.clicked() {
                        open = Some(index);
                    }
                }
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
                });
            });

        if let Some(index) = forget {
            self.projects.remove(index);
        } else if let Some(index) = open {
            self.open_project(index);
        } else if browse {
            self.open_document();
        } else if create {
            self.new_project();
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
        self.projects.retain(|old| *old != path);
        self.projects.insert(0, path.clone());
        self.projects.truncate(PROJECTS_MAX);
        self.path = Some(path);
        self.screen = Screen::Editor;
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
    fn save_document(&mut self, save_as: bool) {
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

    #[cfg(target_arch = "wasm32")]
    fn save_document(&mut self, _save_as: bool) {
        let json = match self.to_json() {
            Ok(json) => json,
            Err(err) => return self.set_error(err),
        };
        let filename = format!("{}.json", self.file_stem());
        if let Err(err) = platform::download_json(&filename, &json) {
            self.set_error(err);
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
                if let Err(err) = self.apply_json(&text) {
                    self.set_error(err);
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
}

impl eframe::App for Metrocraft {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|input| input.time);
        if let Some((_, until)) = &self.error {
            if self.now > *until {
                self.error = None;
            }
        }

        self.poll_load();
        // The list stands on its own until a map is open.
        #[cfg(not(target_arch = "wasm32"))]
        if self.screen == Screen::Projects {
            self.launcher(&ctx);
            self.error_toast(&ctx);
            return;
        }
        self.handle_keys(&ctx);

        self.menu_bar(ui);
        self.canvas(ui);
        // The credit belongs to the map, so it sits under anything drawn over it.
        self.attribution(&ctx);

        self.preview = None;
        self.lines_box(&ctx);
        self.node_card(&ctx);
        self.panel(&ctx);
        self.preferences(&ctx);
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
            },
        );
        #[cfg(not(target_arch = "wasm32"))]
        eframe::set_value(storage, PROJECTS_KEY, &self.projects);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        #[cfg(not(target_arch = "wasm32"))]
        if self.screen == Screen::Projects {
            return theme::PANEL.to_normalized_gamma_f32();
        }
        theme::CANVAS.to_normalized_gamma_f32()
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

    painter.rect_filled(thumb, CornerRadius::same(4), theme::RAISED);
    if let Some(texture) = tiles.ready(id) {
        painter.image(texture.id(), thumb, whole, Color32::WHITE);
    } else {
        tiles.ensure(id);
    }

    let stroke = if current {
        Stroke::new(1.8, theme::ACCENT)
    } else if response.hovered() {
        Stroke::new(1.0, theme::INK_DIM)
    } else {
        Stroke::new(1.0, theme::HAIRLINE)
    };
    painter.rect_stroke(thumb, CornerRadius::same(4), stroke, StrokeKind::Outside);

    let name = ellipsize(painter, basemap.name(), &medium(11.0), PREVIEW_SIDE);
    painter.text(
        Pos2::new(thumb.center().x, thumb.max.y + 2.0),
        Align2::CENTER_TOP,
        name,
        medium(11.0),
        if current { theme::INK } else { theme::INK_DIM },
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
        .color(theme::INK_FAINT);
    ui.add(Button::new(RichText::new(label).font(medium(12.5))).shortcut_text(shortcut))
        .clicked()
}

/// A menu row for something no key answers to.
#[cfg(not(target_arch = "wasm32"))]
fn plain_menu_item(ui: &mut Ui, label: &str) -> bool {
    ui.add(Button::new(RichText::new(label).font(medium(12.5))))
        .clicked()
}

/// One line of text, cut off with an ellipsis rather than wrapped, so that a
/// long path cannot spill out of its row.
#[cfg(not(target_arch = "wasm32"))]
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
        .text_color(theme::INK)
        .margin(Margin::symmetric(0, 2))
        .desired_width(width)
}

fn search_edit(text: &mut String, width: f32) -> TextEdit<'_> {
    TextEdit::singleline(text)
        .frame(
            Frame::new()
                .fill(theme::RAISED)
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::symmetric(8, 4)),
        )
        .hint_text(
            RichText::new("Search")
                .font(medium(12.0))
                .color(theme::INK_GHOST),
        )
        .font(medium(12.0))
        .text_color(theme::INK)
        .margin(Margin::symmetric(0, 0))
        .desired_width(width)
}

/// One half of the station/waypoint switch.
/// A square button in the node bar, with its hover bed.
fn square_button(ui: &mut Ui) -> (Rect, Response) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CARD_BUTTON), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), theme::RAISED_HOVER);
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
            Stroke::new(1.2, theme::INK),
            StrokeKind::Outside,
        );
    } else if response.hovered() {
        painter.rect_stroke(
            fill.expand(1.5),
            CornerRadius::same(2),
            Stroke::new(1.0, theme::INK_DIM),
            StrokeKind::Outside,
        );
    }
    response
}

/// Station ⇄ waypoint. The arrow points at what a click makes of this node.
fn arrow_button(ui: &mut Ui, to_waypoint: bool) -> Response {
    let (rect, response) = square_button(ui);
    let color = if response.hovered() {
        theme::INK
    } else {
        theme::INK_DIM
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
        theme::DANGER
    } else {
        theme::INK_DIM
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
        painter.rect_filled(rect, CornerRadius::same(6), theme::RAISED_HOVER);
    }
    let color = if response.hovered() {
        theme::INK
    } else {
        theme::INK_DIM
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
        theme::INK
    } else {
        theme::INK_FAINT
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
        theme::INK
    } else {
        theme::INK_FAINT
    };
    let centre = rect.center();
    let stroke = Stroke::new(1.1, color);
    painter_cross(ui.painter(), centre, 3.5, stroke);
    response
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

fn line_row(ui: &mut Ui, name: &str, color: [u8; 3], attached: bool, active: bool) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, CornerRadius::same(6), theme::RAISED);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), theme::RAISED_HOVER);
    }
    let bar = Rect::from_center_size(
        Pos2::new(rect.left() + 7.0, rect.center().y),
        Vec2::new(4.0, 14.0),
    );
    painter.rect_filled(bar, CornerRadius::same(2), theme::rgb(color));
    let text_color = if active || response.hovered() {
        theme::INK
    } else {
        theme::INK_DIM
    };
    let font = medium(12.5);
    let room = rect.width() - 34.0;
    painter.text(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        ellipsize(painter, name, &font, room),
        font,
        text_color,
    );
    if attached {
        painter.circle_filled(
            Pos2::new(rect.right() - 8.0, rect.center().y),
            3.0,
            theme::INK,
        );
    }
    response
}

fn ellipsize(painter: &Painter, text: &str, font: &FontId, width: f32) -> String {
    if painter
        .layout_no_wrap(text.to_owned(), font.clone(), theme::INK)
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
            .layout_no_wrap(candidate.clone(), font.clone(), theme::INK)
            .size()
            .x
            <= width
        {
            return candidate;
        }
    }
    "…".to_owned()
}

fn danger_button(ui: &mut Ui, label: &str) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(5), theme::alpha(theme::DANGER, 20));
    }
    painter.text(
        Pos2::new(rect.left() + 2.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        medium(12.0),
        if response.hovered() {
            theme::INK
        } else {
            theme::DANGER
        },
    );
    response
}

fn hex_edit(text: &mut String) -> TextEdit<'_> {
    TextEdit::singleline(text)
        .frame(
            Frame::new()
                .fill(theme::RAISED)
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::symmetric(8, 4)),
        )
        .font(medium(12.0))
        .text_color(theme::INK)
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

    for (id, line) in &project.lines {
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
            basemap: Basemap::default(),
            keys: Keys::default(),
            listening: None,
            preferences: false,
            prefs_tab: 0,
            lines_open: false,
            lines_pick: None,
            focus_name: None,
            path: None,
            projects: Vec::new(),
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
    fn file_stem_is_safe_for_downloads() {
        assert_eq!(file_stem("My & Map"), "my---map");
        assert_eq!(file_stem("   "), "metrocraft-map");
    }
}
