//! Web Mercator camera for the slippy map.

use std::f64::consts::PI;

use eframe::egui::{Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};

use crate::model::GeoPos;

pub const TILE_SIZE: f64 = 256.0;
pub const MIN_ZOOM: f64 = 2.0;
/// The deepest any basemap here is worth magnifying. A basemap that stops
/// serving detail earlier passes its own, lower, ceiling to the calls below.
pub const MAX_ZOOM: f64 = 19.0;
const MAX_FIT_ZOOM: f64 = 15.5;
pub const MAX_LAT: f64 = 85.051_128_779_806_59;

/// A slippy-map camera. `zoom` is fractional; a value of `n` means one tile is
/// 256 points wide and the world is `2^n` tiles across.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapView {
    pub lon: f64,
    pub lat: f64,
    pub zoom: f64,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            lon: 10.0,
            lat: 30.0,
            zoom: 2.6,
        }
    }
}

impl MapView {
    pub fn world_size_at(zoom: f64) -> f64 {
        TILE_SIZE * zoom.exp2()
    }

    /// The lowest zoom at which the world still covers the viewport, so the
    /// basemap never leaves bare canvas down the sides of the window.
    pub fn min_zoom_for(rect: Rect) -> f64 {
        (rect.width() as f64 / TILE_SIZE).log2().max(MIN_ZOOM)
    }

    pub fn geo_to_world_at(pos: GeoPos, zoom: f64) -> (f64, f64) {
        let size = Self::world_size_at(zoom);
        (
            lon_fraction(pos.lon as f64) * size,
            lat_fraction(pos.lat as f64) * size,
        )
    }

    pub fn world_to_geo_at(world: (f64, f64), zoom: f64) -> GeoPos {
        let size = Self::world_size_at(zoom);
        let lon = (world.0 / size) * 360.0 - 180.0;
        let lat = unproject_lat((world.1 / size).clamp(0.0, 1.0));
        GeoPos::new(lon as f32, lat as f32)
    }

    pub fn center_world_at(&self, zoom: f64) -> (f64, f64) {
        Self::geo_to_world_at(GeoPos::new(self.lon as f32, self.lat as f32), zoom)
    }

    pub fn world_to_screen(&self, pos: GeoPos, rect: Rect) -> Pos2 {
        let (x, y) = Self::geo_to_world_at(pos, self.zoom);
        let (cx, cy) = self.center_world_at(self.zoom);
        rect.center() + Vec2::new((x - cx) as f32, (y - cy) as f32)
    }

    pub fn screen_to_world(&self, screen: Pos2, rect: Rect) -> GeoPos {
        let (cx, cy) = self.center_world_at(self.zoom);
        let offset = screen - rect.center();
        Self::world_to_geo_at((cx + offset.x as f64, cy + offset.y as f64), self.zoom)
    }

    pub fn pan_pixels(&mut self, delta: Vec2, rect: Rect) {
        let (cx, cy) = self.center_world_at(self.zoom);
        self.set_center_world((cx - delta.x as f64, cy - delta.y as f64), self.zoom, rect);
    }

    /// Zoom by `factor` with the geography under `cursor` staying put.
    pub fn zoom_at(&mut self, factor: f64, cursor: Pos2, rect: Rect, max_zoom: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor = self.screen_to_world(cursor, rect);
        let limit = max_zoom.min(MAX_ZOOM);
        let zoom = (self.zoom + factor.log2()).clamp(Self::min_zoom_for(rect), limit);
        let (ax, ay) = Self::geo_to_world_at(anchor, zoom);
        let offset = cursor - rect.center();
        self.zoom = zoom;
        self.set_center_world((ax - offset.x as f64, ay - offset.y as f64), zoom, rect);
    }

    pub fn clamp_to(&mut self, rect: Rect, max_zoom: f64) {
        if !self.lon.is_finite() || !self.lat.is_finite() || !self.zoom.is_finite() {
            *self = Self::default();
            return;
        }
        self.zoom = self
            .zoom
            .clamp(Self::min_zoom_for(rect), max_zoom.min(MAX_ZOOM));
        let center = self.center_world_at(self.zoom);
        self.set_center_world(center, self.zoom, rect);
    }

