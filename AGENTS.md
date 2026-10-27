# Metrocraft

Transit map editor in Rust + egui, drawn on a real basemap. Windows, Linux and
the browser (WASM) from one codebase.

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

On the desktop the app opens on the project list, not on a map: one row per map
it has opened, its name over the path it lives at, `Open…` and `New…` under it,
and `File` → `Projects` back to it from the editor. The browser has no
filesystem and goes straight into the editor.

There is no tool palette. A click on bare ground places a node; the bar that
appears next to it offers the nearest lines as colours only, then turns the node
into the other kind, then deletes it. Stations are dots, waypoints are x's. The
panel on the right names the node and lists every line, searchable. Lines
themselves live in one small `Lines` tab that folds, drags and holds their names
and colours. `Edit` → `Preferences` holds two categories down its left side:
`Basemap`, one tile of the same place per basemap, and `Keys`, one row per
command with the key it answers to and a `Defaults` button under it. Nothing
else is on screen but attribution.
