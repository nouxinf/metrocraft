# Metrocraft

Transit map editor in Rust + egui, drawn on a real basemap. Windows, Linux and
the browser (WASM) from one codebase.

## Build

- After every feature, rebuild the Windows build. It has to compile before the
  work is reported done.

## Git

- Every new feature ends in a commit and a push. Both land before the work is
  reported done.

## UI Principles: Minimal Text, Minimal Elements

The default UI is as small as it can be while still working. Every piece of text
and every element has to justify its presence. When in doubt, leave it out.

### Text

- Use the fewest words that work. One or two words is usually enough for a
  button, label, or heading.
- Do not add explanatory text, descriptions, subtitles, taglines, helper text, or
  instructions unless the user would be unable to proceed without them.
- Do not narrate what the UI does ("Here you can view and manage your...",
  "Click the button below to..."). If the control is self-evident, it needs no
  sentence.
- No welcome messages, intro paragraphs, or marketing-style copy.
- Do not add tooltips, hint text, or placeholder prose to explain obvious
  inputs. A placeholder is at most a few words, and only if the label alone is
  ambiguous.
- Empty states get a short label at most, not a paragraph.
- Error messages state what went wrong and what to do in one short line.
- Prefer icons-with-no-text or text-with-no-icons, never both, unless the
  meaning would be unclear.

### Elements

- Only add a UI element if the feature the user asked for cannot work without
  it.
- Do not add extras on your own initiative: settings screens, toggles, stats or
  summary panels, badges, banners, footers, "about" sections, decorative icons,
  dividers, cards, or placeholder navigation.
- Do not add features, buttons, or screens "for completeness" or "for future
  use".
- Do not add confirmation dialogs, toasts, or modals unless an action is
  destructive or irreversible.
- Do not wrap content in extra containers, headers, or sections just to organize
  it. Use the simplest layout that works.
- Do not add loading text, animations, or progress UI beyond what the wait
  actually requires.
- Do not add dark mode, theming, or responsive variants unless asked.

### Process

- Before finishing, review the UI and remove anything that can go without
  breaking the requested functionality.
- If you think an additional element or explanatory text is truly needed, ask
  the user instead of adding it.
- When asked to change the UI, make the minimal change. Do not add surrounding
  elements or text.

### Animation

Animation is restricted so the app feels fast and smooth. Motion must never make
the UI feel slower or heavier.

- No animation by default. Add motion only when it clarifies a state change the
  user could otherwise miss (an item appearing, a panel opening).
- No decorative animation: no bouncing, pulsing, shimmering, parallax, looping
  effects, or animated backgrounds.
- Keep transitions short: 100-200ms, never over 250ms. Use ease-out for things
  entering and ease-in for things leaving.
- Never block interaction on an animation. The user can tap, type, or navigate
  while one is running, and input responds instantly.
- Animate only `transform` and `opacity`. Do not animate layout properties
  (width, height, margin, top, left) or anything that triggers re-layout.
- No staggered or chained animations. Everything in a screen moves together or
  not at all.
- No page or screen transitions beyond a simple fade or an instant switch.
- Do not animate on first load or first render.
- Do not add skeleton loaders, spinners, or progress animations for waits under
  about 300ms.
- Respect `prefers-reduced-motion` and disable all non-essential motion when it
  is set.
- Do not add an animation library. Use native CSS transitions or the platform's
  built-in animation APIs.
- If the UI stutters or drops frames, remove the animation rather than
  optimizing it.

## The map is the interface

The app opens on the project list, not on a map: one row per map it has kept,
its name over where it lives, `Open…`, `New…` and `Import…` under it, and
`File` → `Projects` back to it from the editor. On the desktop a map is a file
and the row says where; the browser has no files, keeps its maps in IndexedDB
keyed by name, and `Save` puts one back there while `Save as…` hands out a file.
Import takes a MetroDreamin' link
and replaces the document in hand with the map it names, or a Brand New Subway
saved game, from which a line that was never run is left out; given a profile link it
instead writes every map that account has into a folder — the browser's own
store, where there are no files — with a bar counting them, and each one joins
the project list as it lands. A row's pencil, under the
cursor beside its cross, renames that map where it lives, and `File` →
`Rename…` renames the map in hand: the file, or the record, moves with the name,
and a name already taken is refused.

There is no tool palette. A click on bare ground places a node; the bar that
appears next to it offers the nearest lines as colours only, then the loop a
node can take part in, then turns the node into the other kind, then deletes it.
Stations are dots, a station where lines meet is a rhombus, the ends of a line
are rounded squares on the diagonal, waypoints are x's. A station's panel sets
whether it is above, at or below grade, which inks that mark (heavier rim, plain,
or filled dark) and nothing else. The panel on the right names the node and lists every line,
searchable; for a station it first lists the stations within a walk that would
open up a line it does not serve, and for a line it lists the stops in order
with the lines to change to at each. Lines themselves live in one small `Lines`
tab that folds, drags and holds their names, colours, mode, group and loop. A
line's mode is MetroDreamin's list — metro, light rail, tram, commuter,
intercity, high-speed rail, bus rapid transit, bus, ferry, gondola, air — and it
is how the line is stroked: rail solid, road dashed, water and cable as beads,
air in long dashes; a line set to a pattern of its own runs solid, dashed or
dotted whatever it carries. Groups are foldable headings that can keep their lines off
the map. `Edit` →
`Preferences` holds three categories down its left side: `Basemap`, one tile of
the same place per basemap, `Theme` (`Dark`, `Light`), and `Keys`, one row per
command with the key it answers to and a `Defaults` button under it. `Tools` →
`Network stats` puts up the count of stations, waypoints and lines and the
length they run to. Nothing else is on screen but attribution.

On Windows the menu bar is the window's own, drawn above the client area, and
follows the app's theme; everywhere else — the browser included — the app draws
it. The two bars hold the same commands, each with the key `Preferences` →
`Keys` gives it beside it, so a command belongs in both, and the window's one
reads what it is to enable, tick and spell from the app each frame. Both
`File` menus also list `Open Recent`, the maps the project list remembers, most
recent first, so a map reopens without the list, and `Clear Recent` empties that
list and leaves the maps where they are — except in the browser, whose list is
the maps, so there it has no such item.

There is no performance switch to find. Marks thin themselves as the map is
zoomed out: stops, then bends, then names are left off once the network's nodes
would sit closer than a mark can read, while stations where lines meet or end
always stay. On a map of fifty thousand nodes this is the difference between a
frame and a crawl, so keep per-frame work proportional to what is on screen:
build node roles in one pass and never ask the lines about each node, stroke
only the stretches of a route that cross the viewport, and drop bends smaller
than the stroke is wide.
