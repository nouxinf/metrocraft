//! Reading a map published on MetroDreamin' into a Metrocraft project.
//!
//! A published map is a public Firestore document under `systems/{id}`, where
//! `{id}` is the base64 of `userId|systemNumber` — the same string a
//! `metrodreamin.com/view/…` link carries. The map itself is not in that
//! document: it lives in the `partitions` subcollection, one document per
//! partition, because a large network does not fit in a single Firestore
//! document. Small maps simply have one partition, so both shapes are read the
//! same way here.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::model::{Elevation, GeoPos, Id, Mode, Node, Pattern, Project, Transfer};

/// The published database every map link points at.
const ON_FIRESTORE: &str =
    "https://firestore.googleapis.com/v1/projects/metrodreamin/databases/(default)/documents";
/// MetroDreamin' ships this key in its own page bundle: it only grants what
/// their security rules allow, which for a published map is reading it.
const KEY: &str = "AIzaSyBIMlulR8OTOoF-57DHty1NuXM0kqVoL5c";
const FALLBACK_COLOR: [u8; 3] = [235, 87, 87];

/// An import in flight. The result arrives on the channel once.
pub struct Import {
    pub receiver: std::sync::mpsc::Receiver<Result<Project, String>>,
}

/// A map an account has, as a scan of the account reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub id: String,
    pub title: String,
}

/// A scan of an account in flight: every map it has, found in one answer.
pub struct Listing {
    pub receiver: std::sync::mpsc::Receiver<Result<Vec<Listed>, String>>,
}

type Done<T> = Box<dyn FnOnce(Result<T, String>) + Send>;

/// Start reading the map a link or a bare system id names.
pub fn start(input: &str, repaint: egui::Context) -> Result<Import, String> {
    let system_id = system_id(input)?;
    let (sender, receiver) = std::sync::mpsc::channel();
    read(
        system_id,
        Box::new(move |result| {
            let _ = sender.send(result);
            repaint.request_repaint();
        }),
    );
    Ok(Import { receiver })
}

/// The system id a link carries: `/view/{id}`, `/edit/{id}`, or the id alone.
pub fn system_id(input: &str) -> Result<String, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("Paste a MetroDreamin' map link.".to_owned());
    }
    let text = text
        .split(['?', '#'])
        .next()
        .unwrap_or(text)
        .trim_end_matches('/');
    let mut text = text;
    for prefix in ["https://", "http://"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest;
            break;
        }
    }
    let mut parts: Vec<&str> = text.split('/').filter(|part| !part.is_empty()).collect();
    // Drop the host, so a bare id and a full link read the same way.
    if parts
        .first()
        .is_some_and(|host| host.contains('.') || *host == "localhost")
    {
        parts.remove(0);
    }
    let Some(last) = parts.last().copied() else {
        return Err("That is not a MetroDreamin' map link.".to_owned());
    };
    let id = percent_decode(last);
    match base64_decode(&id) {
        Some(decoded) if decoded.len() > 3 && decoded.contains(&b'|') => Ok(id),
        _ => Err("That is not a MetroDreamin' map link.".to_owned()),
    }
}

// ------------------------------------------------------------------- network

/// The document, then its partitions, then the map.
fn read(system_id: String, done: Done<Project>) {
    let url = format!(
        "{}/systems/{}/?key={}",
        ON_FIRESTORE,
        escape(&system_id),
        KEY
    );
    get(
        url,
        Box::new(move |result| {
            let document = match result {
                Ok(document) => document,
                Err(err) => return done(Err(err)),
            };
            let fields = plain(document.get("fields").unwrap_or(&Value::Null));
            let title = title(&fields);
            let inline = fields.get("map").cloned().unwrap_or(Value::Null);
            let system = system_id.clone();
            partitions(
                system,
                None,
                Vec::new(),
                Box::new(move |result| match result {
                    Ok(documents) => {
                        let map = merge(inline, documents);
                        done(convert(&title, &map))
                    }
                    // A map with nothing to partition keeps everything in the document.
                    Err(_) if inline.get("stations").is_some_and(Value::is_object) => {
                        done(convert(&title, &inline))
                    }
                    Err(err) => done(Err(err)),
                }),
            );
        }),
    );
}

