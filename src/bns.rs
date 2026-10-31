//! Reading a Brand New Subway saved game into a Metrocraft project.
//!
//! A saved game is one JSON file holding two arrays: `stations`, every point on
//! the map, and `lines`, each naming the stations it stops at (`stations`) and
//! the points its route runs through in order (`draw_map`). The route is the
//! longer of the two: it passes through points the line does not stop at,
//! which is how the game draws a bend without having waypoints of its own.
//!
//! A game ships with every line it could offer, most of them never built, so a
//! line with no stations is one nobody ran and is left out.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::model::{GeoPos, Node, Project, Transfer};

/// A line whose colour the file does not give.
const FALLBACK_COLOR: [u8; 3] = [128, 129, 131];

/// A line as the game drew it: where it stops, and where it runs.
struct Route {
    name: String,
    color: [u8; 3],
    stops: Vec<u64>,
    path: Vec<u64>,
}

/// Turn a saved game into a project named `name`.
pub fn convert(name: &str, text: &str) -> Result<Project, String> {
    let game: Value = serde_json::from_str(text)
        .map_err(|err| format!("That is not a Brand New Subway game: {err}"))?;
    let points = points(&game);
    if points.is_empty() {
        return Err("That game has no stations.".to_owned());
    }
    let routes = routes(&game);
    if routes.is_empty() {
        return Err("That game has no lines with stations.".to_owned());
    }

    // A point any imported line stops at is a station, even where another line
    // runs past it: the map has one mark for a place, not one per line.
    let mut stopped: HashSet<u64> = HashSet::new();
    for route in &routes {
        stopped.extend(route.stops.iter().copied());
    }

    let mut project = Project::new(name);
    let mut nodes: HashMap<u64, Node> = HashMap::new();
    for route in &routes {
        let mut path: Vec<Node> = Vec::new();
        for point in &route.path {
            let Some((pos, label)) = points.get(point) else {
                continue;
            };
            let node = match nodes.get(point) {
                Some(node) => *node,
                None => {
                    let node = if stopped.contains(point) {
                        Node::Station(project.add_named_station(label.clone(), *pos))
                    } else {
                        Node::Waypoint(project.add_waypoint(*pos))
                    };
                    nodes.insert(*point, node);
                    node
                }
            };
            // A route may name a point twice in a row; one node is one node.
            if path.last() != Some(&node) {
                path.push(node);
            }
        }
        if path.len() < 2 {
            continue;
        }
        // A route that comes back to where it started is a loop, with the
        // repeated node dropped in favour of closing the line.
        let closed = path.len() > 2 && path.first() == path.last();
        if closed {
            path.pop();
        }
        // A route that visits a node twice doubles back on itself.
        let mut seen = HashSet::new();
        let turnback = path.iter().any(|node| !seen.insert(*node));

        let id = project.add_line(route.name.clone(), route.color);
        if let Some(line) = project.lines.get_mut(&id) {
            line.nodes = path;
            line.closed = closed;
            line.turnback = turnback;
        }
    }

    if project.lines.is_empty() {
        return Err("That game has no lines with stations.".to_owned());
    }

    // The walks between stations the game links up are the map's interchanges.
    for pair in array(&game, "transfers") {
        let (Some(from), Some(to)) = (pair.get("s").and_then(whole), pair.get("e").and_then(whole))
        else {
            continue;
        };
        let (Some(Node::Station(a)), Some(Node::Station(b))) = (nodes.get(&from), nodes.get(&to))
        else {
            continue;
        };
        if a == b {
            continue;
        }
        let id = project.alloc_id();
        project.transfers.insert(
            id,
            Transfer {
                a: *a,
                b: *b,
                via: Vec::new(),
            },
        );
    }

    project.sanitize();
    Ok(project)
}

/// Every point on the map, by the id a line names it with.
fn points(game: &Value) -> HashMap<u64, (GeoPos, String)> {
    let mut points = HashMap::new();
    for (index, station) in array(game, "stations").iter().enumerate() {
        let id = station.get("id").and_then(whole).unwrap_or(index as u64);
        let (Some(lat), Some(lng)) = (
            station.get("lat").and_then(fraction),
            station.get("lng").and_then(fraction),
        ) else {
            continue;
        };
        let name = station
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("Station")
            .to_owned();
        points.insert(id, (GeoPos::new(lng, lat), name));
    }
    points
}

/// The lines the game actually runs, in the order it lists them.
fn routes(game: &Value) -> Vec<Route> {
    let mut routes = Vec::new();
    for line in array(game, "lines") {
        let stops = ids(line, "stations");
        // Never built, so there is nothing to import.
        if stops.is_empty() {
            continue;
        }
        let mut path = ids(line, "draw_map");
        if path.is_empty() {
            path = stops.clone();
        }
        let name = line
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "Line".to_owned());
        let color = line
            .get("color_bg")
            .and_then(Value::as_str)
            .and_then(hex_color)
            .unwrap_or(FALLBACK_COLOR);
        routes.push(Route {
            name,
            color,
            stops,
            path,
        });
    }
    routes
}

fn array<'a>(game: &'a Value, key: &str) -> &'a [Value] {
    game.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// The point ids one field of a line names.
