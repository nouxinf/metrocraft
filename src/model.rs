//! The map document: stations, lines, and the waypoints that shape them.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Station {
    pub name: String,
    pub pos: GeoPos,
}

impl Station {
    pub fn new(name: impl Into<String>, pos: GeoPos) -> Self {
        Self {
            name: name.into(),
            pos: pos.sanitized(),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Stops and shaping waypoints, in travel order.
    pub nodes: Vec<Node>,
    /// Whether the last node connects back to the first.
    pub closed: bool,
}

impl Default for Line {
    fn default() -> Self {
        Self {
            name: "Line".to_owned(),
            color: [235, 87, 87],
            width: 5.0,
            nodes: Vec::new(),
            closed: false,
        }
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
    pub next_id: Id,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "Untitled".to_owned(),
            stations: IndexMap::new(),
            waypoints: IndexMap::new(),
            lines: IndexMap::new(),
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

    pub fn station_degree(&self) -> std::collections::HashMap<Id, u32> {
        let mut degree = std::collections::HashMap::new();
        for line in self.lines.values() {
            for node in &line.nodes {
                if let Node::Station(id) = node {
                    *degree.entry(*id).or_insert(0) += 1;
                }
            }
        }
        degree
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
        if line.nodes.len() < 2 {
            return line.nodes.len();
        }

        let mut distances: Vec<f32> = line
            .nodes
            .iter()
            .filter_map(|other| self.node_pos(*other))
            .map(|other| rhumb_degrees(pos, other))
            .collect();
        if distances.is_empty() {
            return line.nodes.len();
        }
        distances.sort_by(f32::total_cmp);
        let upper = distances[((distances.len() - 1) / 2).min(9)].max(1e-4);

        let mut best_index = line.nodes.len();
        let mut best_score = 2.0;
        for index in 0..=line.nodes.len() {
            let found = |index: usize| line.nodes.get(index).and_then(|n| self.node_pos(*n));
            let (first, second, third, distance_used) = if index == 0 {
                let (Some(second), Some(third)) = (found(0), found(1)) else {
                    continue;
                };
                (pos, second, third, rhumb_degrees(pos, second))
            } else if index == line.nodes.len() {
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

    /// Turn a station into a waypoint in place, keeping every line that used
    /// it attached to the new node.
    pub fn station_to_waypoint(&mut self, id: Id) -> Option<Id> {
        let pos = self.stations.get(&id)?.pos;
        self.stations.shift_remove(&id);
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
    }

    pub fn remove_waypoint(&mut self, id: Id) {
        self.waypoints.shift_remove(&id);
        for line in self.lines.values_mut() {
            line.nodes
                .retain(|node| !matches!(node, Node::Waypoint(waypoint) if *waypoint == id));
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
        for line in self.lines.values_mut() {
            line.nodes.retain(|node| match node {
                Node::Station(id) => stations.contains_key(id),
                Node::Waypoint(id) => waypoints.contains_key(id),
            });
            line.width = if line.width.is_finite() {
                line.width.clamp(1.0, 24.0)
            } else {
                5.0
            };
            if line.name.trim().is_empty() {
                line.name = "Line".to_owned();
            }
        }

        let max_id = self
            .stations
            .keys()
            .chain(self.waypoints.keys())
            .chain(self.lines.keys())
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
        project.sanitize();

        let json = serde_json::to_string_pretty(&project).unwrap();
        let loaded: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(project, loaded);
        assert!(json.contains("waypoint"));
    }
}
