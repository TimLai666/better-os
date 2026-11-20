# Ticket 147 — Full-colour icons inside every window

**Blocked by:** none

## Why

On 2026-11-21 the owner said the single-colour line icons inside Better OS
windows are ugly: they are Lucide's, the set Zed uses, which `gpui-component`
brings with it. Shown three treatments of the Better Files sidebar side by
side (`design/design-demos/icon-directions.html`), the owner chose filled,
full-colour pictures. DESIGN.md's Icons section now holds the rules; this
ticket draws the icons and puts them in every window.

The pictures in Better Files' content area (folders and file kinds from the
desktop's icon theme) stay as they are; the owner said those are fine.

## What exists today

- 79 distinct `IconName` values from the toolkit are used across the windows,
  and 30 more line icons are drawn in this repository
  (`crates/better-ui/src/fluent/glyphs.rs`, `crates/files-gui/src/icons.rs`,
  and the other windows' asset sources).
- The toolkit's `Icon` paints an SVG as a mask in the text colour, so it
  cannot show a coloured picture. A coloured one is drawn with `img(path)`
  from the window's `AssetSource`, as Better Files already does for the
  developer folder (`crates/files-gui/src/views.rs`, `CODE_FOLDER_SMALL`).

## What to build

1. **One icon set in `better-ui`.** A module that owns every icon by name,
   says for each whether it is a picture (an object or an action icon) or a
   plain symbol, serves its SVG, and gives one element to draw it at a size
   (pictures through `img`, plain symbols as today). The per-window copies of line icons move
   into it; no window keeps its own.
2. **Sort every icon in use** into object, action, or plain symbol (arrows,
   chevrons, close, check, window buttons), by what it stands for at each call
   site. The same toolkit name used for a thing in one place and an action in
   another becomes two names.
3. **Draw the object icons** to DESIGN.md's rules: Better Files' places and
   devices (Home, Desktop, Documents, Downloads, Pictures, Music, Videos,
   Templates, Public, Developer, Trash, This PC, system drive, external drive,
   memory card, USB drive, disc, phone, network), and the things the other
   windows show (folder, file, drive, settings, terminal, globe, battery
   levels, CPU, memory, user, bell, palette, calendar, star, and the rest the
   sorting finds). The nine in the demo are a starting point, not finished
   work: Documents is too pale on the light background.
4. **Draw the action icons** as flat filled shapes, plainer than the object
   icons (owner, 2026-11-21: a line icon with one coloured part "looks odd",
   and a shaded, detailed picture is too realistic for a button). The demo
   has five: new folder, copy, rename, delete, refresh. Plain symbols stay
   line icons.
5. **Every window moved over**: Files, Git, Monitor, Manager and the control
   center pages, Launcher, Awake, Touchpad, the app chooser, and the Fluent
   gallery, which gains a page showing every icon at 16px, 20px, and 40px in
   both appearances.
6. Tray icons and the application icons under `packaging/icons` are out of
   scope.

## Done when

- A test lists every icon name the windows use and fails when one has no
  drawing in the set, and another fails when a window still names a toolkit
  line icon for something sorted as an object.
- A test checks each object icon's SVG: 24px grid, no stroke-only drawing, at
  most four fill colours besides gradient stops.
- Screens of the gallery page and of each window, light and dark, with the
  owner's go-ahead on the drawings.
- `design/design-demos/icon-directions.html` is removed or replaced by the
  gallery page as the reference DESIGN.md points to.