fn ids(line: &Value, key: &str) -> Vec<u64> {
    array(line, key).iter().filter_map(whole).collect()
}

/// A point id, which the game writes as a number and sometimes as its text.
fn whole(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().filter(|id| *id >= 0.0).map(|id| id as u64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn fraction(value: &Value) -> Option<f32> {
    match value {
        Value::Number(number) => number.as_f64().map(|number| number as f32),
        Value::String(text) => text.trim().parse::<f32>().ok(),
        _ => None,
    }
}

fn hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        serde_json::json!({
            "stations": [
                {"id": 0, "name": "Alpha", "lat": 40.70, "lng": -74.00},
                {"id": 1, "name": "Bravo", "lat": 40.71, "lng": -74.01},
                {"id": 2, "name": "Charlie", "lat": 40.72, "lng": -74.02},
                {"id": 3, "name": "Bend", "lat": 40.715, "lng": -74.015},
                {"id": 4, "name": "", "lat": 40.73, "lng": -74.03}
            ],
            "lines": [
                // Stops at 0 and 2, runs past 1, which the C stops at.
                {"id": 0, "name": "A", "color_bg": "#0039A6", "color_text": "#FFFFFF",
                 "stations": [0, 2], "draw_map": [0, 1, 2]},
                // Its own route bends at 3, which nothing stops at.
                {"id": 1, "name": "C", "color_bg": "#FF6319",
                 "stations": [1], "draw_map": [1, 3]},
                // Never built.
                {"id": 2, "name": "T", "color_bg": "#00ADD0", "stations": [], "draw_map": []},
                {"id": 3, "name": "N/A", "color_bg": "#808183", "stations": [], "draw_map": []}
            ],
            // A walk between two stops, one id written as text, and one pair
            // that names a point the map does not have.
            "transfers": [{"s": 0, "e": "2"}, {"s": 1, "e": 99}]
        })
    }

    #[test]
    fn a_game_comes_across_with_its_routes() {
        let project = convert("Test", &sample().to_string()).unwrap();
        assert_eq!(project.name, "Test");
        assert_eq!(project.lines.len(), 2);
        // Nothing ran on T or the unbuilt N/A, so neither is in the map.
        assert!(project.lines.values().all(|line| line.name != "T"));
        assert!(project.lines.values().all(|line| line.name != "N/A"));

        let a = project
            .lines
            .values()
            .find(|line| line.name == "A")
            .unwrap();
        assert_eq!(a.nodes.len(), 3);
        assert_eq!(a.color, [0x00, 0x39, 0xa6]);
        // A pass-through another line stops at is that line's station.
        assert!(a.nodes.iter().all(|node| matches!(node, Node::Station(_))));
        assert!(!a.closed && !a.turnback);

        let c = project
            .lines
            .values()
            .find(|line| line.name == "C")
            .unwrap();
        assert_eq!(c.nodes.len(), 2);
        assert_eq!(c.color, [0xff, 0x63, 0x19]);
        // A point nothing stops at is a waypoint the line bends around.
        assert!(matches!(c.nodes[0], Node::Station(_)));
        assert!(matches!(c.nodes[1], Node::Waypoint(_)));
        assert_eq!(project.waypoints.len(), 1);

        // Alpha, Bravo and Charlie: the unnamed point is never used.
        assert_eq!(project.stations.len(), 3);

        // The walk the game drew, and only the pair it can name.
        assert_eq!(project.transfers.len(), 1);
        let walk = project.transfers.values().next().unwrap();
        assert!(walk.via.is_empty());
        let linked = [
            project.stations[&walk.a].name.as_str(),
            project.stations[&walk.b].name.as_str(),
        ];
        assert_eq!(linked, ["Alpha", "Charlie"]);
    }

    #[test]
    fn a_route_that_returns_home_is_a_loop() {
        let game = serde_json::json!({
            "stations": [
                {"id": 0, "name": "One", "lat": 1.0, "lng": 1.0},
                {"id": 1, "name": "Two", "lat": 1.1, "lng": 1.1},
                {"id": 2, "name": "Three", "lat": 1.2, "lng": 1.2}
            ],
            "lines": [
                {"name": "Ring", "color_bg": "#6CBE45", "stations": [0, 1, 2],
                 "draw_map": [0, 1, 2, 0]}
            ]
        });
        let project = convert("Loop", &game.to_string()).unwrap();
        let line = project.lines.values().next().unwrap();
        assert!(line.closed);
        assert_eq!(line.nodes.len(), 3);
        assert_eq!(
            line.nodes.first(),
            line.nodes.last().map(|_| &line.nodes[0])
        );
    }

    #[test]
    fn a_game_with_nothing_on_it_is_refused() {
        let game = serde_json::json!({"stations": [], "lines": []});
        assert!(convert("Empty", &game.to_string()).is_err());
        let built = serde_json::json!({
            "stations": [{"id": 0, "name": "One", "lat": 1.0, "lng": 1.0}],
            "lines": [{"name": "T", "stations": [], "draw_map": []}]
        });
        assert!(convert("Unbuilt", &built.to_string()).is_err());
        assert!(convert("Broken", "{not json}").is_err());
    }
}
