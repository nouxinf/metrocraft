//! Asynchronous raster tiles for the basemap.
//!
//! Every basemap is a plain XYZ raster service that needs no API key and no
//! account. Requests are fetched off the UI thread and decoded before they
//! reach it.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};
use web_time::Instant;

const MAX_IN_FLIGHT: usize = 6;
const MAX_CACHED: usize = 420;
const RETRY_AFTER: Duration = Duration::from_secs(20);

/// Whether the map under the lines is light or dark, which decides the ink of
/// everything drawn over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Light,
    Dark,
}

/// The basemaps offered in the View menu.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum Basemap {
    #[default]
    OpenStreetMap,
    Humanitarian,
    France,
    Outdoors,
    Streets,
    Topographic,
    Relief,
    Satellite,
    LightCanvas,
    DarkCanvas,
}

impl Basemap {
    /// Every basemap, in the order the menu lists them: streets, then terrain,
    /// then imagery, then the pair of quiet canvases.
    pub const ALL: [Self; 10] = [
        Self::OpenStreetMap,
        Self::Humanitarian,
        Self::France,
        Self::Outdoors,
        Self::Streets,
        Self::Topographic,
        Self::Relief,
        Self::Satellite,
        Self::LightCanvas,
        Self::DarkCanvas,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::OpenStreetMap => "OpenStreetMap",
            Self::Humanitarian => "Humanitarian",
            Self::France => "France",
            Self::Outdoors => "Outdoors",
            Self::Streets => "Streets",
            Self::Topographic => "Topographic",
            Self::Relief => "Relief",
            Self::Satellite => "Satellite",
            Self::LightCanvas => "Light Canvas",
            Self::DarkCanvas => "Dark Canvas",
        }
    }

    /// `{s}` is rotated over the provider's subdomains; the Esri endpoints take
    /// the row before the column.
    fn template(self) -> &'static str {
        match self {
            Self::OpenStreetMap => "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
            Self::Humanitarian => "https://{s}.tile.openstreetmap.fr/hot/{z}/{x}/{y}.png",
            Self::France => "https://{s}.tile.openstreetmap.fr/osmfr/{z}/{x}/{y}.png",
            Self::Outdoors => "https://{s}.tile.opentopomap.org/{z}/{x}/{y}.png",
            Self::Streets => concat!(
                "https://server.arcgisonline.com/ArcGIS/rest/services",
                "/World_Street_Map/MapServer/tile/{z}/{y}/{x}"
            ),
            Self::Topographic => concat!(
                "https://server.arcgisonline.com/ArcGIS/rest/services",
                "/World_Topo_Map/MapServer/tile/{z}/{y}/{x}"
            ),
            Self::Relief => concat!(
                "https://server.arcgisonline.com/ArcGIS/rest/services",
                "/World_Shaded_Relief/MapServer/tile/{z}/{y}/{x}"
            ),
            Self::Satellite => concat!(
                "https://server.arcgisonline.com/ArcGIS/rest/services",
                "/World_Imagery/MapServer/tile/{z}/{y}/{x}"
            ),
            Self::LightCanvas => concat!(
                "https://server.arcgisonline.com/ArcGIS/rest/services",
                "/Canvas/World_Light_Gray_Base/MapServer/tile/{z}/{y}/{x}"
            ),
            Self::DarkCanvas => concat!(
                "https://server.arcgisonline.com/ArcGIS/rest/services",
                "/Canvas/World_Dark_Gray_Base/MapServer/tile/{z}/{y}/{x}"
            ),
        }
    }

    /// The deepest level this basemap really serves. Past it the service either
    /// refuses or hands back a "no data" tile.
    pub fn max_zoom(self) -> u8 {
        match self {
            Self::OpenStreetMap | Self::Humanitarian | Self::France => 19,
            Self::Outdoors => 17,
            Self::Streets | Self::Topographic | Self::Satellite => 19,
            Self::Relief => 13,
            Self::LightCanvas | Self::DarkCanvas => 16,
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Satellite | Self::DarkCanvas => Tone::Dark,
            _ => Tone::Light,
        }
    }

    /// The credit the provider asks for, and where it leads.
    pub fn attribution(self) -> (&'static str, &'static str) {
        const ESRI_TERMS: &str = "https://www.esri.com/en-us/legal/terms/full-master-agreement";
        match self {
            Self::OpenStreetMap | Self::Humanitarian => (
                "OpenStreetMap contributors",
                "https://www.openstreetmap.org/copyright",
            ),
            Self::France => ("OpenStreetMap France", "https://openstreetmap.fr/"),
            Self::Outdoors => (
                "OpenTopoMap, OpenStreetMap contributors",
                "https://opentopomap.org/about",
            ),
            Self::Streets | Self::LightCanvas | Self::DarkCanvas => {
                ("Esri, HERE, Garmin, OpenStreetMap contributors", ESRI_TERMS)
            }
            Self::Topographic => (
                "Esri, HERE, USGS, Intermap, NRCAN, OpenStreetMap contributors",
                ESRI_TERMS,
            ),
            Self::Relief => ("Esri", ESRI_TERMS),
            Self::Satellite => ("Esri, Maxar, Earthstar Geographics", ESRI_TERMS),
        }
    }

    pub fn url(self, key: TileKey) -> String {
        let template = self.template();
        let template = if template.contains("{s}") {
            let subdomain = ["a", "b", "c"][(key.x + key.y).rem_euclid(3) as usize];
            template.replace("{s}", subdomain)
        } else {
            template.to_owned()
        };
        template
            .replace("{z}", &key.z.to_string())
            .replace("{x}", &key.x.to_string())
            .replace("{y}", &key.y.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub z: u8,
    pub x: i64,
    pub y: i64,
}

impl TileKey {
    pub fn new(z: u8, x: i64, y: i64) -> Self {
        let count = 1i64 << z;
        Self {
            z,
            x: x.rem_euclid(count),
            y: y.clamp(0, count - 1),
        }
    }

    pub fn parent(self) -> Option<Self> {
        (self.z > 0).then(|| Self {
            z: self.z - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }
}

/// The tile the preferences previews show: central London at zoom 13, where
/// streets, the river and parks all appear on one tile of every basemap.
pub const PREVIEW_TILE: TileKey = TileKey {
    z: 13,
    x: 4093,
    y: 2724,
};

enum TileState {
    Loading,
    Ready {
        texture: TextureHandle,
        last_used: Instant,
    },
    Failed(Instant),
}

struct TileEntry {
    state: TileState,
}

/// A tile of one basemap. The same coordinates on another basemap are a
/// different picture, so the basemap is part of the identity.
pub type TileId = (Basemap, TileKey);

enum Message {
    Loaded(TileId, ColorImage),
    Failed(TileId),
}

/// Downloads and caches map tiles for every basemap in use.
pub struct TileStore {
    entries: HashMap<TileId, TileEntry>,
    queued: Vec<TileId>,
    in_flight: usize,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

impl Default for TileStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TileStore {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            entries: HashMap::new(),
            queued: Vec::new(),
            in_flight: 0,
            sender,
            receiver,
        }
    }

    /// Queue a tile if it is neither cached nor already on its way.
    pub fn ensure(&mut self, id: TileId) {
        if self.entries.contains_key(&id) {
            return;
        }
        if !self.queued.contains(&id) {
            self.queued.push(id);
        }
    }

    pub fn ready(&mut self, id: TileId) -> Option<&TextureHandle> {
        match self.entries.get_mut(&id).map(|entry| &mut entry.state) {
            Some(TileState::Ready { texture, last_used }) => {
                *last_used = Instant::now();
                Some(texture)
            }
            _ => None,
        }
    }

    /// The nearest cached ancestor, used to keep the map filled in while a new
    /// zoom level streams in.
    pub fn fallback(&mut self, id: TileId) -> Option<(TileKey, TextureHandle)> {
        let (basemap, key) = id;
        let mut parent = key.parent();
        for _ in 0..6 {
            let parent_key = parent?;
            let cached = matches!(
                self.entries.get(&(basemap, parent_key)),
                Some(TileEntry {
                    state: TileState::Ready { .. }
                })
            );
            if cached {
                if let Some(TileEntry {
                    state: TileState::Ready { texture, last_used },
                }) = self.entries.get_mut(&(basemap, parent_key))
                {
                    *last_used = Instant::now();
                    return Some((parent_key, texture.clone()));
                }
            }
            parent = parent_key.parent();
        }
        None
    }

    /// Decode finished downloads and start new requests, up to a small cap.
    pub fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.receiver.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            match message {
                Message::Loaded(id, image) => {
                    let (basemap, key) = id;
                    let texture = ctx.load_texture(
                        format!("tile-{}-{}-{}-{}", key.z, key.x, key.y, basemap as u8),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.entries.insert(
                        id,
                        TileEntry {
                            state: TileState::Ready {
                                texture,
                                last_used: Instant::now(),
                            },
                        },
                    );
                }
                Message::Failed(id) => {
                    self.entries.insert(
                        id,
                        TileEntry {
                            state: TileState::Failed(Instant::now()),
                        },
                    );
                }
            }
        }

        while self.in_flight < MAX_IN_FLIGHT {
            let Some(id) = self.queued.pop() else {
                break;
            };
            match self.entries.get(&id) {
                Some(TileEntry {
                    state: TileState::Failed(when),
                }) if when.elapsed() < RETRY_AFTER => continue,
                Some(TileEntry {
                    state: TileState::Ready { .. } | TileState::Loading,
                }) => continue,
                _ => {}
            }
            self.entries.insert(
                id,
                TileEntry {
                    state: TileState::Loading,
                },
            );
            self.in_flight += 1;
            self.request(ctx, id);
        }

        if self.in_flight > 0 || !self.queued.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        self.prune();
    }

    fn request(&self, ctx: &egui::Context, id: TileId) {
        let url = id.0.url(id.1);
        #[cfg(not(target_arch = "wasm32"))]
        let request = {
            let mut request = ehttp::Request::get(url);
            request.headers.insert(
                "User-Agent",
                "Metrocraft/0.1 (desktop transit map editor)".to_owned(),
            );
            request
        };
        #[cfg(target_arch = "wasm32")]
        let request = ehttp::Request::get(url);

        let sender = self.sender.clone();
        let painter_ctx = ctx.clone();
        ehttp::fetch(request, move |response| {
            let message = match response {
                Ok(response) if response.ok => decode_tile(&response.bytes)
                    .map(|image| Message::Loaded(id, image))
                    .unwrap_or(Message::Failed(id)),
                _ => Message::Failed(id),
            };
            let _ = sender.send(message);
            painter_ctx.request_repaint();
        });
    }

    fn prune(&mut self) {
        if self.entries.len() <= MAX_CACHED {
            return;
        }
        let mut loaded: Vec<(TileId, Instant)> = self
            .entries
            .iter()
            .filter_map(|(key, entry)| match entry.state {
                TileState::Ready { last_used, .. } => Some((*key, last_used)),
                _ => None,
            })
            .collect();
        loaded.sort_by_key(|(_, used)| *used);
        let remove = self
            .entries
            .len()
            .saturating_sub(MAX_CACHED)
            .min(loaded.len());
        for (key, _) in loaded.into_iter().take(remove) {
            self.entries.remove(&key);
        }
    }
}

fn decode_tile(bytes: &[u8]) -> Option<ColorImage> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        decode_png(bytes)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        decode_jpeg(bytes)
    } else {
        None
    }
}

