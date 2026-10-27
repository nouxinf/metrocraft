# Metrocraft

A transit map editor that draws on a real basemap. Place stations and waypoints
on the map, then attach them to lines. A waypoint bends a line without becoming a
stop.

Rust + egui. One codebase for the desktop (Windows, Linux) and the browser
(WASM).

## Using it

The app opens on its project list: one row per map it has opened before, the
name over the path it lives at, newest first, with `Open…` and `New…` under it.
`Open…` takes a map from anywhere on disk, `New…` asks where a map should live
and writes an empty one there, and the cross on a row takes it off the list.
Every map that is opened or saved is added, up to twelve, and `File` →
`Projects` goes back to the list from the editor. The list is the desktop
build's; the browser has no filesystem and opens straight into the editor.

There are no tools to pick. A click on bare ground places a node, and a bar
appears beside it.

| Action | How |
| --- | --- |
| Choose a map | the launch list; `Open…` for any file, `New…` to make one |
| Back to that list | `File` → `Projects` |
| Place a station | click bare ground |
| Attach to a line | click its colour in the bar, or its row in the panel |
| Preview the join | hover a colour or row; the line is drawn where the node would go |
| Make it a waypoint, or a station again | the arrow in the bar |
| Delete a node | the bin in the bar, or `Delete` |
| Hide waypoint marks | `View` → `Waypoints` (ticked) |
| Change the basemap | `Ctrl+,` or `Edit` → `Preferences…` → `Basemap`; pick one of the previews |
| Change a key | `Preferences` → `Keys`; click a row, press the new key (`Esc` backs out) |
| Close Preferences | `Esc` or its × |
| New line | `+` in the `Lines` box |
| Rename or recolour a line | open `Lines`, pick the line; the colour well opens a picker, the field takes a hex code |
| Delete a line | the bin beside its name in `Lines` |
| Move the `Lines` box | drag it by its header |
| Select / move | click a node or line; drag a node to move it; drag bare ground to pan |
| Undo / redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Fit map to points | `F` |
| Save / open / new | `Ctrl+S`, `Ctrl+Shift+S`, `Ctrl+O`, `Ctrl+N`, or the `File` menu (browser: downloads and uploads a `.json` map) |

The bar holds up to three squares, one per line nearest the node, in that line's
colour and outlined once the node is on it. Stations are dots; waypoints are x's.
The arrow turns the node into the other kind and also sets what the next clicks
place, so the mode always describes the node in hand. Lines stay attached
through a conversion.

The `Lines` box starts folded to a tab in the top-left corner; open it with the
chevron, drag it anywhere by its header, and use it to create lines and edit
their names and colours. The panel on the right is for the selected node alone.

`Preferences` holds the settings, split into categories listed down its left
side. `Basemap` shows every basemap as a tile of the same place, so the styles
can be compared at a glance, with the one in use outlined. `Keys` lists the
commands that have a key — undo, redo, save, save as, open, new, fit and this
window — and the key at the right of each row; clicking a row takes the next key
pressed, keeping only the command that claimed it last. `Defaults` under the
rows puts them all back. `Delete` keeps the standard editing key, so it is not
in the list.

The document, camera and basemap are remembered between sessions.

## Build

### Desktop

```sh
cargo run --release
```

Produces a single executable; on Linux install the usual X11/Wayland dev
packages first (`libxkbcommon`, `libwayland-dev`, `libx11-dev`).

### Web

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk serve --release      # http://127.0.0.1:8080
trunk build --release      # static site in dist/
```

`dist/` needs to be served over HTTP (wasm will not load from `file://`).

## Notes

- Ten basemaps, all of them key-less raster services: OpenStreetMap, its
  Humanitarian and France renders, OpenTopoMap, and Esri's Streets,
  Topographic, Relief, satellite imagery and two canvas styles. The credit each
  provider asks for is drawn in the corner.
- The tile level rounds up, so a tile is never stretched in device pixels, and a
  dense screen asks for a deeper level. Each basemap stops the camera a level
  past the deepest detail it serves.
- A wheel notch zooms by 0.32 levels on every platform.
- Tiles are fetched off the UI thread, decoded before they reach it, cached per
  basemap and pruned, so panning and zooming stay cheap. The middle of the view
  fills in first.
