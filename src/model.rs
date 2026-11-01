//! The map document: stations, lines, and the waypoints that shape them.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub type Id = u32;

/// A position on the world map, in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoPos {
    pub lon: f32,
    pub lat: f32,
}

impl GeoPos {
    pub const fn new(lon: f32, lat: f32) -> Self {
        Self { lon, lat }
    }

    pub fn sanitized(self) -> Self {
        Self {
            lon: if self.lon.is_finite() {
                self.lon.clamp(-180.0, 180.0)
            } else {
                0.0
            },
            lat: if self.lat.is_finite() {
                self.lat.clamp(-89.9, 89.9)
            } else {
                0.0
            },
        }
    }
}

/// Longitude difference folded into -180..180, so work near the antimeridian
/// does not wrap the wrong way round the world.
fn wrap_lon(delta: f32) -> f32 {
    let mut delta = delta % 360.0;
    if delta > 180.0 {
        delta -= 360.0;
    } else if delta < -180.0 {
        delta += 360.0;
    }
    delta
}

/// Distance in degrees along a rhumb line, close enough for comparing angles.
fn rhumb_degrees(a: GeoPos, b: GeoPos) -> f32 {
    let dlat = b.lat - a.lat;
    let dlon = wrap_lon(b.lon - a.lon);
    let scale = ((a.lat + b.lat) * 0.5).to_radians().cos();
    (dlat * dlat + dlon * dlon * scale * scale).sqrt()
}

/// Compass bearing from `a` to `b`, in degrees clockwise from north.
fn rhumb_bearing(a: GeoPos, b: GeoPos) -> f32 {
    let dlon = wrap_lon(b.lon - a.lon).to_radians();
    let stretch = |lat: f32| {
        (lat.to_radians() * 0.5 + std::f32::consts::FRAC_PI_4)
            .tan()
            .ln()
    };
    let dlat = stretch(b.lat) - stretch(a.lat);
    if dlon == 0.0 && dlat == 0.0 {
        return 0.0;
    }
    dlon.atan2(dlat).to_degrees().rem_euclid(360.0)
}

fn midpoint(a: GeoPos, b: GeoPos) -> GeoPos {
    GeoPos::new(a.lon + wrap_lon(b.lon - a.lon) * 0.5, (a.lat + b.lat) * 0.5)
}

/// How close two stations have to be for a walking interchange to make sense:
/// half a mile, the distance MetroDreamin' walks.
pub const WALK_KM: f32 = 0.804_672;
/// Minutes on foot per kilometre, at twenty minutes to the mile.
const WALK_MINUTES_PER_KM: f32 = 12.427_424;

/// How long the walk between two stations takes, in whole minutes.
pub fn walk_minutes(km: f32) -> u32 {
    ((km * WALK_MINUTES_PER_KM).round() as u32).max(1)
}

/// Great-circle distance in kilometres, for line lengths and walking links.
pub fn distance_km(a: GeoPos, b: GeoPos) -> f32 {
    let lat1 = (a.lat as f64).to_radians();
    let lat2 = (b.lat as f64).to_radians();
    let dlat = lat2 - lat1;
    let dlon = (wrap_lon(b.lon - a.lon) as f64).to_radians();
    let h = (dlat * 0.5).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon * 0.5).sin().powi(2);
    (2.0 * h.sqrt().clamp(0.0, 1.0).asin() * 6371.0) as f32
}

/// How high a station sits relative to the ground it is drawn on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Elevation {
    Above,
    #[default]
    At,
    Below,
}

impl Elevation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Above => "Above grade",
            Self::At => "At grade",
            Self::Below => "Below grade",
        }
    }

    /// The three, highest first: the order they are offered in.
    pub const ALL: [Self; 3] = [Self::Above, Self::At, Self::Below];
}

/// What a line carries. The keys are MetroDreamin's, so a map from there and a
/// map saved here name a mode the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Rapid,
    Light,
    Tram,
    Regional,
    LongDistance,
    HighSpeed,
    Brt,
    Bus,
    Ferry,
    Gondola,
    Air,
}

impl Mode {
    /// Every mode, rail first, then road, water, cable and air.
    pub const ALL: [Self; 11] = [
        Self::Rapid,
        Self::Light,
        Self::Tram,
        Self::Regional,
        Self::LongDistance,
        Self::HighSpeed,
        Self::Brt,
        Self::Bus,
        Self::Ferry,
        Self::Gondola,
        Self::Air,
    ];

    /// The key the mode is written as, as MetroDreamin' writes it.
    pub fn key(self) -> &'static str {
        match self {
            Self::Rapid => "RAPID",
            Self::Light => "LIGHT",
            Self::Tram => "TRAM",
            Self::Regional => "REGIONAL",
            Self::LongDistance => "MLDISTANCE",
            Self::HighSpeed => "HSR",
            Self::Brt => "BRT",
            Self::Bus => "BUS",
            Self::Ferry => "FERRY",
            Self::Gondola => "GONDOLA",
            Self::Air => "AIR",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key.trim().to_ascii_uppercase().as_str() {
            "RAPID" => Self::Rapid,
            "LIGHT" => Self::Light,
            "TRAM" => Self::Tram,
            "REGIONAL" => Self::Regional,
            "MLDISTANCE" => Self::LongDistance,
            "HSR" => Self::HighSpeed,
            "BRT" => Self::Brt,
            "BUS" => Self::Bus,
            "FERRY" => Self::Ferry,
            "GONDOLA" => Self::Gondola,
            "AIR" => Self::Air,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rapid => "Metro",
            Self::Light => "Light rail",
            Self::Tram => "Tram",
            Self::Regional => "Commuter rail",
            Self::LongDistance => "Intercity rail",
            Self::HighSpeed => "High-speed rail",
            Self::Brt => "Bus rapid transit",
            Self::Bus => "Bus",
            Self::Ferry => "Ferry",
            Self::Gondola => "Gondola",
            Self::Air => "Air",
        }
    }
}

impl Serialize for Mode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