    /// Frame the given bounds with a comfortable margin.
    pub fn fit_bounds(&mut self, min: GeoPos, max: GeoPos, rect: Rect, max_zoom: f64) {
        if rect.width() < 4.0 || rect.height() < 4.0 {
            return;
        }
        let west = min.lon.min(max.lon) as f64;
        let east = min.lon.max(max.lon) as f64;
        let south = min.lat.min(max.lat) as f64;
        let north = min.lat.max(max.lat) as f64;

        let width_fraction = ((east - west) / 360.0).max(1.0e-7);
        let top = lat_fraction(north);
        let bottom = lat_fraction(south);
        let height_fraction = (bottom - top).abs().max(1.0e-7);
        let margin = 1.3;

        let zoom_x = (rect.width() as f64 / (width_fraction * TILE_SIZE * margin)).log2();
        let zoom_y = (rect.height() as f64 / (height_fraction * TILE_SIZE * margin)).log2();
        let zoom = zoom_x
            .min(zoom_y)
            .clamp(Self::min_zoom_for(rect), MAX_FIT_ZOOM.min(max_zoom));

        self.zoom = zoom;
        let center_fraction_x = lon_fraction((west + east) * 0.5);
        let center_fraction_y = (top + bottom) * 0.5;
        let size = Self::world_size_at(zoom);
        self.lon = (center_fraction_x * 360.0 - 180.0).clamp(-180.0, 180.0);
        self.lat = unproject_lat(center_fraction_y).clamp(-MAX_LAT, MAX_LAT);
        self.set_center_world(
            (center_fraction_x * size, center_fraction_y * size),
            zoom,
            rect,
        );
    }

    pub fn focus(&mut self, pos: GeoPos, rect: Rect, min_zoom: f64, max_zoom: f64) {
        self.lon = (pos.lon as f64).clamp(-180.0, 180.0);
        self.lat = (pos.lat as f64).clamp(-MAX_LAT, MAX_LAT);
        self.zoom = self
            .zoom
            .max(min_zoom)
            .clamp(Self::min_zoom_for(rect), max_zoom.min(MAX_ZOOM));
        self.clamp_to(rect, max_zoom);
    }

    fn set_center_world(&mut self, world: (f64, f64), zoom: f64, rect: Rect) {
        let size = Self::world_size_at(zoom);
        let width = rect.width() as f64;
        let height = rect.height() as f64;
        let x = if size <= width {
            size * 0.5
        } else {
            world.0.clamp(width * 0.5, size - width * 0.5)
        };
        let y = if size <= height {
            size * 0.5
        } else {
            world.1.clamp(height * 0.5, size - height * 0.5)
        };
        self.zoom = zoom.clamp(Self::min_zoom_for(rect), MAX_ZOOM);
        self.lon = ((x / size) * 360.0 - 180.0).clamp(-180.0, 180.0);
        self.lat = unproject_lat((y / size).clamp(0.0, 1.0)).clamp(-MAX_LAT, MAX_LAT);
    }

    // ------------------------------------------------------------------ tiles    /// The tile level to draw. Rounding up means a tile is never stretched in
    /// device pixels (a little magnified is exactly what blur looks like), and
    /// a dense screen asks for a deeper level to match. It never goes past the
    /// deepest level the basemap serves.
    pub fn tile_zoom(&self, pixels_per_point: f32, max_tile_zoom: u8) -> u8 {
        let density = (pixels_per_point.max(0.5) as f64).log2();
        (self.zoom + density)
            .ceil()
            .clamp(2.0, max_tile_zoom as f64) as u8
    }

    pub fn tile_scale(&self, tile_zoom: u8) -> f64 {
        (self.zoom - tile_zoom as f64).exp2()
    }