fn title(fields: &Value) -> String {
    fields
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("MetroDreamin' map")
        .to_owned()
}

/// One page of the partition subcollection, and the next one while there is one.
fn partitions(system_id: String, page: Option<String>, found: Vec<Value>, done: Done<Vec<Value>>) {
    let mut url = format!(
        "{}/systems/{}/partitions?key={}",
        ON_FIRESTORE,
        escape(&system_id),
        KEY
    );
    if let Some(token) = &page {
        url.push_str(&format!("&pageToken={}", escape(token)));
    }
    get(
        url,
        Box::new(move |result| {
            let page = match result {
                Ok(page) => page,
                Err(err) => return done(Err(err)),
            };
            let mut found = found;
            if let Some(documents) = page.get("documents").and_then(Value::as_array) {
                found.extend(documents.iter().cloned());
            }
            match page.get("nextPageToken").and_then(Value::as_str) {
                Some(token) => partitions(system_id, Some(token.to_owned()), found, done),
                None => done(Ok(found)),
            }
        }),
    );
}

fn get(url: String, done: Done<Value>) {
    ehttp::fetch(ehttp::Request::get(url), move |response| {
        let result = match response {
            Ok(response) if response.ok => serde_json::from_slice(&response.bytes)
                .map_err(|err| format!("Could not read that map: {err}")),
            Ok(response) => Err(match response.status {
                404 => "That map could not be found.".to_owned(),
                status => format!("That map could not be read ({status})."),
            }),
            Err(err) => Err(format!("Could not reach MetroDreamin': {err}")),
        };
        done(result);
    });
}

// -------------------------------------------------------------------- account

/// Every map an account may hold, which the query asks for in one go.
const ACCOUNT_LIMIT: u32 = 1000;
/// The walk of an account's map numbers gives up after this many misses, and
/// never looks past this index.
const MAX_MISSES: usize = 40;
const MAX_INDEX: usize = 1000;

/// The account a profile link or a bare user id names.
pub fn user_id(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    let text = text.split(['?', '#']).next().unwrap_or(text);
    let text = text.trim_end_matches('/');
    // A profile link carries the id after `/user/`.
    if let Some((_, rest)) = text.split_once("/user/") {
        return account_token(rest);
    }
    // Without a profile path, only a bare id will do: a map link is not one.
    if text.contains(['/', '.', ' ']) {
        return None;
    }
    account_token(text)
}

fn account_token(text: &str) -> Option<String> {
    let id = text.split('/').next()?.trim();
    let shaped = id.len() >= 8
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        });
    shaped.then(|| id.to_owned())
}

/// Start finding every map an account has, private ones included.
pub fn start_listing(input: &str, repaint: egui::Context) -> Result<Listing, String> {
    let user = user_id(input).ok_or_else(|| "Paste a MetroDreamin' profile link.".to_owned())?;
    let (sender, receiver) = std::sync::mpsc::channel();
    list(
        user,
        Box::new(move |result| {
            let _ = sender.send(result);
            repaint.request_repaint();
        }),
    );
    Ok(Listing { receiver })
}