/// A mode this build does not know is read as metro rather than losing the map.
impl<'de> Deserialize<'de> for Mode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Ok(Mode::from_key(&key).unwrap_or_default())
    }
}

/// How a line is stroked, whatever it carries. Left to the mode, a line runs
/// the way its mode does; set outright, any line can run solid, dashed, or as a
/// chain of shapes stamped along its route, the ones MetroDreamin' offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pattern {
    #[default]
    Auto,
    Solid,
    Dashed,
    Circle,
    Diamond,
    Plus,
    Star,
    Heart,
}

impl Pattern {
    pub const ALL: [Self; 8] = [
        Self::Auto,
        Self::Solid,
        Self::Dashed,
        Self::Circle,
        Self::Diamond,
        Self::Plus,
        Self::Star,
        Self::Heart,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::Solid => "SOLID",
            Self::Dashed => "DASHED",
            Self::Circle => "CIRCLE",
            Self::Diamond => "DIAMOND",
            Self::Plus => "PLUS",
            Self::Star => "STAR",
            Self::Heart => "HEART",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key.trim().to_ascii_uppercase().as_str() {
            "AUTO" => Self::Auto,
            "SOLID" => Self::Solid,
            "DASHED" => Self::Dashed,
            // A map saved when dots were a pattern of their own still reads as
            // the circle it looked like.
            "DOTTED" => Self::Circle,
            "CIRCLE" => Self::Circle,
            "DIAMOND" => Self::Diamond,
            "PLUS" => Self::Plus,
            "STAR" => Self::Star,
            "HEART" => Self::Heart,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Solid => "Solid",
            Self::Dashed => "Dashed",
            Self::Circle => "Circle",
            Self::Diamond => "Diamond",
            Self::Plus => "Plus",
            Self::Star => "Star",
            Self::Heart => "Heart",
        }
    }
}

impl Serialize for Pattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

/// A pattern this build does not know is read as automatic rather than losing
/// the map.
impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Ok(Pattern::from_key(&key).unwrap_or_default())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Station {
    pub name: String,
    pub pos: GeoPos,
    pub elevation: Elevation,
}

impl Default for Station {
    fn default() -> Self {
        Self {
            name: String::new(),
            pos: GeoPos::new(0.0, 0.0),
            elevation: Elevation::At,
        }
    }
}

impl Station {
    pub fn new(name: impl Into<String>, pos: GeoPos) -> Self {
        Self {
            name: name.into(),
            pos: pos.sanitized(),
            elevation: Elevation::At,
        }
    }
}

/// A shaping node that lines can be attached to. Lines bend at waypoints, but
/// no stop is drawn there and trains would not call at them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    pub pos: GeoPos,
}

impl Waypoint {
    pub fn new(pos: GeoPos) -> Self {
        Self {
            pos: pos.sanitized(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "lowercase")]
pub enum Node {
    Station(Id),
    Waypoint(Id),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Line {
    pub name: String,
    /// sRGB colour of the line.
    pub color: [u8; 3],
    /// Stroke width in screen pixels.
    pub width: f32,
    /// What the line carries: metro, tram, bus, ferry, and the rest.
    pub mode: Mode,
    /// How the line is stroked: as its mode runs, or solid, dashed or a shape
    /// whatever it carries.
    pub pattern: Pattern,
    /// Stops and shaping waypoints, in travel order.
    pub nodes: Vec<Node>,
    /// Whether the last node connects back to the first.
    pub closed: bool,
    /// Whether a closed line doubles back on itself at the loop (a turnback)
    /// rather than crossing straight over.
    pub turnback: bool,
    /// The group this line is filed under, if any.
    pub group: Option<Id>,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            name: "Line".to_owned(),
            color: [235, 87, 87],
            width: 5.0,
            mode: Mode::Rapid,
            pattern: Pattern::Auto,
            nodes: Vec::new(),
            closed: false,
            turnback: false,
            group: None,
        }
    }
}

/// A category lines are filed under, so a network of many lines stays legible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineGroup {
    pub name: String,
    /// Whether the group's lines are kept off the map.
    pub hidden: bool,
    /// Whether the group is folded shut in the Lines box.
    pub open: bool,
}

impl Default for LineGroup {
    fn default() -> Self {
        Self {
            name: "Group".to_owned(),
            hidden: false,
            open: true,
        }
    }
}

/// A walking interchange: two stations close enough to change between on foot.
/// It is drawn like a line, and shaped by waypoints the same way.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transfer {
    pub a: Id,
    pub b: Id,
    /// Waypoints that shape the walk, in order between the two stations.
    pub via: Vec<Id>,
}

/// What the lines make of their nodes. Built in one pass, so a map with tens
/// of thousands of nodes can be drawn without asking the lines again and again.
/// Counts are kept by id rather than in maps: filling and reading them costs no
/// hashing, which is what a map at the size of a real network needs.
#[derive(Default)]
pub struct NodeRoles {
    /// How many lines call at each station.
    degree: Vec<u32>,
    /// The stations a line ends at.
    terminals: Vec<bool>,
    /// The bends at least one line runs through.
    in_use: Vec<bool>,
}

impl NodeRoles {
    /// How many lines call at this station.
    pub fn degree(&self, id: Id) -> u32 {
        self.degree.get(id as usize).copied().unwrap_or(0)
    }

    /// Whether a line ends at this station.
    pub fn is_terminal(&self, id: Id) -> bool {
        self.terminals.get(id as usize).copied().unwrap_or(false)
    }

    /// Whether any line runs through this bend.
    pub fn in_use(&self, id: Id) -> bool {
        self.in_use.get(id as usize).copied().unwrap_or(false)
    }

    fn count(counter: &mut Vec<u32>, id: Id) {
        let index = id as usize;
        if index >= counter.len() {
            counter.resize(index + 1, 0);
        }
        counter[index] += 1;
    }

    fn mark(flags: &mut Vec<bool>, id: Id) {
        let index = id as usize;
        if index >= flags.len() {
            flags.resize(index + 1, false);
        }
        flags[index] = true;
    }
}