fn decode_jpeg(bytes: &[u8]) -> Option<ColorImage> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    let data = decoder.decode().ok()?;
    let info = decoder.info()?;
    let size = [info.width as usize, info.height as usize];

    let pixels: Vec<egui::Color32> = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => data
            .iter()
            .map(|value| egui::Color32::from_gray(*value))
            .collect(),
        jpeg_decoder::PixelFormat::RGB24 => data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|[r, g, b]| egui::Color32::from_rgb(*r, *g, *b))
            .collect(),
        jpeg_decoder::PixelFormat::CMYK32 => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|[c, m, y, k]| {
                let key = 255 - *k as u16;
                let channel = |value: u8| 255u8.saturating_sub((value as u16 * key / 255) as u8);
                egui::Color32::from_rgb(channel(*c), channel(*m), channel(*y))
            })
            .collect(),
        jpeg_decoder::PixelFormat::L16 => return None,
    };

    Some(ColorImage::new(size, pixels))
}

fn decode_png(bytes: &[u8]) -> Option<ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    let data = &buffer[..info.buffer_size()];

    let mut pixels = Vec::with_capacity((info.width * info.height) as usize);
    match info.color_type {
        png::ColorType::Rgba => {
            for [r, g, b, a] in data.as_chunks::<4>().0 {
                pixels.push(egui::Color32::from_rgba_unmultiplied(*r, *g, *b, *a));
            }
        }
        png::ColorType::Rgb => {
            for [r, g, b] in data.as_chunks::<3>().0 {
                pixels.push(egui::Color32::from_rgb(*r, *g, *b));
            }
        }
        png::ColorType::Grayscale => {
            for value in data {
                pixels.push(egui::Color32::from_gray(*value));
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for [value, alpha] in data.as_chunks::<2>().0 {
                pixels.push(egui::Color32::from_rgba_unmultiplied(
                    *value, *value, *value, *alpha,
                ));
            }
        }
        png::ColorType::Indexed => return None,
    }

    Some(ColorImage::new(
        [info.width as usize, info.height as usize],
        pixels,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_basemap_builds_a_tile_url() {
        let key = TileKey::new(16, 32746, 21793);
        assert_eq!(
            Basemap::OpenStreetMap.url(key),
            "https://tile.openstreetmap.org/16/32746/21793.png"
        );
        // The Esri services take the row before the column.
        assert!(Basemap::Satellite
            .url(key)
            .ends_with("/tile/16/21793/32746"));
        // Subdomains rotate, and stay inside the three the providers run.
        let outdoors = Basemap::Outdoors.url(key);
        assert!(outdoors.contains(".tile.opentopomap.org/16/32746/21793.png"));
        assert!(
            outdoors.starts_with("https://a.")
                || outdoors.starts_with("https://b.")
                || outdoors.starts_with("https://c.")
        );
        for basemap in Basemap::ALL {
            let url = basemap.url(key);
            assert!(url.starts_with("https://"), "{url}");
            assert!(!url.contains('{'), "{url}");
        }
    }

    #[test]
    fn the_same_tile_of_two_basemaps_is_kept_apart() {
        let mut store = TileStore::new();
        let tile = TileKey::new(4, 3, 5);
        store.ensure((Basemap::OpenStreetMap, tile));
        store.ensure((Basemap::OpenStreetMap, tile));
        assert_eq!(store.queued.len(), 1);

        // Another basemap has its own picture at those coordinates.
        store.ensure((Basemap::Satellite, tile));
        assert_eq!(store.queued.len(), 2);
    }

    #[test]
    fn the_preview_tile_lands_on_the_map() {
        assert_eq!(PREVIEW_TILE.z, 13);
        assert_eq!(
            Basemap::OpenStreetMap.url(PREVIEW_TILE),
            "https://tile.openstreetmap.org/13/4093/2724.png"
        );
    }

    #[test]
    fn tiles_wrap_horizontally_and_clamp_vertically() {
        let wrapped = TileKey::new(3, -1, 99);
        assert_eq!(wrapped.x, 7);
        assert_eq!(wrapped.y, 7);
        assert_eq!(wrapped.z, 3);
        assert_eq!(TileKey::new(0, 0, 0).parent(), None);
        assert_eq!(TileKey::new(5, 9, 12).parent().unwrap().x, 4);
    }

    #[test]
    fn decodes_eight_bit_rgba_png() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 0, 255, 128])
                .unwrap();
        }
        let image = decode_tile(&encoded).expect("png should decode");
        assert_eq!(image.size, [2, 1]);
        assert_eq!(image.pixels[0], egui::Color32::from_rgb(255, 0, 0));
        assert_eq!(
            image.pixels[1],
            egui::Color32::from_rgba_unmultiplied(0, 0, 255, 128)
        );
    }

    #[test]
    fn decodes_rgb_jpeg() {
        // 8x8 solid blue JPEG, encoded by the `jpeg-encoder` used for this fixture.
        let encoded = include_bytes!("../assets/test-tile.jpg");
        let image = decode_tile(encoded).expect("jpeg should decode");
        assert_eq!(image.size, [8, 8]);
        let pixel = image.pixels[0];
        assert!(pixel.b() > 200, "expected a blue pixel, got {pixel:?}");
        assert!(pixel.r() < 60, "expected a blue pixel, got {pixel:?}");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_tile(b"not a tile").is_none());
        assert!(decode_tile(&[0xFF, 0xD8, 0xFF, 0x00, 0x01]).is_none());
    }
}
