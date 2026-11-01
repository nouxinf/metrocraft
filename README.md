# Metrocraft

A transit map editor that draws on a real basemap. Place stations and waypoints
on the map, then attach them to lines. A waypoint bends a line without becoming a
stop.

Rust + egui. One codebase for the desktop (Windows, Linux) and the browser
(WASM).

## Using it

The app opens on its project list: one row per map it has kept, the name over
where that map lives, newest first, with `Open…`, `New…` and `Import…` under it.
`Open…` takes a map file from anywhere, `New…` starts an empty one, `Import…`
reads a map published on MetroDreamin' — or, given a profile link, saves every
map that account has into a folder of your choosing, or into the browser's own
store where there are no folders, all of them joining the list as they land —
and the cross on a row takes it off the list.
`File` → `Projects` goes back to the list from the editor.

On the desktop a map is a file you choose, and each row says where that file is.
The browser has no files: its maps live in the browser's own database, so `Save`
keeps the map there and `Save as…` hands out a `.json` copy.

There are no tools to pick. A click on bare ground places a node, and a bar
appears beside it.

| Action | How |
| --- | --- |
| Choose a map | the launch list; `Open…` for any file, `New…` to make one |
| Back to that list | `File` → `Projects` |
| Read a MetroDreamin' map | `File` → `Import from MetroDreamin'…`, then paste the map's link |
| Save every map of an account | the same dialog, given the account's profile link |
| Read a Brand New Subway game | `File` → `Import from Brand New Subway…`, then pick the saved game |
| Place a station | click bare ground |
| Attach to a line | click its colour in the bar, or its row in the panel |
| Preview the join | hover a colour or row; the line is drawn where the node would go |
| Make it a waypoint, or a station again | the arrow in the bar |
| Delete a node | the bin in the bar, or the bin at the foot of the panel |
| Hide waypoint marks | `View` → `Waypoints` (ticked) |
| Set a station above, at or below grade | click the station, then the grade under its name |
| Walk between two stations | click the station, then the other station's row under its name |
| Shape a walk | select a waypoint near it, then its white square in the bar |
| Loop a line | the loop button in the bar, then the line |
| Turnback or crossing | `Lines` → the line → `Loop` |
| A line's stops and changes | click the line; the panel lists each stop and the lines there |
| File a line under a group | `Lines` → the line → `Group` → `New group`, or an existing one |
| Fold a group | the fold at its heading in `Lines` |
| Hide a group's lines | click its heading, then the eye beside its name |
| Rename or delete a group | click its heading in `Lines` |
| Light or dark | `Preferences` → `Theme` |
| Counts and total length | `Tools` → `Network stats` |
| Change the basemap | `Ctrl+,` or `Edit` → `Preferences…` → `Basemap`; pick one of the previews |
| Change a key | `Preferences` → `Keys`; click a row, press the new key (`Esc` backs out) |
| Close Preferences | `Esc` or its × |
| New line | `+` in the `Lines` box |
| Rename or recolour a line | open `Lines`, pick the line; the colour well opens a picker, the field takes a hex code |
| Change what a line carries | the mode under its colour: metro, light rail, tram, commuter rail, intercity rail, high-speed rail, bus rapid transit, bus, ferry, gondola or air |
| Set how a line is stroked | the pattern under its mode: Auto, Solid, Dashed or a shape |
| Delete a line | the bin beside its name in `Lines` |
| Move the `Lines` box | drag it by its header |
| Select / move | click a node or line; drag a node to move it; drag bare ground to pan |
| Undo / redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Fit map to points | `F` |
| Save / open / new | `Ctrl+S`, `Ctrl+Shift+S`, `Ctrl+O`, `Ctrl+N`, or the `File` menu (`Save` keeps the map; `Save as…` writes a file) |

The bar holds up to three squares, one per line nearest the node, in that line's
colour and outlined once the node is on it. Stations are dots; waypoints are x's;
a station where two or more lines meet is a rhombus, larger than a dot and sharp
where the ends of lines are soft; the two ends of a line are rounded squares
turned onto their diagonal, so a terminus never reads as a stop passed through.
Where a node is both, the rhombus wins: lines pass through it.

A station's height is set in its panel, from the grade under its name: `Above
grade` stations carry a heavier rim, `Below grade` ones are filled dark, and
`At grade` is the plain white mark. A station's height changes its mark only,
never its route. The arrow turns the node into the
other kind and also sets what the next clicks place, so the mode always
describes the node in hand. Lines stay attached through a conversion.

A station's panel lists the stations within half a mile on foot that are on a
line it does not serve, each with the walk time and the colours of the lines the
walk opens up; clicking one links the two as a walking interchange, drawn as a
white band in a thin dark outline. Walks already made sit at the top of that list, with
the same band for a mark; clicking one opens it, to shape or delete it. The
station's lines are listed below, and the ones only reached on foot carry a
two-dot mark. A waypoint near a walk gets a white square in its bar, which hangs
the waypoint on the walk so its path can be bent. Walks are picked out before
lines when clicked.

The loop button in the bar cuts the line at that node and joins it back to where
it started, so the route runs as a circle through it. `Lines` → the line →
`Loop` shows how it closes: `Turnback` doubles back beside the last stretch,
`Crossing` joins straight across, `Off` opens the line again.

Each line carries a mode, from `Metro` to `Air`. Rail runs solid, road runs
dashed, water and cable run as a chain of beads, and air runs in long dashes, so
a map says what each line is without a legend. A pattern set by hand overrides
that: `Solid`, `Dashed` or a shape runs that way whatever the line carries, and
`Auto` leaves it to the mode.

The `Lines` box starts folded to a tab in the top-left corner; open it with the
chevron, drag it anywhere by its header, and use it to create lines and edit
their names, colours, mode, group and loop. Groups are headings in that list: `New
group` in a line's editor makes one and files the line under it; the fold at a
heading opens or shuts it, and clicking the heading renames it, hides its lines
from the map, or deletes the group itself. The panel on the right is for the
selected node or line.

`Preferences` holds the settings, split into categories listed down its left
side. `Basemap` shows every basemap as a tile of the same place, so the styles
can be compared at a glance, with the one in use outlined. `Theme` is the pair
`Dark` and `Light`. `Keys` lists the
commands that have a key — undo, redo, save, save as, open, new, fit and this
window — and the key at the right of each row; clicking a row takes the next key
pressed, keeping only the command that claimed it last. `Defaults` under the
rows puts them all back. `Delete` keeps the standard editing key, so it is not
in the list.

`Tools` → `Network stats` puts up a small box, which can be dragged, with the
number of stations, waypoints and lines and the length of every line added
together. Clicking a line shows its stops down a rail in its own colour, in
travel order, with the other lines that call at each of them as colour chips at
the right; clicking a stop selects it.

The document, camera, basemap, theme and keys are remembered between sessions.

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
- Importing reads the same public Firestore a MetroDreamin' map is served from,
  so a link is all it needs — an unlisted (`private`) map included, since that
  flag keeps a map out of their browse pages rather than behind a lock. A large network is spread over several
  documents in a `partitions` subcollection; every partition is read and joined
  before the map is built, which is what other exporters miss.
- One account's maps are found in one query, since every map carries the
  account it belongs to, and the answer includes unlisted maps. If the database
  ever refuses to be queried, the reader walks the account's map numbers from
  zero instead — reading each map by id until forty misses in a row say there
  are no more — and also stops at a thousand. Saved maps are named after their
  titles, and a name already taken by a map in the folder — or in the browser's
  store — gains ` (2)` rather than being overwritten. The browser runs that
  same query, and each map lands in its store as a record.
- In the browser a map is a record in an IndexedDB store keyed by its name, with
  the map's JSON as the value: saving puts it back under the same name, so
  saving again replaces the map instead of piling up copies, and a map holds as
  much as a fifty-thousand-node network needs without running into a storage
  limit. A new map is named around the ones already there, so `Untitled` never
  lands on top of a saved map. Requests run off the drawing thread and report
  back when they answer. Stations and
  waypoints keep their place and grade, lines their colour, route, mode, group
  and loop, and walking links their two stations. Per-line waypoint overrides
  have no counterpart here and are left behind.
- A wheel notch zooms by 0.32 levels on every platform.
- Tiles are fetched off the UI thread, decoded before they reach it, cached per
  basemap and pruned, so panning and zooming stay cheap. The middle of the view
  fills in first.
- A map of fifty thousand nodes costs the marks it can show, not the marks it
  has. Node marks are left off while the network's own spacing on screen is too
  tight to tell one from the next — plain stops first, then bends, then names —
  and the stations that hold the shape of the network (where lines meet or end)
  always stay; zoom in and they come back. Routes are stroked only where they
  cross the viewport, and bends too slight to read inside the stroke's own width
  are dropped before the stroke is built. So the whole network still draws at
  about a hundredth of the cost it did when every node was drawn at once.