/// A complete transport map document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub name: String,
    pub stations: IndexMap<Id, Station>,
    pub waypoints: IndexMap<Id, Waypoint>,
    pub lines: IndexMap<Id, Line>,
    /// Categories lines are filed under.
    pub groups: IndexMap<Id, LineGroup>,
    /// Walking interchanges between two stations.
    pub transfers: IndexMap<Id, Transfer>,
    pub next_id: Id,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "Untitled".to_owned(),
            stations: IndexMap::new(),
            waypoints: IndexMap::new(),
            lines: IndexMap::new(),
            groups: IndexMap::new(),
            transfers: IndexMap::new(),
            next_id: 1,
        }
    }
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        id
    }

    pub fn add_station(&mut self, pos: GeoPos) -> Id {
        let name = format!("Station {}", self.stations.len() + 1);
        self.add_named_station(name, pos)
    }

    pub fn add_named_station(&mut self, name: impl Into<String>, pos: GeoPos) -> Id {
        let id = self.alloc_id();
        self.stations.insert(id, Station::new(name, pos));
        id
    }

    pub fn add_waypoint(&mut self, pos: GeoPos) -> Id {
        let id = self.alloc_id();
        self.waypoints.insert(id, Waypoint::new(pos));
        id
    }

    pub fn add_line(&mut self, name: impl Into<String>, color: [u8; 3]) -> Id {
        let id = self.alloc_id();
        self.lines.insert(
            id,
            Line {
                name: name.into(),
                color,
                ..Line::default()
            },
        );
        id
    }

    pub fn node_pos(&self, node: Node) -> Option<GeoPos> {
        match node {
            Node::Station(id) => self.stations.get(&id).map(|s| s.pos),
            Node::Waypoint(id) => self.waypoints.get(&id).map(|w| w.pos),
        }
    }

    /// What every node is to the network, worked out in one pass over the
    /// lines: how many lines call at each station, which stations end a line,
    /// and which bends a line actually runs through.
    pub fn node_roles(&self) -> NodeRoles {
        let mut roles = NodeRoles::default();
        for line in self.lines.values() {
            for node in &line.nodes {
                match *node {
                    Node::Station(id) => NodeRoles::count(&mut roles.degree, id),
                    Node::Waypoint(id) => NodeRoles::mark(&mut roles.in_use, id),
                }
            }
            if line.closed {
                continue;
            }
            for end in [line.nodes.first(), line.nodes.last()] {
                if let Some(Node::Station(id)) = end {
                    NodeRoles::mark(&mut roles.terminals, *id);
                }
            }
        }
        roles
    }

    /// Put `node` on a line where the route bends least, so a line grows in
    /// the direction it is already heading instead of only from its end.
    /// A node already on the line is left alone.
    pub fn attach(&mut self, line_id: Id, node: Node) {
        let index = self.best_index(line_id, node);
        if let Some(line) = self.lines.get_mut(&line_id) {
            if !line.nodes.contains(&node) {
                let index = index.min(line.nodes.len());
                line.nodes.insert(index, node);
            }
        }
    }

    /// The gap in `line` that `node` fits best: the one where the turn at the
    /// node, plus how far the node sits from the line it is joining, is least.
    /// Appends when the line has fewer than two nodes to form an angle.
    pub fn best_index(&self, line_id: Id, node: Node) -> usize {
        let Some(line) = self.lines.get(&line_id) else {
            return 0;
        };
        let Some(pos) = self.node_pos(node) else {
            return line.nodes.len();
        };
        self.best_index_in(&line.nodes, pos)
    }

    /// The same fit, against any run of nodes: a line's route, or the sequence
    /// a walking interchange takes from one station to the other.
    pub fn best_index_in(&self, nodes: &[Node], pos: GeoPos) -> usize {
        if nodes.len() < 2 {
            return nodes.len();
        }

        let mut distances: Vec<f32> = nodes
            .iter()
            .filter_map(|other| self.node_pos(*other))
            .map(|other| rhumb_degrees(pos, other))
            .collect();
        if distances.is_empty() {
            return nodes.len();
        }
        distances.sort_by(f32::total_cmp);
        let upper = distances[((distances.len() - 1) / 2).min(9)].max(1e-4);

        let mut best_index = nodes.len();
        let mut best_score = 2.0;
        for index in 0..=nodes.len() {
            let found = |index: usize| nodes.get(index).and_then(|n| self.node_pos(*n));
            let (first, second, third, distance_used) = if index == 0 {
                let (Some(second), Some(third)) = (found(0), found(1)) else {
                    continue;
                };
                (pos, second, third, rhumb_degrees(pos, second))
            } else if index == nodes.len() {
                let (Some(first), Some(second)) = (found(index - 2), found(index - 1)) else {
                    continue;
                };
                (first, second, pos, rhumb_degrees(second, pos))
            } else {
                let (Some(first), Some(third)) = (found(index - 1), found(index)) else {
                    continue;
                };
                let used = rhumb_degrees(first, pos).min(rhumb_degrees(pos, third));
                (first, pos, third, used)
            };

            // The turn the line would make at the node, in degrees.
            let turn = (rhumb_bearing(first, second) - rhumb_bearing(second, third)).abs() % 360.0;
            let turn = if turn > 180.0 { 360.0 - turn } else { turn };
            // A node far off the existing stretch is a bad fit even when the
            // turn is shallow, so take the worse of angle and offset.
            let offset = rhumb_degrees(midpoint(first, third), second);
            let score = turn.max(offset) / 180.0 + distance_used / upper;

            if score.is_finite() && score < best_score {
                best_score = score;
                best_index = index;
            }
        }
        best_index
    }

    pub fn detach(&mut self, line_id: Id, node: Node) {
        if let Some(line) = self.lines.get_mut(&line_id) {
            line.nodes.retain(|other| *other != node);
        }
    }

    pub fn has_node(&self, line_id: Id, node: Node) -> bool {
        self.lines
            .get(&line_id)
            .map(|line| line.nodes.contains(&node))
            .unwrap_or(false)
    }

    pub fn toggle_node(&mut self, line_id: Id, node: Node) {
        if self.has_node(line_id, node) {
            self.detach(line_id, node);
        } else {
            self.attach(line_id, node);
        }
    }

    // ------------------------------------------------------------------ loops

    /// How far into the line a loop can be made at this node: the length of
    /// route that would be kept. Either where it starts, or far enough in that
    /// there is a stretch to close. `None` when the line cannot loop there.
    pub fn loop_index(&self, line_id: Id, node: Node) -> Option<usize> {
        let line = self.lines.get(&line_id)?;
        if line.nodes.len() < 3 {
            return None;
        }
        let index = line.nodes.iter().rposition(|other| *other == node)?;
        (index == 0 || index >= 2).then_some(index)
    }

    /// Loop the line at `node`: the route is cut there and joined back to where
    /// it started, so the line runs as a circle through that node.
    pub fn make_loop(&mut self, line_id: Id, node: Node) -> bool {
        let Some(index) = self.loop_index(line_id, node) else {
            return false;
        };
        let Some(line) = self.lines.get_mut(&line_id) else {
            return false;
        };
        if index > 0 {
            line.nodes.truncate(index + 1);
        }
        line.closed = true;
        true
    }

    // ----------------------------------------------------------------- groups

    pub fn add_group(&mut self, name: impl Into<String>) -> Id {
        let id = self.alloc_id();
        self.groups.insert(
            id,
            LineGroup {
                name: name.into(),
                ..LineGroup::default()
            },
        );
        id
    }

    /// Drop a group; its lines keep their place, unfiled.
    pub fn remove_group(&mut self, id: Id) {
        self.groups.shift_remove(&id);
        for line in self.lines.values_mut() {
            if line.group == Some(id) {
                line.group = None;
            }
        }
    }

    // -------------------------------------------------------------- transfers

    /// The stations a walk already joins this one to, nearest first.
    pub fn walk_partners(&self, station: Id) -> Vec<(Id, f32)> {
        let Some(pos) = self.stations.get(&station).map(|stop| stop.pos) else {
            return Vec::new();
        };
        let mut partners: Vec<(Id, f32)> = self
            .transfers
            .values()
            .filter_map(|transfer| {
                if transfer.a == station {
                    Some(transfer.b)
                } else if transfer.b == station {
                    Some(transfer.a)
                } else {
                    None
                }
            })
            .filter_map(|other| {
                self.stations
                    .get(&other)
                    .map(|stop| (other, distance_km(pos, stop.pos)))
            })
            .collect();
        partners.sort_by(|a, b| a.1.total_cmp(&b.1));
        partners
    }

    /// Stations a walk away that this one is not joined to yet, nearest first.
    pub fn stations_within_walk(&self, station: Id) -> Vec<(Id, f32)> {
        let Some(pos) = self.stations.get(&station).map(|stop| stop.pos) else {
            return Vec::new();
        };
        let mut found: Vec<(Id, f32)> = self
            .stations
            .iter()
            .filter(|(other, _)| **other != station)
            .filter(|(other, _)| self.transfer_between(station, **other).is_none())
            .filter_map(|(other, stop)| {
                let distance = distance_km(pos, stop.pos);
                (distance <= WALK_KM).then_some((*other, distance))
            })
            .collect();
        found.sort_by(|a, b| a.1.total_cmp(&b.1));
        found
    }

    /// The lines a walk from one station to another would open up: those that
    /// call at the other end but not at this one.
    pub fn lines_gained(&self, station: Id, other: Id) -> Vec<Id> {
        let mine = self.lines_at(station);
        self.lines_at(other)
            .into_iter()
            .filter(|line| !mine.contains(line))
            .collect()
    }

    /// Lines reached from a station on foot that do not call there.
    pub fn walked_lines(&self, station: Id) -> Vec<(Id, Id)> {
        let mut found: Vec<(Id, Id)> = Vec::new();
        for walk in self.transfers_at(station) {
            let Some(transfer) = self.transfers.get(&walk) else {
                continue;
            };
            let other = if transfer.a == station {
                transfer.b
            } else {
                transfer.a
            };
            for line in self.lines_gained(station, other) {
                if !found.iter().any(|(known, _)| *known == line) {
                    found.push((line, walk));
                }
            }
        }
        found
    }

    /// The lines that call at a station, in the order they were made.
    pub fn lines_at(&self, station: Id) -> Vec<Id> {
        self.lines
            .iter()
            .filter(|(_, line)| line.nodes.contains(&Node::Station(station)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Whether the line is in a group that is hidden, and so off the map.
    pub fn line_hidden(&self, line_id: Id) -> bool {
        self.lines
            .get(&line_id)
            .and_then(|line| line.group)
            .is_some_and(|group| self.groups.get(&group).is_some_and(|group| group.hidden))
    }

    /// Whether these two stations already walk between each other.
    pub fn transfer_between(&self, a: Id, b: Id) -> Option<Id> {
        self.transfers.iter().find_map(|(id, transfer)| {
            let pair = (transfer.a, transfer.b);
            (pair == (a, b) || pair == (b, a)).then_some(*id)
        })
    }

    /// The walking interchanges a station is an end of.
    pub fn transfers_at(&self, station: Id) -> Vec<Id> {
        self.transfers
            .iter()
            .filter(|(_, transfer)| transfer.a == station || transfer.b == station)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn add_transfer(&mut self, a: Id, b: Id) -> Option<Id> {
        if a == b || self.transfer_between(a, b).is_some() {
            return None;
        }
        let (pa, pb) = (self.stations.get(&a)?.pos, self.stations.get(&b)?.pos);
        if distance_km(pa, pb) > WALK_KM {
            return None;
        }
        let id = self.alloc_id();
        self.transfers.insert(
            id,
            Transfer {
                a,
                b,
                via: Vec::new(),
            },
        );
        Some(id)
    }

    pub fn remove_transfer(&mut self, id: Id) {
        self.transfers.shift_remove(&id);
    }

    /// The walk from one station to the other, its shaping waypoints in order.
    pub fn transfer_nodes(&self, id: Id) -> Option<Vec<Node>> {
        let transfer = self.transfers.get(&id)?;
        let mut nodes = vec![Node::Station(transfer.a)];
        nodes.extend(
            transfer
                .via
                .iter()
                .map(|waypoint| Node::Waypoint(*waypoint)),
        );
        nodes.push(Node::Station(transfer.b));
        Some(nodes)
    }

    /// Put a waypoint on a walk, where the walk bends least through it.
    pub fn attach_to_transfer(&mut self, transfer_id: Id, waypoint: Id) {
        let Some(transfer) = self.transfers.get(&transfer_id).cloned() else {
            return;
        };
        if transfer.via.contains(&waypoint) {
            return;
        }
        let node = Node::Waypoint(waypoint);
        let Some(pos) = self.node_pos(node) else {
            return;
        };
        let Some(path) = self.transfer_nodes(transfer_id) else {
            return;
        };
        // The path is the two stations with the waypoints between them, so the
        // slot the global index names is one less within `via`.
        let index = self.best_index_in(&path, pos).saturating_sub(1);
        if let Some(transfer) = self.transfers.get_mut(&transfer_id) {
            let index = index.min(transfer.via.len());
            transfer.via.insert(index, waypoint);
        }
    }

    pub fn detach_from_transfer(&mut self, transfer_id: Id, waypoint: Id) {
        if let Some(transfer) = self.transfers.get_mut(&transfer_id) {
            transfer.via.retain(|other| *other != waypoint);
        }
    }

    // ------------------------------------------------------------------ notes

    /// Route length in kilometres, closing the circle when the line runs as one.
    pub fn line_length_km(&self, id: Id) -> f32 {
        let Some(line) = self.lines.get(&id) else {
            return 0.0;
        };
        let positions: Vec<GeoPos> = line
            .nodes
            .iter()
            .filter_map(|node| self.node_pos(*node))
            .collect();
        let mut total = 0.0;
        for pair in positions.windows(2) {
            total += distance_km(pair[0], pair[1]);
        }
        if line.closed && positions.len() > 2 {
            total += distance_km(positions[positions.len() - 1], positions[0]);
        }
        total
    }

    /// Every line's route added together.
    pub fn total_length_km(&self) -> f32 {
        self.lines.keys().map(|id| self.line_length_km(*id)).sum()
    }

    /// Turn a station into a waypoint in place, keeping every line that used
    /// it attached to the new node.
    pub fn station_to_waypoint(&mut self, id: Id) -> Option<Id> {
        let pos = self.stations.get(&id)?.pos;
        self.stations.shift_remove(&id);
        // A walk needs two stations, and one of them has just become a bend.
        self.transfers
            .retain(|_, transfer| transfer.a != id && transfer.b != id);
        let waypoint = self.add_waypoint(pos);
        self.rewire(Node::Station(id), Node::Waypoint(waypoint));
        Some(waypoint)
    }

    /// The reverse of [`Self::station_to_waypoint`].
    pub fn waypoint_to_station(&mut self, id: Id) -> Option<Id> {
        let pos = self.waypoints.get(&id)?.pos;
        self.waypoints.shift_remove(&id);
        let station = self.add_station(pos);
        self.rewire(Node::Waypoint(id), Node::Station(station));
        Some(station)
    }

    fn rewire(&mut self, from: Node, to: Node) {
        for line in self.lines.values_mut() {
            for node in &mut line.nodes {
                if *node == from {
                    *node = to;
                }
            }
        }
    }

    pub fn remove_line(&mut self, id: Id) {
        self.lines.shift_remove(&id);
    }

    pub fn remove_station(&mut self, id: Id) {
        self.stations.shift_remove(&id);
        for line in self.lines.values_mut() {
            line.nodes
                .retain(|node| !matches!(node, Node::Station(station) if *station == id));
        }
        self.transfers
            .retain(|_, transfer| transfer.a != id && transfer.b != id);
    }

    pub fn remove_waypoint(&mut self, id: Id) {
        self.waypoints.shift_remove(&id);
        for line in self.lines.values_mut() {
            line.nodes
                .retain(|node| !matches!(node, Node::Waypoint(waypoint) if *waypoint == id));
        }
        for transfer in self.transfers.values_mut() {
            transfer.via.retain(|waypoint| *waypoint != id);
        }
    }

    pub fn bounds(&self) -> Option<(GeoPos, GeoPos)> {
        let mut positions = self
            .stations
            .values()
            .map(|station| station.pos)
            .chain(self.waypoints.values().map(|waypoint| waypoint.pos));
        let first = positions.next()?;
        let mut min = first;
        let mut max = first;
        for pos in positions {
            min.lon = min.lon.min(pos.lon);
            min.lat = min.lat.min(pos.lat);
            max.lon = max.lon.max(pos.lon);
            max.lat = max.lat.max(pos.lat);
        }
        Some((min, max))
    }

    /// Drop dangling references, clamp out-of-range data, and renumber ids.
    /// Safe to call on any imported document.
    pub fn sanitize(&mut self) {
        for station in self.stations.values_mut() {
            station.pos = station.pos.sanitized();
            if station.name.trim().is_empty() {
                station.name = "Station".to_owned();
            }
        }
        for waypoint in self.waypoints.values_mut() {
            waypoint.pos = waypoint.pos.sanitized();
        }
        if self.name.trim().is_empty() {
            self.name = "Untitled".to_owned();
        }

        let stations = &self.stations;
        let waypoints = &self.waypoints;
        for group in self.groups.values_mut() {
            if group.name.trim().is_empty() {
                group.name = "Group".to_owned();
            }
        }
        let groups = &self.groups;
        for line in self.lines.values_mut() {
            line.nodes.retain(|node| match node {
                Node::Station(id) => stations.contains_key(id),
                Node::Waypoint(id) => waypoints.contains_key(id),
            });
            if line.group.is_some_and(|group| !groups.contains_key(&group)) {
                line.group = None;
            }
            line.width = if line.width.is_finite() {
                line.width.clamp(1.0, 24.0)
            } else {
                5.0
            };
            if line.name.trim().is_empty() {
                line.name = "Line".to_owned();
            }
        }

        self.transfers.retain(|_, transfer| {
            transfer.a != transfer.b
                && stations.contains_key(&transfer.a)
                && stations.contains_key(&transfer.b)
        });
        for transfer in self.transfers.values_mut() {
            let mut seen = std::collections::HashSet::new();
            transfer
                .via
                .retain(|id| waypoints.contains_key(id) && seen.insert(*id));
        }

        let max_id = self
            .stations
            .keys()
            .chain(self.waypoints.keys())
            .chain(self.lines.keys())
            .chain(self.groups.keys())
            .chain(self.transfers.keys())
            .copied()
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max_id.saturating_add(1)).max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_is_written_as_metrodreamin_writes_it() {
        assert_eq!(Mode::HighSpeed.key(), "HSR");
        assert_eq!(Mode::LongDistance.key(), "MLDISTANCE");
        assert_eq!(Mode::from_key(" ferry "), Some(Mode::Ferry));
        assert_eq!(Mode::from_key("SPACESHIP"), None);

        let line = Line {
            mode: Mode::Brt,
            ..Line::default()
        };
        let json = serde_json::to_string(&line).unwrap();
        assert!(json.contains("\"mode\":\"BRT\""));
        assert_eq!(serde_json::from_str::<Line>(&json).unwrap().mode, Mode::Brt);

        // A mode this build does not know, and an old map with none at all,
        // both read as metro rather than losing the line.
        let unknown = json.replace("\"BRT\"", "\"HYPERLOOP\"");
        assert_eq!(
            serde_json::from_str::<Line>(&unknown).unwrap().mode,
            Mode::Rapid
        );
        let none = json.replace("\"mode\":\"BRT\",", "");
        assert_eq!(
            serde_json::from_str::<Line>(&none).unwrap().mode,
            Mode::Rapid
        );
    }

    #[test]
    fn a_pattern_is_written_plainly_and_an_unknown_one_runs_as_the_mode() {
        let line = Line {
            pattern: Pattern::Dashed,
            ..Line::default()
        };
        let json = serde_json::to_string(&line).unwrap();
        assert!(json.contains("\"pattern\":\"DASHED\""));
        assert_eq!(
            serde_json::from_str::<Line>(&json).unwrap().pattern,
            Pattern::Dashed
        );

        // A pattern this build does not know, and an old map with none at all,
        // both read as automatic, which strokes the line as its mode runs.
        let unknown = json.replace("\"DASHED\"", "\"WAVY\"");
        assert_eq!(
            serde_json::from_str::<Line>(&unknown).unwrap().pattern,
            Pattern::Auto
        );
        let none = json.replace("\"pattern\":\"DASHED\",", "");
        assert_eq!(
            serde_json::from_str::<Line>(&none).unwrap().pattern,
            Pattern::Auto
        );

        // Dots were a pattern of their own until the circle covered them, and
        // a map that carries one still reads as that circle.
        assert_eq!(Pattern::from_key("DOTTED"), Some(Pattern::Circle));

        // A shape is a pattern too, written as MetroDreamin' names it.
        let shaped = Line {
            pattern: Pattern::Heart,
            ..Line::default()
        };
        let json = serde_json::to_string(&shaped).unwrap();
        assert!(json.contains("\"pattern\":\"HEART\""));
        assert_eq!(
            serde_json::from_str::<Line>(&json).unwrap().pattern,
            Pattern::Heart
        );
    }

    #[test]
    fn nodes_attach_once_and_toggle_off_again() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(0.0, 0.0));
        let b = project.add_station(GeoPos::new(10.0, 0.0));
        let wp = project.add_waypoint(GeoPos::new(5.0, 1.0));
        let line = project.add_line("Line", [1, 2, 3]);

        project.attach(line, Node::Station(a));
        project.attach(line, Node::Station(a));
        project.attach(line, Node::Waypoint(wp));
        project.attach(line, Node::Station(b));
        assert_eq!(
            project.lines.get(&line).unwrap().nodes,
            vec![Node::Station(a), Node::Waypoint(wp), Node::Station(b)]
        );
        assert!(project.has_node(line, Node::Waypoint(wp)));

        project.toggle_node(line, Node::Waypoint(wp));
        assert!(!project.has_node(line, Node::Waypoint(wp)));
        assert!(project.waypoints.contains_key(&wp));

        project.toggle_node(line, Node::Waypoint(wp));
        assert!(project.has_node(line, Node::Waypoint(wp)));
    }

    #[test]
    fn a_line_bends_around_a_node_that_sits_between_its_ends() {
        let mut project = Project::new("test");
        let west = project.add_station(GeoPos::new(-1.0, 51.5));
        let east = project.add_station(GeoPos::new(4.0, 51.5));
        let line = project.add_line("Line", [1, 2, 3]);
        project.attach(line, Node::Station(west));
        project.attach(line, Node::Station(east));

        // Just off the straight run between the two, so it belongs in the gap.
        let mid = project.add_station(GeoPos::new(1.5, 51.52));
        project.attach(line, Node::Station(mid));
        assert_eq!(
            project.lines.get(&line).unwrap().nodes,
            vec![Node::Station(west), Node::Station(mid), Node::Station(east)]
        );

        // Well past the end of the run, so it joins at the end.
        let past = project.add_station(GeoPos::new(9.0, 51.5));
        project.attach(line, Node::Station(past));
        assert_eq!(
            project.lines.get(&line).unwrap().nodes,
            vec![
                Node::Station(west),
                Node::Station(mid),
                Node::Station(east),
                Node::Station(past)
            ]
        );
    }

    #[test]
    fn switching_a_node_keeps_its_lines_attached() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(0.0, 0.0));
        let b = project.add_station(GeoPos::new(10.0, 0.0));
        let line = project.add_line("Line", [1, 2, 3]);
        project.attach(line, Node::Station(a));
        project.attach(line, Node::Station(b));

        let waypoint = project.station_to_waypoint(a).unwrap();
        assert!(project.waypoints.contains_key(&waypoint));
        assert_eq!(
            project.lines.get(&line).unwrap().nodes,
            vec![Node::Waypoint(waypoint), Node::Station(b)]
        );

        let station = project.waypoint_to_station(waypoint).unwrap();
        assert!(project.stations.contains_key(&station));
        assert_eq!(
            project.lines.get(&line).unwrap().nodes,
            vec![Node::Station(station), Node::Station(b)]
        );
    }

    #[test]
    fn deleting_a_station_detaches_it_and_keeps_waypoints() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(0.0, 0.0));
        let b = project.add_station(GeoPos::new(10.0, 0.0));
        let line = project.add_line("Line", [1, 2, 3]);
        let wp = project.add_waypoint(GeoPos::new(5.0, 0.0));
        project.lines.get_mut(&line).unwrap().nodes =
            vec![Node::Station(a), Node::Waypoint(wp), Node::Station(b)];

        project.remove_station(b);
        let nodes = &project.lines.get(&line).unwrap().nodes;
        assert_eq!(nodes, &vec![Node::Station(a), Node::Waypoint(wp)]);
        assert!(project.waypoints.contains_key(&wp));

        project.remove_line(line);
        assert!(project.waypoints.contains_key(&wp));
    }

    #[test]
    fn sanitize_repairs_bad_imports() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(200.0, 100.0));
        let line = project.add_line("", [1, 2, 3]);
        project.lines.get_mut(&line).unwrap().nodes = vec![Node::Station(a), Node::Station(999)];
        project.sanitize();
        assert_eq!(
            project.lines.get(&line).unwrap().nodes,
            vec![Node::Station(a)]
        );
        assert_eq!(
            project.stations.get(&a).unwrap().pos,
            GeoPos::new(180.0, 89.9)
        );
    }

    #[test]
    fn json_round_trip_preserves_everything() {
        let mut project = Project::new("test");
        let a = project.add_named_station("A", GeoPos::new(-0.12, 51.5));
        let b = project.add_named_station("B", GeoPos::new(2.35, 48.85));
        let wp = project.add_waypoint(GeoPos::new(1.4, 50.1));
        let line = project.add_line("Line", [1, 2, 3]);
        project.lines.get_mut(&line).unwrap().nodes =
            vec![Node::Station(a), Node::Waypoint(wp), Node::Station(b)];
        let group = project.add_group("RER");
        project.lines.get_mut(&line).unwrap().group = Some(group);
        project.lines.get_mut(&line).unwrap().closed = true;
        project.lines.get_mut(&line).unwrap().turnback = true;
        project.sanitize();

        // A walk between two of the stations, shaped by the waypoint.
        let near = project.add_station(GeoPos::new(-0.121, 51.501));
        let walk = project.add_transfer(a, near).unwrap();
        project.attach_to_transfer(walk, wp);

        let json = serde_json::to_string_pretty(&project).unwrap();
        let loaded: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(project, loaded);
        assert!(json.contains("waypoint"));
        assert!(json.contains("turnback"));
        assert!(json.contains("RER"));
    }

    #[test]
    fn the_ends_of_an_open_line_are_termini_but_a_loop_has_none() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(0.0, 0.0));
        let b = project.add_station(GeoPos::new(0.01, 0.0));
        let c = project.add_station(GeoPos::new(0.02, 0.0));
        let line = project.add_line("Line", [1, 2, 3]);
        project.lines.get_mut(&line).unwrap().nodes =
            vec![Node::Station(a), Node::Station(b), Node::Station(c)];
        let roles = project.node_roles();
        assert!(roles.is_terminal(a) && roles.is_terminal(c));
        assert!(!roles.is_terminal(b));

        // A waypoint at an end is not a terminus; only stations draw one.
        let wp = project.add_waypoint(GeoPos::new(0.03, 0.0));
        project
            .lines
            .get_mut(&line)
            .unwrap()
            .nodes
            .push(Node::Waypoint(wp));
        assert!(project.node_roles().is_terminal(a));
        assert!(!project.node_roles().is_terminal(c));

        project.lines.get_mut(&line).unwrap().closed = true;
        assert!(!project.node_roles().is_terminal(b));
    }

    #[test]
    fn a_line_loops_at_the_node_it_is_cut_at() {
        let mut project = Project::new("test");
        let ids: Vec<Id> = (0..4)
            .map(|step| project.add_station(GeoPos::new(step as f32 * 0.01, 0.0)))
            .collect();
        let line = project.add_line("Line", [1, 2, 3]);
        project.lines.get_mut(&line).unwrap().nodes =
            ids.iter().map(|id| Node::Station(*id)).collect();

        assert!(project.make_loop(line, Node::Station(ids[2])));
        let line_ref = project.lines.get(&line).unwrap();
        assert_eq!(line_ref.nodes.len(), 3);
        assert!(line_ref.closed);
        assert_eq!(line_ref.nodes.last(), Some(&Node::Station(ids[2])));

        // Down at the start of a shaped route, the loop closes where it stands.
        let mut first = Project::new("test");
        let ring: Vec<Id> = (0..3)
            .map(|step| first.add_station(GeoPos::new(step as f32 * 0.01, 0.0)))
            .collect();
        let line = first.add_line("Line", [1, 2, 3]);
        first.lines.get_mut(&line).unwrap().nodes =
            ring.iter().map(|id| Node::Station(*id)).collect();
        assert_eq!(first.loop_index(line, Node::Station(ring[0])), Some(0));
        assert!(first.make_loop(line, Node::Station(ring[0])));
        assert_eq!(first.lines.get(&line).unwrap().nodes.len(), 3);
        assert!(first.lines.get(&line).unwrap().closed);
        // One stop in there is no route behind it to close.
        assert_eq!(first.loop_index(line, Node::Station(ring[1])), None);
        assert!(!first.make_loop(line, Node::Station(ring[1])));

        // A line of two has nothing to loop.
        let mut short = Project::new("test");
        let a = short.add_station(GeoPos::new(0.0, 0.0));
        let b = short.add_station(GeoPos::new(0.01, 0.0));
        let line = short.add_line("Line", [1, 2, 3]);
        short.lines.get_mut(&line).unwrap().nodes = vec![Node::Station(a), Node::Station(b)];
        assert!(!short.make_loop(line, Node::Station(b)));
        assert!(!short.lines.get(&line).unwrap().closed);
    }

    #[test]
    fn walking_interchanges_need_two_stations_a_walk_apart() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(0.0, 51.5));
        let near = project.add_station(GeoPos::new(0.005, 51.5));
        let far = project.add_station(GeoPos::new(0.05, 51.5));
        assert!(distance_km(GeoPos::new(0.0, 51.5), GeoPos::new(0.005, 51.5)) <= WALK_KM);

        assert!(project.add_transfer(a, far).is_none());
        assert!(project.add_transfer(a, a).is_none());
        let walk = project.add_transfer(a, near).unwrap();
        // The same pair the other way round is the same walk.
        assert!(project.add_transfer(near, a).is_none());
        assert_eq!(project.transfer_between(near, a), Some(walk));
        assert_eq!(project.transfers_at(a), vec![walk]);
        assert_eq!(
            project.walk_partners(a),
            vec![(
                near,
                distance_km(GeoPos::new(0.0, 51.5), GeoPos::new(0.005, 51.5))
            )]
        );
        // Nothing to change to yet: neither station carries a line.
        assert!(project.lines_gained(a, near).is_empty());

        // Losing a station takes its walk with it.
        project.remove_station(near);
        assert!(project.transfers.is_empty());
    }

    #[test]
    fn a_station_without_an_elevation_loads_at_grade() {
        // Files written before stations had a height carry no such field.
        let station: Station =
            serde_json::from_str(r#"{"name":"A","pos":{"lon":1.0,"lat":2.0}}"#).unwrap();
        assert_eq!(station.elevation, Elevation::At);
        assert_eq!(station, Station::new("A", GeoPos::new(1.0, 2.0)));

        let mut raised = station.clone();
        raised.elevation = Elevation::Above;
        let text = serde_json::to_string(&raised).unwrap();
        assert!(text.contains(r#""elevation":"above""#), "{text}");
        assert_eq!(serde_json::from_str::<Station>(&text).unwrap(), raised);

        assert_eq!(Elevation::Above.label(), "Above grade");
        assert_eq!(Elevation::ALL[1], Elevation::At);
    }

    #[test]
    fn a_group_starts_open_and_can_be_hidden_off_the_map() {
        let mut project = Project::new("test");
        let group = project.add_group("Ring");
        let line = project.add_line("One", [1, 2, 3]);
        project.lines.get_mut(&line).unwrap().group = Some(group);

        assert!(project.groups[&group].open);
        assert!(!project.line_hidden(line));
        project.groups.get_mut(&group).unwrap().hidden = true;
        assert!(project.line_hidden(line));

        // Dropping the group leaves its lines on the map, unfiled.
        project.remove_group(group);
        assert!(!project.line_hidden(line));
        assert_eq!(project.lines[&line].group, None);
    }

    #[test]
    fn waypoints_shape_a_walk_in_route_order() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(0.0, 51.5));
        let b = project.add_station(GeoPos::new(0.006, 51.5));
        let walk = project.add_transfer(a, b).unwrap();
        let beyond = project.add_waypoint(GeoPos::new(0.0045, 51.5));
        let middle = project.add_waypoint(GeoPos::new(0.003, 51.501));

        // Attached out of order, the walk still runs them in route order.
        project.attach_to_transfer(walk, beyond);
        project.attach_to_transfer(walk, middle);
        assert_eq!(
            project.transfers.get(&walk).unwrap().via,
            vec![middle, beyond]
        );
        assert_eq!(
            project.transfer_nodes(walk).unwrap(),
            vec![
                Node::Station(a),
                Node::Waypoint(middle),
                Node::Waypoint(beyond),
                Node::Station(b)
            ]
        );

        // Attaching twice is attaching once, and deleting the waypoint clears it.
        project.attach_to_transfer(walk, middle);
        assert_eq!(project.transfers.get(&walk).unwrap().via.len(), 2);
        project.remove_waypoint(middle);
        assert_eq!(project.transfers.get(&walk).unwrap().via, vec![beyond]);
    }

    #[test]
    fn length_follows_the_route_and_closes_the_loop() {
        let mut project = Project::new("test");
        let a = project.add_station(GeoPos::new(0.0, 0.0));
        let b = project.add_station(GeoPos::new(0.01, 0.0));
        let c = project.add_station(GeoPos::new(0.01, 0.01));
        let line = project.add_line("Line", [1, 2, 3]);
        project.lines.get_mut(&line).unwrap().nodes =
            vec![Node::Station(a), Node::Station(b), Node::Station(c)];
        let open = project.line_length_km(line);
        assert!((open - 2.223).abs() < 0.02, "{open}");

        project.lines.get_mut(&line).unwrap().closed = true;
        let closed = project.line_length_km(line);
        // Back along the diagonal to where it started.
        assert!((closed - (open + 1.571)).abs() < 0.02, "{closed}");
        assert!((project.total_length_km() - closed).abs() < 1e-6);
    }

    #[test]
    fn groups_file_lines_and_fold_away_without_them() {
        let mut project = Project::new("test");
        let line = project.add_line("Line", [1, 2, 3]);
        let group = project.add_group("Tram");
        project.lines.get_mut(&line).unwrap().group = Some(group);
        assert_eq!(project.groups.get(&group).unwrap().name, "Tram");

        project.remove_group(group);
        assert_eq!(project.lines.get(&line).unwrap().group, None);

        // A file that names a group that is not there loses the reference.
        project.lines.get_mut(&line).unwrap().group = Some(999);
        project.sanitize();
        assert_eq!(project.lines.get(&line).unwrap().group, None);
    }
}