/// Ask the database which maps belong to the account. Every map carries the
/// account it belongs to, so one query names them all — private ones too,
/// since a private map is only hidden from browsing.
fn list(user: String, done: Done<Vec<Listed>>) {
    let query = serde_json::json!({
        "structuredQuery": {
            "from": [{ "collectionId": "systems" }],
            "where": { "fieldFilter": {
                "field": { "fieldPath": "userId" },
                "op": "EQUAL",
                "value": { "stringValue": user },
            }},
            "limit": ACCOUNT_LIMIT,
        }
    });
    let mut request = ehttp::Request::post(
        format!("{ON_FIRESTORE}:runQuery?key={KEY}"),
        query.to_string().into_bytes(),
    );
    request.headers =
        ehttp::Headers::new(&[("Content-Type", "application/json"), ("Accept", "*/*")]);
    ehttp::fetch(request, move |response| match response {
        Ok(response) if response.ok => match serde_json::from_slice::<Value>(&response.bytes) {
            Ok(rows) => done(Ok(collect(&rows))),
            Err(err) => done(Err(format!("Could not read that account: {err}"))),
        },
        // The database may refuse to be queried, and still hand over a map
        // whose id is known: walk the account's numbers instead.
        Ok(response) if matches!(response.status, 400 | 403 | 404 | 405) => {
            probe(user, 0, 0, Vec::new(), done);
        }
        Ok(response) => done(Err(format!(
            "That account could not be read ({}).",
            response.status
        ))),
        Err(err) => done(Err(format!("Could not reach MetroDreamin': {err}"))),
    });
}

/// The maps a query answer names, in the order the account numbered them.
fn collect(rows: &Value) -> Vec<Listed> {
    let mut found: Vec<Listed> = rows
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("document"))
        .filter_map(|document| {
            let fields = plain(document.get("fields").unwrap_or(&Value::Null));
            let id = fields
                .get("systemId")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    document
                        .get("name")
                        .and_then(Value::as_str)
                        .and_then(|name| name.rsplit('/').next())
                        .map(str::to_owned)
                })?;
            Some(Listed {
                title: title(&fields),
                id,
            })
        })
        .collect();
    found.sort_by_key(|map| (system_num(&map.id), map.title.to_lowercase()));
    found
}

/// The number a map holds in its account's numbering, read out of its id.
fn system_num(id: &str) -> u64 {
    base64_decode(id)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| {
            text.rsplit('|')
                .next()
                .and_then(|number| number.trim().parse().ok())
        })
        .unwrap_or(u64::MAX)
}

/// Walk an account's map numbers from zero, reading each map that is there by
/// its id until a long run of misses says the account has no more.
fn probe(user: String, index: usize, misses: usize, found: Vec<Listed>, done: Done<Vec<Listed>>) {
    let walk = Probe {
        user,
        index,
        misses,
        found,
        done,
    };
    Box::new(walk).step();
}

struct Probe {
    user: String,
    index: usize,
    misses: usize,
    found: Vec<Listed>,
    done: Done<Vec<Listed>>,
}

impl Probe {
    fn step(mut self: Box<Self>) {
        if self.index >= MAX_INDEX || self.misses >= MAX_MISSES {
            return (self.done)(Ok(self.found));
        }
        let id = base64_encode(format!("{}|{}", self.user, self.index).as_bytes());
        let url = format!("{}/systems/{}?key={}", ON_FIRESTORE, escape(&id), KEY);
        ehttp::fetch(ehttp::Request::get(url), move |response| {
            // A map that is not there is a miss either way: a 404, or a request
            // that never answered.
            let title = match response {
                Ok(response) if response.ok => serde_json::from_slice::<Value>(&response.bytes)
                    .ok()
                    .map(|document| plain(document.get("fields").unwrap_or(&Value::Null)))
                    .map(|fields| title(&fields)),
                _ => None,
            };
            match title {
                Some(title) => {
                    self.found.push(Listed { id, title });
                    self.misses = 0;
                }
                None => self.misses += 1,
            }
            self.index += 1;
            self.step();
        });
    }
}

// -------------------------------------------------------------------- values