    /// Inclusive range of raw tile indices covering `rect` at `tile_zoom`.
    /// X may be negative or beyond the world; callers wrap it with `rem_euclid`.
    pub fn visible_tile_range(&self, rect: Rect, tile_zoom: u8) -> (i64, i64, i64, i64) {
        let (cx, cy) = self.center_world_at(tile_zoom as f64);
        let scale = self.tile_scale(tile_zoom);
        let half_width = rect.width() as f64 * 0.5 / scale;
        let half_height = rect.height() as f64 * 0.5 / scale;
        let count = 1i64 << tile_zoom;

        let x0 = ((cx - half_width) / TILE_SIZE).floor() as i64;
        let x1 = ((cx + half_width) / TILE_SIZE).floor() as i64;
        let y0 = (((cy - half_height) / TILE_SIZE).floor() as i64).clamp(0, count - 1);
        let y1 = (((cy + half_height) / TILE_SIZE).floor() as i64).clamp(0, count - 1);
        (x0, x1, y0, y1)
    }

    pub fn tile_screen_rect(&self, tile_zoom: u8, x: i64, y: i64, rect: Rect) -> Rect {
        let (cx, cy) = self.center_world_at(tile_zoom as f64);
        let scale = self.tile_scale(tile_zoom);
        let left = rect.center().x as f64 + (x as f64 * TILE_SIZE - cx) * scale;
        let top = rect.center().y as f64 + (y as f64 * TILE_SIZE - cy) * scale;
        let size = TILE_SIZE * scale;
        Rect::from_min_size(
            Pos2::new(left as f32, top as f32),
            Vec2::new(size as f32, size as f32),
        )
    }
}

fn lon_fraction(lon: f64) -> f64 {
    (lon + 180.0) / 360.0
}

fn lat_fraction(lat: f64) -> f64 {
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    0.5 - (lat.tan() + 1.0 / lat.cos()).ln() / (2.0 * PI)
}