/// Firestore wraps every value in a tag (`{"stringValue": …}`, `{"mapValue":
/// {"fields": …}}`); this takes the value out of it.
fn plain(value: &Value) -> Value {
    let Some(tagged) = value.as_object() else {
        return value.clone();
    };
    if let Some(text) = tagged.get("stringValue") {
        return text.clone();
    }
    if let Some(number) = tagged.get("integerValue") {
        let parsed = match number {
            Value::String(text) => text.parse::<i64>().ok(),
            other => other.as_i64(),
        };
        return parsed.map(Value::from).unwrap_or(Value::Null);
    }
    if let Some(number) = tagged.get("doubleValue") {
        return number.clone();
    }
    if let Some(flag) = tagged.get("booleanValue") {
        return flag.clone();
    }
    if let Some(value) = tagged.get("mapValue") {
        let fields = value.get("fields").and_then(Value::as_object);
        let mut map = Map::new();
        for (key, item) in fields.into_iter().flatten() {
            map.insert(key.clone(), plain(item));
        }
        return Value::Object(map);
    }
    if let Some(values) = tagged.get("arrayValue") {
        let items = values.get("values").and_then(Value::as_array);
        return Value::Array(items.into_iter().flatten().map(plain).collect());
    }
    if tagged.contains_key("nullValue") {
        return Value::Null;
    }
    // Not a tag: an ordinary object, its values still wrapped.
    let mut map = Map::new();
    for (key, item) in tagged {
        map.insert(key.clone(), plain(item));
    }
    Value::Object(map)
}

/// Every partition's entities gathered into one map.
fn merge(inline: Value, documents: Vec<Value>) -> Value {
    let mut merged = Map::new();
    absorb(&mut merged, inline);
    for document in documents {
        absorb(
            &mut merged,
            plain(document.get("fields").unwrap_or(&Value::Null)),
        );
    }
    Value::Object(merged)
}