fn unproject_lat(y: f64) -> f64 {
    (PI * (1.0 - 2.0 * y)).sinh().atan().to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))
    }

    #[test]
    fn mercator_round_trips() {
        let view = MapView {
            lon: 2.35,
            lat: 48.86,
            zoom: 14.0,
        };
        for pos in [
            GeoPos::new(2.35, 48.86),
            GeoPos::new(-0.12, 51.5),
            GeoPos::new(-179.9, -84.0),
            GeoPos::new(179.9, 84.0),
        ] {
            let screen = view.world_to_screen(pos, rect());
            let back = view.screen_to_world(screen, rect());
            assert!((back.lon - pos.lon).abs() < 0.001, "{back:?} vs {pos:?}");
            assert!((back.lat - pos.lat).abs() < 0.001, "{back:?} vs {pos:?}");
        }
    }

    #[test]
    fn zoom_keeps_cursor_anchor() {
        let mut view = MapView {
            zoom: 10.0,
            ..MapView::default()
        };
        let cursor = pos2(930.0, 240.0);
        let before = view.screen_to_world(cursor, rect());
        view.zoom_at(1.8, cursor, rect(), MAX_ZOOM);
        let after = view.screen_to_world(cursor, rect());
        assert!(
            (before.lon - after.lon).abs() < 0.001,
            "{before:?} {after:?}"
        );
        assert!(
            (before.lat - after.lat).abs() < 0.001,
            "{before:?} {after:?}"
        );
    }

    #[test]
    fn zooming_out_still_covers_the_viewport() {
        let mut view = MapView {
            zoom: MIN_ZOOM,
            ..MapView::default()
        };
        view.clamp_to(rect(), MAX_ZOOM);
        assert!(MapView::world_size_at(view.zoom) >= rect().width() as f64 - 1.0);
        view.zoom_at(0.2, rect().center(), rect(), MAX_ZOOM);
        assert!(MapView::world_size_at(view.zoom) >= rect().width() as f64 - 1.0);

        // The world itself, not just the zoom, must span the viewport.
        let z = view.tile_zoom(1.0, 19);
        let scale = view.tile_scale(z);
        let (cx, _) = view.center_world_at(z as f64);
        let left = rect().center().x as f64 - cx * scale;
        let right = left + MapView::world_size_at(z as f64) * scale;
        assert!(left <= 0.5 && right >= rect().width() as f64 - 0.5);
    }

    #[test]
    fn zooming_in_stops_where_the_basemap_does() {
        let mut view = MapView::default();
        for _ in 0..50 {
            view.zoom_at(4.0, rect().center(), rect(), MAX_ZOOM);
        }
        assert!((view.zoom - MAX_ZOOM).abs() < 1e-9, "{}", view.zoom);

        // A basemap that stops serving detail at 16 takes the camera no further
        // than two levels of magnification past it.
        let mut shallow = MapView::default();
        for _ in 0..50 {
            shallow.zoom_at(4.0, rect().center(), rect(), 18.0);
        }
        assert!((shallow.zoom - 18.0).abs() < 1e-9, "{}", shallow.zoom);
        shallow.clamp_to(rect(), 18.0);
        assert!((shallow.zoom - 18.0).abs() < 1e-9, "{}", shallow.zoom);
    }

    #[test]
    fn the_tile_level_stays_inside_what_the_basemap_serves() {
        let mid = MapView {
            zoom: 12.0,
            ..MapView::default()
        };
        assert_eq!(mid.tile_zoom(1.0, 19), 12);
        // A denser screen asks for a deeper level so tiles are not stretched.
        assert_eq!(mid.tile_zoom(2.0, 19), 13);
        assert!(mid.tile_scale(mid.tile_zoom(2.0, 19)) < 1.0);

        let deep = MapView {
            zoom: 18.0,
            ..MapView::default()
        };
        assert_eq!(deep.tile_zoom(1.0, 16), 16);
        assert_eq!(deep.tile_zoom(2.0, 16), 16);
    }

    #[test]
    fn panning_stays_inside_the_world() {
        let mut view = MapView {
            zoom: 8.0,
            ..MapView::default()
        };
        view.pan_pixels(Vec2::new(1.0e7, 1.0e7), rect());
        assert!(view.lon >= -180.0 && view.lon <= 180.0);
        assert!(view.lat >= -MAX_LAT && view.lat <= MAX_LAT);
        view.pan_pixels(Vec2::new(-1.0e7, -1.0e7), rect());
        assert!(view.lon >= -180.0 && view.lon <= 180.0);
        assert!(view.lat >= -MAX_LAT && view.lat <= MAX_LAT);
    }

    #[test]
    fn fitting_bounds_puts_them_on_screen() {
        let mut view = MapView::default();
        let min = GeoPos::new(-10.0, 40.0);
        let max = GeoPos::new(20.0, 55.0);
        view.fit_bounds(min, max, rect(), MAX_ZOOM);
        for corner in [
            GeoPos::new(min.lon, min.lat),
            GeoPos::new(max.lon, max.lat),
            GeoPos::new(min.lon, max.lat),
            GeoPos::new(max.lon, min.lat),
        ] {
            let screen = view.world_to_screen(corner, rect());
            assert!(rect().expand(1.0).contains(screen), "{screen:?}");
        }
    }

    #[test]
    fn visible_tiles_cover_the_viewport() {
        let view = MapView {
            zoom: 11.4,
            lon: 13.4,
            lat: 52.5,
        };
        let z = view.tile_zoom(1.0, 19);
        let (x0, x1, y0, y1) = view.visible_tile_range(rect(), z);
        assert!(x1 >= x0 && y1 >= y0);
        assert!(x1 - x0 < 20 && y1 - y0 < 20);

        let center = rect().center();
        let covers_x = (x0..=x1).any(|x| {
            let tile = view.tile_screen_rect(z, x, y0, rect());
            tile.x_range().min <= center.x && center.x <= tile.x_range().max
        });
        let covers_y = (y0..=y1).any(|y| {
            let tile = view.tile_screen_rect(z, x0, y, rect());
            tile.y_range().min <= center.y && center.y <= tile.y_range().max
        });
        assert!(covers_x && covers_y);
    }
}