fn absorb(merged: &mut Map<String, Value>, source: Value) {
    let Value::Object(source) = source else {
        return;
    };
    for key in ["stations", "lines", "interchanges", "lineGroups"] {
        let Some(items) = source.get(key).and_then(Value::as_object) else {
            continue;
        };
        let into = merged
            .entry(key.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(into) = into {
            for (id, item) in items {
                into.insert(id.clone(), item.clone());
            }
        }
    }
}

// ---------------------------------------------------------------- conversion

/// Turn a MetroDreamin' map into a project. Ids are renumbered in one space,
/// since a MetroDreamin' station and line may share a number.
pub fn convert(title: &str, map: &Value) -> Result<Project, String> {
    let mut project = Project::new(title);

    let mut nodes: HashMap<String, Node> = HashMap::new();
    for (key, station) in entities(map, "stations") {
        let Some(pos) = position(station) else {
            continue;
        };
        let node = if station.get("isWaypoint").and_then(Value::as_bool) == Some(true) {
            Node::Waypoint(project.add_waypoint(pos))
        } else {
            let name = station
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("Station");
            let id = project.add_named_station(name, pos);
            if let Some(grade) = station.get("grade").and_then(Value::as_str) {
                if let Some(stop) = project.stations.get_mut(&id) {
                    stop.elevation = elevation(grade);
                }
            }
            Node::Station(id)
        };
        nodes.insert(key.to_owned(), node);
    }
    if nodes.is_empty() {
        return Err("That map has no stations.".to_owned());
    }

    let mut groups: HashMap<String, Id> = HashMap::new();
    for (key, group) in entities(map, "lineGroups") {
        let label = group
            .get("label")
            .or_else(|| group.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .unwrap_or("Group");
        groups.insert(key.to_owned(), project.add_group(label));
    }

    for (_, line) in entities(map, "lines") {
        let name = line
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "Line".to_owned());
        let color = line
            .get("color")
            .and_then(Value::as_str)
            .and_then(hex_color)
            .unwrap_or(FALLBACK_COLOR);
        let id = project.add_line(name, color);
        // A line saved before modes existed is a metro line.
        let mode = line
            .get("mode")
            .and_then(Value::as_str)
            .and_then(Mode::from_key)
            .unwrap_or_default();

        let mut route: Vec<Node> = line
            .get("stationIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|key| key.as_str().and_then(|key| nodes.get(key).copied()))
            .collect();
        // A loop comes back to the node it started at; the last node is the
        // same place as the first, so it is dropped and the line closed.
        let closed = route.len() > 2 && route.first() == route.last();
        if closed {
            route.pop();
        }
        // A route that visits a node twice doubles back on itself.
        let mut seen = std::collections::HashSet::new();
        let turnback = route.iter().any(|node| !seen.insert(*node));
        let group = line
            .get("lineGroupId")
            .and_then(Value::as_str)
            .and_then(|group| groups.get(group).copied());
        // A patterned line in MetroDreamin' is a solid line with a shape
        // stamped along it; a line with no icon is solid.
        let pattern = line
            .get("icon")
            .and_then(Value::as_str)
            .and_then(Pattern::from_key)
            .unwrap_or(Pattern::Solid);

        if let Some(line) = project.lines.get_mut(&id) {
            line.mode = mode;
            line.pattern = pattern;
            line.nodes = route;
            line.closed = closed;
            line.turnback = turnback;
            line.group = group;
        }
    }

    for (_, interchange) in entities(map, "interchanges") {
        let stops: Vec<Id> = interchange
            .get("stationIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|key| key.as_str())
            .filter_map(|key| match nodes.get(key) {
                Some(Node::Station(id)) => Some(*id),
                _ => None,
            })
            .collect();
        if let (Some(&a), Some(&b)) = (stops.first(), stops.get(1)) {
            let id = project.alloc_id();
            project.transfers.insert(
                id,
                Transfer {
                    a,
                    b,
                    via: Vec::new(),
                },
            );
        }
    }

    project.sanitize();
    Ok(project)
}

/// The entities of one kind in a map, as id and body.
fn entities<'a>(map: &'a Value, key: &str) -> Vec<(&'a String, &'a Value)> {
    map.get(key)
        .and_then(Value::as_object)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn position(station: &Value) -> Option<GeoPos> {
    let lon = number(station.get("lng").or_else(|| station.get("lon")))?;
    let lat = number(station.get("lat"))?;
    Some(GeoPos::new(lon, lat))
}

fn number(value: Option<&Value>) -> Option<f32> {
    let value = value?;
    match value {
        Value::Number(number) => number.as_f64().map(|number| number as f32),
        Value::String(text) => text.trim().parse::<f32>().ok(),
        _ => None,
    }
}

fn elevation(grade: &str) -> Elevation {
    match grade.trim().to_ascii_lowercase().as_str() {
        "above" => Elevation::Above,
        "below" => Elevation::Below,
        _ => Elevation::At,
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

// ------------------------------------------------------------------- strings

/// Percent-decode, so an id carrying `%3D` reads as the `=` it stands for.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let pair = std::str::from_utf8(&bytes[index + 1..index + 3])
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok());
            if let Some(byte) = pair {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Base64, either alphabet, with or without padding.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for character in text.chars() {
        let value = match character {
            'A'..='Z' => character as u32 - 'A' as u32,
            'a'..='z' => character as u32 - 'a' as u32 + 26,
            '0'..='9' => character as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A path segment safe to put in a URL: an id may carry `/` or `+`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'/' | b'%' | b'?' | b'#' | b'&' | b' ' => {
                out.push_str(&format!("%{byte:02X}"));
            }
            _ => out.push(byte as char),
        }
    }
    out
}

/// Base64url without padding — how a map id carries `userId|number`.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |at: usize| u32::from(*chunk.get(at).unwrap_or(&0));
        let bits = byte(0) << 16 | byte(1) << 8 | byte(2);
        out.push(ALPHABET[(bits >> 18) as usize & 63] as char);
        out.push(ALPHABET[(bits >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(bits >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[bits as usize & 63] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        serde_json::json!({
            "stations": {
                "0": {"id": "0", "name": "Alpha", "lat": 51.5, "lng": -0.1},
                "1": {"id": "1", "name": "Bravo", "lat": 51.51, "lng": -0.12, "grade": "below"},
                "2": {"id": "2", "lat": 51.52, "lng": -0.13, "isWaypoint": true},
                "3": {"id": "3", "name": "Charlie", "lat": 51.53, "lng": -0.14}
            },
            "lines": {
                "0": {
                    "id": "0", "name": "Circle", "color": "#4363d8",
                    "stationIds": ["0", "1", "2", "3", "0"], "lineGroupId": "0",
                    "mode": "TRAM"
                },
                "1": {"id": "1", "name": "Spur", "color": "nonsense", "stationIds": ["3", "9"]}
            },
            "lineGroups": {"0": {"id": "0", "label": "Loops"}},
            "interchanges": {"0": {"id": "0", "stationIds": ["1", "3"]}}
        })
    }

    fn station_id(project: &Project, name: &str) -> Id {
        *project
            .stations
            .iter()
            .find(|(_, station)| station.name == name)
            .expect("the station is in the map")
            .0
    }

    #[test]
    fn a_link_names_the_map() {
        assert_eq!(
            system_id("https://metrodreamin.com/view/TTAwNGdHOUVIclBZZmo2cjJqalRtZEFQVUpMMnww")
                .unwrap(),
            "TTAwNGdHOUVIclBZZmo2cjJqalRtZEFQVUpMMnww"
        );
        // The padding a link carries is percent-encoded.
        assert_eq!(
            system_id(
                "https://metrodreamin.com/view/M09URlVQWjBCT2JmWk9mWGJOczF0a0VmaFVCMnwxMg%3D%3D"
            )
            .unwrap(),
            "M09URlVQWjBCT2JmWk9mWGJOczF0a0VmaFVCMnwxMg=="
        );
        assert_eq!(
            system_id("www.metrodreamin.com/edit/TTAwNGdHOUVIclBZZmo2cjJqalRtZEFQVUpMMnww/")
                .unwrap(),
            "TTAwNGdHOUVIclBZZmo2cjJqalRtZEFQVUpMMnww"
        );
        assert_eq!(
            system_id("TTAwNGdHOUVIclBZZmo2cjJqalRtZEFQVUpMMnww").unwrap(),
            "TTAwNGdHOUVIclBZZmo2cjJqalRtZEFQVUpMMnww"
        );
        assert!(system_id("").is_err());
        assert!(system_id("https://example.com/hello").is_err());
        assert!(system_id("https://metrodreamin.com/").is_err());
    }

    #[test]
    fn firestore_values_are_unwrapped() {
        let document: Value = serde_json::from_str(
            r#"{"fields": {"title": {"stringValue": "Test"},
                "count": {"integerValue": "12"},
                "map": {"mapValue": {"fields": {
                    "stations": {"mapValue": {"fields": {}}},
                    "lines": {"mapValue": {}},
                    "ids": {"arrayValue": {"values": [
                        {"stringValue": "a"}, {"booleanValue": true}
                    ]}}
                }}}}}"#,
        )
        .unwrap();
        let fields = plain(document.get("fields").unwrap());
        assert_eq!(fields["title"], "Test");
        assert_eq!(fields["count"], 12);
        assert_eq!(fields["map"]["ids"][1], true);
        // An empty map has no `fields` at all.
        assert!(fields["map"]["lines"].as_object().unwrap().is_empty());
    }

    #[test]
    fn converts_a_route_its_loop_and_its_walk() {
        let project = convert("Test Map", &sample()).unwrap();
        assert_eq!(project.name, "Test Map");
        assert_eq!(project.stations.len(), 3);
        assert_eq!(project.waypoints.len(), 1);

        let bravo = station_id(&project, "Bravo");
        assert_eq!(project.stations[&bravo].elevation, Elevation::Below);

        let circle = project
            .lines
            .values()
            .find(|line| line.name == "Circle")
            .unwrap();
        // The repeated first station is dropped and the line closed instead.
        assert!(circle.closed);
        assert_eq!(circle.nodes.len(), 4);
        assert_eq!(
            circle.nodes[0],
            Node::Station(station_id(&project, "Alpha"))
        );
        assert!(matches!(circle.nodes[2], Node::Waypoint(_)));
        assert_eq!(circle.color, [0x43, 0x63, 0xd8]);
        assert_eq!(circle.mode, Mode::Tram);
        let group = circle.group.expect("the line is filed under its group");
        assert_eq!(project.groups[&group].name, "Loops");

        let spur = project
            .lines
            .values()
            .find(|line| line.name == "Spur")
            .unwrap();
        // A node the map does not have is simply not on the line.
        assert_eq!(spur.nodes.len(), 1);
        // A line with no mode of its own is a metro line.
        assert_eq!(spur.mode, Mode::Rapid);
        assert_eq!(spur.color, FALLBACK_COLOR);
        assert_eq!(spur.group, None);

        // The pair walked between keeps its two stations.
        let walk = project.transfers.values().next().unwrap();
        assert_eq!([walk.a, walk.b], [bravo, station_id(&project, "Charlie")]);
        assert!(walk.via.is_empty());
    }

    /// Reads a real published map over the network. Run it with
    /// `cargo test a_published_map -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_published_map_comes_across() {
        // Current LA Metro: partitioned storage, line groups, a walking link.
        let (sender, receiver) = std::sync::mpsc::channel();
        read(
            "M01kRDA4SjNieFptWnFzd0NIcWhVM2kzenoxMnww".to_owned(),
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        let project = receiver
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("the read should answer")
            .expect("the map should convert");
        println!(
            "{}: {} stations, {} waypoints, {} lines, {} groups, {} walks",
            project.name,
            project.stations.len(),
            project.waypoints.len(),
            project.lines.len(),
            project.groups.len(),
            project.transfers.len()
        );
        assert!(!project.stations.is_empty());
        assert!(!project.lines.is_empty());
        assert!(!project.groups.is_empty());
        assert!(!project.transfers.is_empty());
        assert!(project.lines.values().any(|line| line.nodes.len() > 10));
    }

    /// Every shape MetroDreamin' strokes a line with - `LINE_ICON_SHAPES` in
    /// its own source, written lowercase - and, with none or with an icon that
    /// is not a shape, a solid line.
    #[test]
    fn every_line_shape_comes_across() {
        let shapes = [
            ("circle", Pattern::Circle),
            ("diamond", Pattern::Diamond),
            ("heart", Pattern::Heart),
            ("plus", Pattern::Plus),
            ("star", Pattern::Star),
            ("", Pattern::Solid),
            ("BUS", Pattern::Solid),
        ];
        let mut lines = Map::new();
        for (index, (icon, _)) in shapes.iter().enumerate() {
            let mut line = serde_json::json!({
                "id": index.to_string(),
                "name": index.to_string(),
                "stationIds": ["0", "1"],
            });
            if !icon.is_empty() {
                line["icon"] = serde_json::json!(*icon);
            }
            lines.insert(index.to_string(), line);
        }
        let map = serde_json::json!({
            "stations": {
                "0": {"id": "0", "name": "Alpha", "lat": 51.5, "lng": -0.1},
                "1": {"id": "1", "name": "Bravo", "lat": 51.51, "lng": -0.12}
            },
            "lines": Value::Object(lines)
        });

        let project = convert("Shapes", &map).unwrap();
        for (index, (icon, pattern)) in shapes.iter().enumerate() {
            let line = project
                .lines
                .values()
                .find(|line| line.name == index.to_string())
                .expect("the line is in the map");
            assert_eq!(line.pattern, *pattern, "a line carrying `{icon}`");
        }
    }

    #[test]
    fn an_empty_map_is_refused() {
        let empty = serde_json::json!({"stations": {}, "lines": {}});
        assert!(convert("Empty", &empty).is_err());
    }

    #[test]
    fn a_profile_link_names_the_account() {
        assert_eq!(
            user_id("https://metrodreamin.com/user/3MAkSMqXSDX8ZM8a8vrJiFDYJoa2").unwrap(),
            "3MAkSMqXSDX8ZM8a8vrJiFDYJoa2"
        );
        assert_eq!(
            user_id("www.metrodreamin.com/user/3MAkSMqXSDX8ZM8a8vrJiFDYJoa2/maps").unwrap(),
            "3MAkSMqXSDX8ZM8a8vrJiFDYJoa2"
        );
        assert_eq!(
            user_id(" 3MAkSMqXSDX8ZM8a8vrJiFDYJoa2 ").unwrap(),
            "3MAkSMqXSDX8ZM8a8vrJiFDYJoa2"
        );
        // A map link names one map, not an account.
        assert!(
            user_id("https://metrodreamin.com/view/M01kRDA4SjNieFptWnFzd0NIcWhVM2kzenoxMnww")
                .is_none()
        );
        assert!(user_id("").is_none());
        assert!(user_id("three words").is_none());
        assert!(user_id("short").is_none());
    }

    #[test]
    fn a_map_id_is_its_account_and_number_in_base64() {
        assert_eq!(
            base64_encode(b"3MdD08J3bxZmZqswCHqhU3i3zz12|0"),
            "M01kRDA4SjNieFptWnFzd0NIcWhVM2kzenoxMnww"
        );
        // Padding is equally fine on the way back in.
        assert_eq!(
            system_num("M01kRDA4SjNieFptWnFzd0NIcWhVM2kzenoxMnwxMg=="),
            12
        );
        assert_eq!(base64_encode(b"a"), "YQ");
        assert_eq!(base64_encode(b"ab"), "YWI");
    }

    #[test]
    fn a_query_answer_is_every_map_of_the_account() {
        let row = |number: u32, title: &str| {
            let id = base64_encode(format!("3MAkSMqXSDX8ZM8a8vrJiFDYJoa2|{number}").as_bytes());
            serde_json::json!({"document": {
                "name": format!("projects/metrodreamin/databases/(default)/documents/systems/{id}"),
                "fields": {"title": {"stringValue": title}}
            }})
        };
        let rows = serde_json::json!([
            row(2, "Third"),
            row(10, "Fourth"),
            {"readTime": "2026-10-29T16:34:54.394894Z"},
            row(0, "First"),
        ]);

        let maps = collect(&rows);
        // The account's own numbering, not the order the answer came in.
        assert_eq!(
            maps.iter()
                .map(|map| map.title.as_str())
                .collect::<Vec<_>>(),
            ["First", "Third", "Fourth"]
        );
        assert!(maps.iter().all(|map| map.id.starts_with("M01")));
        assert!(collect(&serde_json::json!([])).is_empty());
    }

    /// Lists a real account over the network, both ways. Run it with
    /// `cargo test a_whole_account -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_whole_account_comes_across() {
        let list_it = |user: &str| -> Vec<Listed> {
            let (sender, receiver) = std::sync::mpsc::channel();
            list(
                user.to_owned(),
                Box::new(move |result| {
                    let _ = sender.send(result);
                }),
            );
            receiver
                .recv_timeout(std::time::Duration::from_secs(90))
                .expect("the query should answer")
                .expect("the account should list")
        };

        // An account whose only map is private: it is still the account's map.
        let seattle = list_it("3MAkSMqXSDX8ZM8a8vrJiFDYJoa2");
        println!("seattle: {seattle:?}");
        assert!(seattle.iter().any(|map| map.title.contains("Seattle")));

        let la = list_it("3MdD08J3bxZmZqswCHqhU3i3zz12");
        println!("la: {}", la.len());
        assert!(la.len() >= 4);
        let numbers: Vec<u64> = la.iter().map(|map| system_num(&map.id)).collect();
        assert!(numbers.windows(2).all(|pair| pair[0] <= pair[1]));

        // The walk of an account's numbers finds the same maps alone.
        let (sender, receiver) = std::sync::mpsc::channel();
        probe(
            "3MdD08J3bxZmZqswCHqhU3i3zz12".to_owned(),
            0,
            0,
            Vec::new(),
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        let walked = receiver
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("the walk should answer")
            .expect("the walk should finish");
        println!("walked: {}", walked.len());
        assert_eq!(walked, la);
    }
}
