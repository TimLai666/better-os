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
4. **Draw the action icons** in two layers of one colour that the window
   gives them, as DESIGN.md's Icons section says: the body in the full
   colour, a minor part in a lighter shade, the layers never overlapping, a
   mark on a shape cut out of it. Alone in a toolbar an action icon is the
   accent at 18px; beside a text label it is the secondary text colour at
   16px; a destructive one is the critical colour in both (owner,
   2026-11-21: a line icon with one coloured part "looks odd", a shaded
   picture is too realistic, and a flat shape in its own bright colour is
   "too childish"). `design/design-demos/action-icon-options.html` holds the
   five the owner approved as drawn: new folder, copy, rename, delete,
   refresh. Plain symbols stay line icons.
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

## Notes

### Stage 1 (2026-11-21): the set and its drawings

Done in this stage, not yet in any window:

- `better_ui::icon_set` (`crates/better-ui/src/icon_set.rs`). `better_ui::icons`
  was already taken by the desktop icon-theme lookup that five crates use, so
  the set has its own name. `Picture` names every picture icon with its
  `kind()` (`Kind::Object` or `Kind::Action`), `path()` (served as
  `better-ui/icons/<kind>/<file>.svg`) and `svg()`. `icon_set::load(path)` is
  what a window's `AssetSource` calls before the kit's assets, as
  `fluent::glyphs::load` is. `icon_set::picture(Picture, size)` draws an
  object through `img`, and `picture_disabled` draws it at 36%. The data half
  (`icon_set/catalog.rs`) has no GPUI in it.
- 86 object drawings under `crates/better-ui/assets/icons/object/`.
  Documents is redrawn darker so it shows on `#f3f3f3`.
- Tests in `icon_set/tests.rs` check every file against DESIGN.md's rules
  (an action layer may hold only the mask colours and no gradient), and
  `icon_set::renderer_tests` renders every file through GPUI's own
  `SvgRenderer` and checks the action colours per placement. Neither yet checks which icons the windows use; that test
  belongs to stage 2, when the windows name pictures.
- `design/design-demos/icon-set.html` shows every file at 16, 20 and 40px
  with a light/dark switch. The gallery has an Icons page
  (`better-fluent-gallery --page icons`).
- Stage 1b (owner, 2026-11-21, after seeing the set): the objects were
  approved; the flat multi-colour actions were rejected as "too childish"
  and redrawn to the two-layer rule (item 4). How it is built:
  - Each action is one or two mask files: `action/<file>.svg` is the body,
    `action/<file>.2.svg` the minor part (22 of the 39 have one). They are
    drawn in black and carry no colour; a cut-out mark is an `evenodd` hole
    or a `<mask>`. `Picture::second_layer()` gives the second file;
    `load` serves both.
  - `icon_set::action(Picture, Placement, &Palette, disabled)` stacks the
    two layers as `svg()` elements, each tinted with `text_color`.
    `Placement::Alone` is the accent at 18px, `Placement::Labelled` the
    quiet grey at 16px; `Picture::destructive()` (Delete, EndTask) is the
    critical colour in both. `action_sized` takes another size. Disabled is
    the whole icon at 36%. GPUI renders an `svg()` through resvg at the
    element's own size and keeps only alpha, so the masks, holes and
    `<mask>` cut-outs all come through; seen in the gallery in a headless
    GNOME Shell, light and dark.
  - The palette gained `icon_accent_2`, `icon_critical_2`, `icon_quiet` and
    `icon_quiet_2`. The quiet grey is the demo's (`#77736c`, `#a9a59d`).
    Every lighter shade comes from one rule, `fluent::second_layer`: the
    body mixed toward the window background, 55% in light and 40% in dark.
    On the demo's colours it gives `#a0b8f2` for `#a3bdfa` and `#72706b`
    for `#6b6862`; the demo's dark blue shade is more saturated
    (`#5673c4`, the rule gives `#5d72a6`). `with_accent`
    applies it to a desktop accent, so the shade follows the accent in
    force. Checked with the default accent and GNOME purple (`#9141ac`) in
    both appearances in the contact sheet. A dark accent on the dark
    appearance leaves the lighter layer dim, because the accent itself is
    near the background there.
  - The five approved drawings are used as drawn, except that the plus on
    new folder and the slots on delete are cut out of the shape (holes),
    where the demo painted them over it.
  - Redrawn objects: Click is a mouse with its left button pressed, Disc an
    iridescent face with a silver hub, Fixes a broad wrench crossed over a
    screwdriver, Dashboard a card with a ring chart and two bars (Apps keeps
    the four tiles), Scrolling a page with its scroll bar.
  - `design/design-demos/icon-set.html` now embeds every file (data URIs,
    actions as CSS masks), so it works opened alone, and has light/dark and
    default/purple accent switches. It is generated by a script kept out of
    the repository, from the files on disk.
- Not done in stage 1: the per-window copies of line icons
  (`files-gui/src/icons.rs`, Better Git's three, `fluent::glyphs`) are still
  in place. The tables below say what replaces each one.

### Stage 2 (2026-11-21): every window moved over

Done, one commit per crate after its checks passed:

- The kit takes pictures. `better_ui::icon_set::Glyph` is a toolkit
  symbol, a window's own SVG path, a built toolkit `Icon`, or a `Picture`,
  and `Glyph::element` draws it as the control places it. These now take
  `impl Into<Glyph>` where they took `IconName` or `Icon`, so every older
  caller compiles unchanged: `Command::new`, `MenuItem::icon`,
  `MenuEntry::Submenu.icon`, `Button::icon` and `Button::icon_button`,
  `DropDownButton::icon`, `NavItem::new` and `NavItem::emblem`,
  `NavAction::new`, `SettingsCard::icon`, `SettingsExpander::icon`,
  `DeviceBanner::new`, `ListItem::icon`, `ListRow::glyph`, `Tab::icon`,
  `SelectorItem::icon`, `Chip::icon`. The public `icon_path` fields of
  `Command`, `MenuItem`, `NavItem` and `Tab` are gone; their `icon_path`
  setters set a `Glyph::Path`. A `Button` places an action alone without a
  label, beside its label with one, and in the new `Placement::OnAccent`
  (text on accent and a shade of it) on an accent fill.
- Files, Git, Monitor, Manager with the control center pages, Launcher,
  Awake, Touchpad, the app chooser, the gallery, and the info bar's copy
  button draw the sorted pictures. Files' place and device line glyphs,
  Git's branch, worktree and tag glyphs, and `fluent::glyphs::REFRESH` are
  gone; `fluent::glyphs` keeps only `WRAP`, which Git still draws. Files
  keeps the developer folder's two content-area glyphs (`CODE_MARK`,
  `CODE_FOLDER_SMALL`), as the content area keeps the desktop theme's
  pictures.
- Every window that draws a picture serves `icon_set::load` from its own
  asset source. Manager, Launcher, Touchpad and the app chooser had none
  (they handed GPUI the toolkit's assets), so each got one.
- `crates/better-ui/tests/window_icons.rs` holds the two tests "Done when"
  asks for. Both read the window crates' source each run (every `*-gui`
  crate, the gallery and `better-ui`): the first collects every
  `Picture::Name` and checks it is drawn and served with both layers, and
  that each window naming a picture serves `icon_set::load`; the second
  fails on any `IconName` outside DESIGN.md's plain symbols, the status
  marks and the marks kept on purpose (GitHub's mark, a text box's reveal
  and find marks, the hyperlink's mark, Git's expand, Undo and Redo). Both
  were seen to fail on a planted fault.

Not moved at stage 2, and moved in the review round below: Better Awake's
eight and Better Touchpad's seven sidebar pages.

Seen on screen: a headless GNOME Shell on a private bus with a throwaway
home, under the 4G scope and the no-op `ibus-daemon`, screens in
`/tmp/claude-1000/t147-screens/`. Files, Git, Monitor, Launcher and
Touchpad were seen light and dark; Manager and Awake only dark (Manager did
not take the throwaway state file's theme, and Awake follows only the
desktop, which a headless shell does not set). Not seen: the app chooser
(its binary was not built and the disk was below the 8 GB floor), the
gallery after its own move (its binary predates that commit, same reason),
and every window's deeper pages and menus.

Still to do for "Done when": DESIGN.md's Icons section points to
`design/design-demos/icon-directions.html`; once it points to the gallery's
Icons page and `design/design-demos/action-icon-options.html` instead,
`icon-directions.html` can go. The owner's go-ahead on the screens is owed.

### Review round (2026-11-21): two reviews' findings fixed

- The Files preview window started GPUI with the toolkit's assets alone,
  so its pictures drew nothing; it serves `FilesAssets`.
  `window_icons.rs` now checks every `application()` start and every
  `with_assets(...)` call: the type handed to GPUI must have an
  `AssetSource` impl in its crate that calls `icon_set::load`.
- A checked command-bar toggle and an accent button both place their icon
  with `Placement::for_control`, so either is `OnAccent`; a disabled
  `OnAccent` icon is `text_on_accent_disabled`.
- `Glyph::element` draws an action at the slot it is given;
  `Glyph::slot` gives an action alone its 18px. (Chosen over refusing the
  combination in the type: the kit's 12 to 48px slots are all real.)
- GPUI rasterises an `img` SVG once, at twice the size the file declares.
  Above 24px an object is drawn from a large copy whose header declares
  64px (same drawing and viewBox), served by `icon_set::load` under
  `better-ui/icons/object/large/`. A test renders through GPUI's renderer
  and fails on the old behaviour.
- Better Awake's action buttons: a labelled one takes the action through
  gpui-component's own icon slot (the action's mask in its placement's
  colour) so its words stay in `label`, the accessible name; a disabled
  one dims its icon; one with only an icon is made square at 18px.
- Better Awake and Better Touchpad use the kit's `NavigationView` with a
  picture per page, as Better Manager does; no accent tile behind a
  picture; the pane's own collapse button overrides the width rule until
  pressed again. Awake's keyboard hint and Touchpad's session line moved
  from the old sidebar footers to the top bars. Awake's "Session defaults"
  shows `Checklist` so the pane has one gear only.
- Drawings: new-file's fold has a gap; download, open-external and
  show-pane are one full-colour layer whose parts are separated by gaps;
  view-list's lines are its body; battery (five), file and pointer are
  filled shapes; dashboard, pictures, palette, scrolling, startup, fixes
  and battery-charging keep to four colours with strokes counted. A new
  two-layer action, `open-terminal`, for Run in terminal and Open
  terminal here.
- Tests: an object's colours count fills, strokes and mask contents; a
  two-layer action's second layer paints less than its body and never
  overlaps it at 96px; `window_icons.rs` fails on `IconName as`, a glob
  import of the toolkit (across lines), and a kit SVG named by path, and
  has no sidebar exemption. Nine glob imports were replaced by named ones.

Seen on screen in this round (a headless GNOME Shell on a private bus, a
throwaway home, each window in its own 4G scope, input through the shell's
own Eval in unsafe mode; screens in `/tmp/claude-1000/t147-screens/round2/`):
the Files preview playing a tone, Files with the preview pane on, its `…`
and New menus and a context menu, Monitor's Details with tree view on and
a process menu, Awake's navigation, Quick Sessions and a rule draft with
its disabled remove (light and dark, set through Awake's own Appearance
setting), Touchpad's navigation, Manager in light (its state file in the
throwaway home), the app chooser, the gallery's Icons page in both
appearances, and Launcher's 64px folder preview. Two faults the screens
found are fixed: Awake's primary buttons drew their icon in the kit's
on-accent colour on gpui-component's own primary fill (dark on black in
light), and Touchpad's wrapping top bar stretched over its page.

Open risk, not fixed here: an action alone is drawn in the desktop's
accent. In this round's sandbox the accent the windows took was `#bde6fb`
(the Files sidebar's selection bar), and accent-coloured icons alone all
but vanish on the light background (the gallery's Icons page shows it). In
the stage 2 run, before the latest merge of `main`, the same Files reload
icon was a saturated blue. Which accent the owner's desktop gives, and
whether an icon should use a darker accent role, is for the owner to see.

Left as they are on purpose: arrows that name a command (Undo, Redo,
move up and down, move to front, lower priority), Git's expand and wrap
marks, and Git's toolbar Terminal button, which opens the Terminal
application as its Editor and Files neighbours open theirs.

### Stage 2 work list: every icon the windows name

How completeness was checked: `rg -o 'IconName::[A-Za-z0-9_]+' crates`
finds 526 uses of 79 names in 55 files (the ticket's 79 names). A script
assigned each of the 526 to exactly one row below and failed on any use left
over or matched twice. Locally drawn glyphs were found with
`grep -rn '\.svg"' crates --include=*.rs` and the four `impl AssetSource`
blocks (Files, Git, Monitor, Awake; the other windows serve only the kit's
assets).

Kinds: object 274 uses, action 127, plain symbol 88, status mark 32 (the
status colour carries it; DESIGN.md keeps info-bar icons as they are), test
fixture 5. "stays" means the toolkit's line icon remains. Sites are
`crate/file:line` under `crates/`, with `src/` and `.rs` left out.

| Toolkit name | Meaning there (uses): sites | Kind | Replaced by |
|---|---|---|---|
| `IconName::ALargeSmall` | Git settings nav: keyboard shortcuts (1): git-gui/render:4345 | object | `Keyboard` |
| `IconName::ALargeSmall` | Files command: Rename (1): files-gui/shell:467 | action | `Rename` |
| `IconName::ALargeSmall` | Gallery nav: Text page (1): fluent-gallery/main:150 | object | `Font` |
| `IconName::ArrowDown` | move an item down (Awake presets, rules; gesture profiles) (3): awake-gui/pages_status:457, awake-gui/pages_rules:289, manager-gui/pages/touchpad/render_gestures:806 | symbol | stays |
| `IconName::ArrowDown` | Manager: updates (check-updates card, Updates page, available version, update summary) (4): manager-gui/render:403, manager-gui/render_apps:335, manager-gui/render_apps:679, manager-gui/render_apps:818 | object | `Updates` |
| `IconName::ArrowDown` | Monitor menu: lower priority (1): monitor-gui/render:620 | symbol | stays |
| `IconName::ArrowDown` | Git: Pull (sync button, menu) (2): git-gui/render:631, git-gui/render:781 | action | `Pull` |
| `IconName::ArrowLeft` | back / move to front (4): better-ui/fluent/chrome:487, git-gui/render:4598, files-gui/shell:101, launcher-gui/overlay:731 | symbol | stays |
| `IconName::ArrowRight` | Files: forward (1): files-gui/shell:108 | symbol | stays |
| `IconName::ArrowRight` | Files command: file operations panel (1): files-gui/shell:507 | object | `Transfers` |
| `IconName::ArrowUp` | move an item up (Awake presets, rules; gesture profiles) (3): awake-gui/pages_status:448, awake-gui/pages_rules:280, manager-gui/pages/touchpad/render_gestures:800 | symbol | stays |
| `IconName::ArrowUp` | Git: Publish / Push (sync button, menu) (4): git-gui/render:626, git-gui/render:628, git-gui/render:638, git-gui/render:785 | action | `Push` |
| `IconName::ArrowUp` | Git: unpushed-commit marker in a row (1): git-gui/render:2406 | symbol | stays |
| `IconName::ArrowUp` | Files: up one folder (1): files-gui/shell:118 | symbol | stays |
| `IconName::ArrowUp` | Files sidebar: eject a device (2): files-gui/shell:1071, files-gui/shell:1101 | action | `Eject` |
| `IconName::Battery` | battery category / level unknown (Awake page, System card, Monitor) (6): awake-gui/shell:67, manager-gui/pages/system/render:256, manager-gui/pages/system/render:268, monitor-gui/popup:95, monitor-gui/pages:477, monitor-gui/pages:494 | object | `Battery` |
| `IconName::BatteryCharging` | battery charging (2): manager-gui/render:163, manager-gui/pages/system/render:296 | object | `BatteryCharging` |
| `IconName::BatteryFull` | battery level >= 90% (2): manager-gui/render:164, manager-gui/pages/system/render:265 | object | `BatteryFull` |
| `IconName::BatteryFull` | System: power mode card (3): manager-gui/pages/system/render:154, manager-gui/pages/system/render:170, manager-gui/pages/system/render:624 | object | `Gauge` |
| `IconName::BatteryLow` | battery low (3): manager-gui/render:166, manager-gui/pages/system/render:267, manager-gui/pages/keep_awake/render:515 | object | `BatteryLow` |
| `IconName::BatteryMedium` | battery level 40-89% / charge limit / Awake battery card (5): manager-gui/render:165, manager-gui/pages/system/render:204, manager-gui/pages/system/render:232, manager-gui/pages/system/render:266, manager-gui/pages/keep_awake/render:584 | object | `BatteryMedium` |
| `IconName::Bell` | notifications / window attention / events (5): manager-gui/render_window_attention:147, manager-gui/shell:33, monitor-gui/render:45, fluent-gallery/main:2359, fluent-gallery/main:2393 | object | `Bell` |
| `IconName::BookOpen` | Manager: release notes (1): manager-gui/render_apps:688 | object | `Book` |
| `IconName::Bot` | Awake rules / triggered session (3): awake-gui/shell:65, manager-gui/pages/keep_awake/render:310, manager-gui/pages/keep_awake/render:1022 | object | `Automation` |
| `IconName::Calendar` | Monitor nav: History (1): monitor-gui/render:44 | object | `History` |
| `IconName::Calendar` | Monitor: changes by date; Gallery: Time page (2): monitor-gui/stored:683, fluent-gallery/main:152 | object | `Calendar` |
| `IconName::Calendar` | Files sidebar: Recent (1): files-gui/shell:1412 | object | `Recent` |
| `IconName::ChartPie` | Monitor nav: Performance (1): monitor-gui/render:41 | object | `Chart` |
| `IconName::Check` | check marks (toggle, menu, chip, time picker, copied, current appearance, info bar success) (8): manager-gui/pages/system/render:589, better-ui/fluent/time_picker:462, better-ui/fluent/toggle:201, better-ui/fluent/menu:153, better-ui/fluent/info_bar:40, better-ui/fluent/chip:84, git-gui/render:4785, launcher-gui/overlay:1391 | symbol | stays |
| `IconName::Check` | Gallery command: Select all (1): fluent-gallery/main:1847 | action | `SelectAll` |
| `IconName::ChevronDown` | chevrons (14): manager-gui/pages/keep_awake/render:837, app-chooser-gui/chooser:606, better-ui/fluent/button:404, better-ui/fluent/button:575, better-ui/fluent/list:1211, better-ui/fluent/list:1306, better-ui/fluent/selection:322, better-ui/fluent/command_bar:318, better-ui/fluent/settings_card:449, better-ui/fluent/text_box:536, git-gui/graph_pane:433, git-gui/render:435, git-gui/render:731, files-gui/views:784 | symbol | stays |
| `IconName::ChevronLeft` | chevrons (1): launcher-gui/overlay:918 | symbol | stays |
| `IconName::ChevronRight` | chevrons (7): app-chooser-gui/chooser:608, better-ui/fluent/breadcrumb:171, better-ui/fluent/list:1213, better-ui/fluent/menu:674, better-ui/fluent/settings_card:260, files-gui/views:550, launcher-gui/overlay:781 | symbol | stays |
| `IconName::ChevronUp` | chevrons (6): manager-gui/pages/keep_awake/render:826, better-ui/fluent/list:1304, better-ui/fluent/settings_card:447, better-ui/fluent/text_box:535, git-gui/graph_pane:424, files-gui/views:783 | symbol | stays |
| `IconName::ChevronsUpDown` | Monitor: end process tree (command, menu) (2): monitor-gui/render:153, monitor-gui/render:591 | action | `EndTask` |
| `IconName::ChevronsUpDown` | Monitor: tree view toggle (1): monitor-gui/render:171 | action | `TreeView` |
| `IconName::ChevronsUpDown` | Touchpad nav: Scrolling (1): touchpad-gui/pages:38 | object | `Scrolling` |
| `IconName::ChevronsUpDown` | Gallery nav: Selection page (1): fluent-gallery/main:151 | object | `Checklist` |
| `IconName::CircleCheck` | success status mark (badges, rows, checks) (8): awake-gui/pages_records:257, awake-gui/pages_records:262, manager-gui/render:282, manager-gui/render_apps:268, manager-gui/render_apps:1155, manager-gui/render_apps:1361, git-gui/render:825, fluent-gallery/main:2244 | symbol (status mark) | stays |
| `IconName::CircleCheck` | Manager: Health page, status card, checks; Monitor: data sources (4): manager-gui/render_apps:336, manager-gui/render_apps:596, manager-gui/render_apps:637, monitor-gui/pages:2415 | object | `Health` |
| `IconName::CircleCheck` | Enable (Manager component, Monitor command and menu) (5): manager-gui/render_apps:716, monitor-gui/render:186, monitor-gui/render:225, monitor-gui/render:518, monitor-gui/render:539 | action | `Enable` |
| `IconName::CircleCheck` | Touchpad nav: Clicking; Gallery nav: Buttons (2): touchpad-gui/pages:39, fluent-gallery/main:149 | object | `Click` |
| `IconName::CircleCheck` | Files command: Select all (1): files-gui/shell:566 | action | `SelectAll` |
| `IconName::CircleX` | error status mark / clear field (6): manager-gui/render:284, manager-gui/render_apps:1157, manager-gui/render_apps:1363, better-ui/fluent/text_box:345, git-gui/render:826, fluent-gallery/main:2263 | symbol (status mark) | stays |
| `IconName::CircleX` | Disable (Manager component, Monitor command and menu, Awake override all) (6): manager-gui/render_apps:714, manager-gui/pages/keep_awake/render:915, monitor-gui/render:195, monitor-gui/render:234, monitor-gui/render:516, monitor-gui/render:537 | action | `Disable` |
| `IconName::CircleX` | Monitor: End task (command, menu) (4): monitor-gui/render:116, monitor-gui/render:143, monitor-gui/render:475, monitor-gui/render:578 | action | `EndTask` |
| `IconName::Close` | close / dismiss / info bar error mark (15): manager-gui/pages/keep_awake/render:1309, better-ui/fluent/time_picker:463, better-ui/fluent/info_bar:42, better-ui/fluent/info_bar:146, better-ui/fluent/text_box:275, better-ui/fluent/toast:525, better-ui/fluent/tabs:308, git-gui/render:2472, git-gui/render:2641, files-gui/render:839, files-gui/views:738, files-gui/shell:749, files-gui/panels:67, files-gui/panels:338, files-gui/panels:807 | symbol | stays |
| `IconName::Close` | Launcher: Remove from list (1): launcher-gui/overlay:732 | action | `Remove` |
| `IconName::Copy` | Copy, Copy path, Duplicate, Copy result (12): manager-gui/pages/system/render:591, better-ui/fluent/info_bar:374, git-gui/render:2787, fluent-gallery/main:634, fluent-gallery/main:639, fluent-gallery/main:1241, fluent-gallery/main:1408, fluent-gallery/main:1833, files-gui/shell:455, files-gui/shell:575, files-gui/shell:580, launcher-gui/overlay:735 | action | `Copy` |
| `IconName::Cpu` | Awake nav: Diagnostics; Awake backend card (2): awake-gui/shell:69, manager-gui/pages/keep_awake/render:1747 | object | `Diagnostics` |
| `IconName::Cpu` | Manager nav: System (1): manager-gui/shell:32 | object | `Computer` |
| `IconName::Cpu` | System: sensors (1): manager-gui/pages/system/render:543 | object | `Thermometer` |
| `IconName::Cpu` | Monitor title bar (1): monitor-gui/render:98 | object | `Chart` |
| `IconName::Cpu` | Monitor: CPU (2): monitor-gui/popup:90, monitor-gui/pages:270 | object | `Cpu` |
| `IconName::Delete` | Delete / Remove / Uninstall / Move to trash / Empty trash (13): awake-gui/pages_status:466, awake-gui/pages_rules:697, manager-gui/render_apps:729, manager-gui/render_apps:763, manager-gui/pages/keep_awake/render:848, manager-gui/pages/keep_awake/render:1280, fluent-gallery/main:1266, fluent-gallery/main:1412, fluent-gallery/main:1840, files-gui/shell:426, files-gui/shell:432, files-gui/shell:477, launcher-gui/overlay:734 | action | `Delete` |
| `IconName::Delete` | the Trash place (Files sidebar, location, Gallery nav) (3): fluent-gallery/main:1683, files-gui/shell:1383, files-gui/shell:1411 | object | `Trash` |
| `IconName::Ellipsis` | more (2): better-ui/fluent/button:523, better-ui/fluent/command_bar:497 | symbol | stays |
| `IconName::ExternalLink` | System: removable or optical drive card (1): manager-gui/pages/system/drives:492 | object | `ExternalDrive` |
| `IconName::ExternalLink` | App chooser title bar (1): app-chooser-gui/main:98 | object | `Apps` |
| `IconName::ExternalLink` | hyperlink's trailing mark (1): better-ui/fluent/hyperlink:111 | symbol | stays |
| `IconName::ExternalLink` | Open (in browser, a result) (4): git-gui/render:851, git-gui/render:1374, git-gui/render:2794, launcher-gui/overlay:727 | action | `OpenExternal` |
| `IconName::ExternalLink` | Files command: New window (1): files-gui/shell:549 | action | `NewWindow` |
| `IconName::Eye` | Manager: login screen watch (1): manager-gui/render_apps:1204 | object | `LoginScreen` |
| `IconName::Eye` | password reveal inside a text box (1): better-ui/fluent/text_box:289 | symbol | stays |
| `IconName::Eye` | Preview window title bar (1): files-gui/bin/preview:992 | object | `File` |
| `IconName::EyeOff` | Git: a private repository (1): git-gui/render:3490 | object | `Lock` |
| `IconName::File` | Git: editor (shortcut, settings card) (2): git-gui/render_parts:201, git-gui/render:4400 | object | `CodeEditor` |
| `IconName::File` | Files menu: New file (1): files-gui/shell:291 | action | `NewFile` |
| `IconName::File` | a file (rows, tabs, path bar, fallback, search results, import summary) (13): manager-gui/pages/touchpad/render_gestures:878, fluent-gallery/main:1398, fluent-gallery/main:1530, fluent-gallery/main:1533, fluent-gallery/main:1536, fluent-gallery/main:1547, fluent-gallery/main:1550, fluent-gallery/main:1803, files-gui/plugin_page:34, files-gui/views:538, files-gui/views:1036, launcher-gui/overlay:1113, launcher-gui/overlay:1171 | object | `File` |
| `IconName::Folder` | test fixture (3): better-ui/fluent/tests:716, better-ui/fluent/tests:718, better-ui/fluent/tests:922 | test fixture | stays |
| `IconName::Folder` | a folder (repo rows, tabs, sidebar, fallback, Files component, title bar) (19): manager-gui/components:96, manager-gui/pages/touchpad/render_gestures:792, manager-gui/render_apps:646, git-gui/render:478, git-gui/render:1044, git-gui/render:1567, fluent-gallery/main:1403, fluent-gallery/main:1524, fluent-gallery/main:1527, fluent-gallery/main:1541, fluent-gallery/main:1544, fluent-gallery/main:1672, files-gui/render:298, files-gui/plugin_page:32, files-gui/views:1035, files-gui/shell:781, files-gui/shell:1385, files-gui/shell:1414, launcher-gui/overlay:1111 | object | `Folder` |
| `IconName::FolderClosed` | Files menu: New folder (1): files-gui/shell:282 | action | `NewFolder` |
| `IconName::FolderOpen` | Open file location / Add local repository (4): monitor-gui/render:485, monitor-gui/render:628, git-gui/render:1533, launcher-gui/overlay:733 | action | `OpenFolder` |
| `IconName::FolderOpen` | Files command: Extract (1): files-gui/shell:592 | action | `Extract` |
| `IconName::FolderOpen` | an open folder (Git Files shortcut, Launcher folder details) (2): git-gui/render_parts:202, launcher-gui/overlay:1169 | object | `FolderOpen` |
| `IconName::Frame` | touchpad (component, nav, cards, hero, title bar) (9): manager-gui/components:98, manager-gui/shell:34, manager-gui/pages/touchpad/render_gestures:223, manager-gui/pages/touchpad/render:96, manager-gui/pages/touchpad/render:148, manager-gui/pages/touchpad/render:242, manager-gui/pages/touchpad/render:573, touchpad-gui/pages:72, touchpad-gui/pages:947 | object | `Touchpad` |
| `IconName::Frame` | Monitor: GPU (3): monitor-gui/popup:92, monitor-gui/pages:421, monitor-gui/pages:449 | object | `Gpu` |
| `IconName::Frame` | Touchpad nav: Pointer (1): touchpad-gui/pages:37 | object | `Pointer` |
| `IconName::Frame` | Gallery: Window page, window demo (2): fluent-gallery/main:158, fluent-gallery/main:2429 | object | `Window` |
| `IconName::Frame` | Files command: Cut (1): files-gui/shell:449 | action | `Cut` |
| `IconName::GalleryVerticalEnd` | Manager title bar (1): manager-gui/shell:78 | object | `Settings` |
| `IconName::GalleryVerticalEnd` | Monitor: Processes nav, grouping card (2): monitor-gui/render:39, monitor-gui/pages:2392 | object | `ProcessList` |
| `IconName::GalleryVerticalEnd` | Files: List view (1): files-gui/shell:1376 | action | `ViewList` |
| `IconName::Github` | Manager: the better-git component (1): manager-gui/components:85 | object | `Branch` |
| `IconName::Github` | Git toolbar: open on GitHub (brand mark) (1): git-gui/render_parts:204 | symbol | stays |
| `IconName::Globe` | language / public repository / web (cards, nav demo) (7): manager-gui/render:380, monitor-gui/pages:2457, git-gui/render:3492, git-gui/render:4516, fluent-gallery/main:1676, fluent-gallery/main:2291, fluent-gallery/main:2625 | object | `Globe` |
| `IconName::Globe` | Git graph: remote ref chip (1): git-gui/graph_pane:734 | object | `Cloud` |
| `IconName::Globe` | Git welcome: Clone a repository (1): git-gui/render:1521 | action | `Download` |
| `IconName::Globe` | Files: a tailnet machine / peer (2): files-gui/plugin_page:29, files-gui/shell:952 | object | `RemoteComputer` |
| `IconName::HardDrive` | a drive (components, cards, Monitor disk, sidebar rows, Devices section) (12): manager-gui/components:86, manager-gui/pages/system/drives:493, monitor-gui/popup:93, monitor-gui/pages:345, fluent-gallery/main:1679, fluent-gallery/main:2350, files-gui/plugin_page:30, files-gui/shell:611, files-gui/shell:1011, files-gui/shell:1027, files-gui/shell:1097, files-gui/shell:1416 | object | `Drive` |
| `IconName::HardDrive` | Touchpad nav: Devices (1): touchpad-gui/pages:41 | object | `Touchpad` |
| `IconName::HardDrive` | Files sidebar: This PC (1): files-gui/shell:975 | object | `Computer` |
| `IconName::Inbox` | Manager: update channel, generic component, kind, installed version (4): manager-gui/render:419, manager-gui/components:99, manager-gui/render_apps:602, manager-gui/render_apps:667 | object | `Package` |
| `IconName::Inbox` | Apps (Manager nav, Gallery nav demo) (2): manager-gui/shell:36, fluent-gallery/main:1864 | object | `Apps` |
| `IconName::Inbox` | test fixture (1): better-ui/fluent/tests:1275 | test fixture | stays |
| `IconName::Inbox` | Gallery nav: Dialogs page (1): fluent-gallery/main:154 | object | `About` |
| `IconName::Inbox` | Files command: Paste (1): files-gui/shell:461 | action | `Paste` |
| `IconName::Inbox` | Files command: Compress (1): files-gui/shell:586 | action | `Compress` |
| `IconName::Inbox` | Files sidebar: Dropbox (1): files-gui/shell:885 | object | `Cloud` |
| `IconName::Inbox` | Git: stashed changes row (1): git-gui/render:1939 | object | `Inbox` |
| `IconName::Info` | information status mark (rows, info bar, checks, sign-in notes) (7): manager-gui/render:285, manager-gui/render_apps:1364, better-ui/fluent/info_bar:39, git-gui/render:828, fluent-gallery/main:2271, files-gui/signin:100, files-gui/signin:120 | symbol (status mark) | stays |
| `IconName::Info` | About / what it does / system info / rules intro / prompts (cards, nav) (8): manager-gui/render:431, manager-gui/pages/system/render:566, manager-gui/pages/default_apps/render:470, manager-gui/render_apps:589, monitor-gui/render:46, monitor-gui/stored:440, monitor-gui/pages:2478, git-gui/render:4344 | object | `About` |
| `IconName::Info` | Properties (Monitor command and menu, App chooser details, Gallery) (5): app-chooser-gui/chooser:636, monitor-gui/render:129, monitor-gui/render:162, monitor-gui/render:634, fluent-gallery/main:1851 | action | `Properties` |
| `IconName::Info` | keyboard hint / keyboard shortcuts (Awake footer, Files command) (2): awake-gui/shell:128, files-gui/shell:618 | object | `Keyboard` |
| `IconName::Inspector` | History (Awake nav, Manager activity pages) (2): awake-gui/shell:68, manager-gui/render_apps:337 | object | `History` |
| `IconName::Inspector` | Manager: the Monitor component (1): manager-gui/components:95 | object | `Chart` |
| `IconName::Inspector` | Touchpad nav: Diagnostics (1): touchpad-gui/pages:42 | object | `Diagnostics` |
| `IconName::LayoutDashboard` | Overview / Home / Status pages (6): awake-gui/shell:63, manager-gui/shell:31, monitor-gui/render:38, touchpad-gui/pages:36, fluent-gallery/main:1671, fluent-gallery/main:1860 | object | `Dashboard` |
| `IconName::LayoutDashboard` | applications (Launcher component, Files Applications place, app fallback) (4): manager-gui/components:97, files-gui/views:1037, files-gui/shell:1384, files-gui/shell:1417 | object | `Apps` |
| `IconName::LayoutDashboard` | Gallery nav: Collections page (1): fluent-gallery/main:155 | object | `Table` |
| `IconName::LayoutDashboard` | Files: Grid view (1): files-gui/shell:1375 | action | `ViewGrid` |
| `IconName::Loader` | Monitor: fans (1): monitor-gui/popup:97 | object | `Fan` |
| `IconName::Loader` | Monitor: sampling interval (1): monitor-gui/pages:2473 | object | `Timer` |
| `IconName::Loader` | Git: checks pending (status mark) (1): git-gui/render:827 | symbol (status mark) | stays |
| `IconName::Loader` | Gallery nav: Status page (1): fluent-gallery/main:153 | object | `Health` |
| `IconName::Map` | Gallery nav: Navigation page (1): fluent-gallery/main:156 | object | `Map` |
| `IconName::Maximize` | gestures (preset, diagnostics, Touchpad nav) (3): manager-gui/pages/touchpad/render_gestures:263, manager-gui/pages/touchpad/render:739, touchpad-gui/pages:40 | object | `Gesture` |
| `IconName::Maximize` | Git: expand file (1): git-gui/render:2910 | symbol | stays |
| `IconName::MemoryStick` | Monitor: memory (2): monitor-gui/popup:91, monitor-gui/pages:305 | object | `Memory` |
| `IconName::Menu` | navigation pane toggle (1): better-ui/fluent/navigation:688 | symbol | stays |
| `IconName::Minus` | stepper minus (2): awake-gui/pages_rules:317, awake-gui/components:232 | symbol | stays |
| `IconName::Moon` | lid closed / sleep / dark appearance (5): manager-gui/pages/keep_awake/render:478, manager-gui/pages/keep_awake/render:611, manager-gui/pages/keep_awake/render:1232, fluent-gallery/main:2378, fluent-gallery/main:2604 | object | `Moon` |
| `IconName::Network` | network (3): monitor-gui/popup:94, monitor-gui/pages:398, fluent-gallery/main:2326 | object | `Network` |
| `IconName::Palette` | appearance / theme (7): manager-gui/render:339, monitor-gui/pages:2451, git-gui/render:4343, git-gui/render:4496, fluent-gallery/main:2341, fluent-gallery/main:2697, launcher-gui/overlay:1396 | object | `Palette` |
| `IconName::PanelBottom` | the device banner (this laptop) (2): manager-gui/render:150, fluent-gallery/main:2178 | object | `Computer` |
| `IconName::PanelLeft` | Files: Columns view (1): files-gui/shell:1377 | action | `ViewColumns` |
| `IconName::PanelRight` | Details / Preview pane (2): fluent-gallery/main:1843, files-gui/shell:495 | action | `ShowPane` |
| `IconName::PanelRight` | Files: Split view (1): files-gui/shell:559 | action | `SplitView` |
| `IconName::Pause` | Pause (5): manager-gui/pages/keep_awake/render:901, manager-gui/pages/keep_awake/render:908, monitor-gui/render:265, monitor-gui/render:612, files-gui/bin/preview:900 | action | `Pause` |
| `IconName::Play` | quick sessions (Awake nav and card) (2): awake-gui/shell:64, manager-gui/pages/keep_awake/render:796 | object | `Timer` |
| `IconName::Play` | Touchpad: live gesture testing card (1): manager-gui/pages/touchpad/render_gestures:736 | object | `Diagnostics` |
| `IconName::Play` | Monitor nav: Startup (1): monitor-gui/render:42 | object | `Startup` |
| `IconName::Play` | Start / Resume / Play (5): awake-gui/pages_status:432, manager-gui/pages/keep_awake/render:924, monitor-gui/render:263, monitor-gui/render:610, files-gui/bin/preview:902 | action | `Play` |
| `IconName::Plus` | stepper plus / new tab (3): awake-gui/pages_rules:325, awake-gui/components:238, better-ui/fluent/tabs:347 | symbol | stays |
| `IconName::Plus` | Manager: what a component enhances (card) (1): manager-gui/render_apps:618 | object | `Enhance` |
| `IconName::Plus` | Add / New (rule, condition, group, app, repository, Files New menu) (13): awake-gui/pages_status:366, awake-gui/pages_rules:107, awake-gui/pages_rules:566, awake-gui/pages_rules:725, manager-gui/render_window_attention:173, manager-gui/pages/keep_awake/render:896, manager-gui/pages/keep_awake/render:1347, manager-gui/pages/keep_awake/render:1358, git-gui/render_parts:318, git-gui/render:1309, git-gui/render:1527, fluent-gallery/main:1829, files-gui/shell:444 | action | `Add` |
| `IconName::Redo` | Files: Redo (1): files-gui/shell:543 | symbol | stays |
| `IconName::Redo` | Manager: restart requirement card (1): manager-gui/render_apps:631 | object | `Restart` |
| `IconName::Redo` | Refresh / Verify again / Fetch / Re-run checks / Retry mount (8): manager-gui/pages/default_apps/render:268, manager-gui/pages/touchpad/render:151, manager-gui/pages/touchpad/render:249, monitor-gui/render:204, monitor-gui/render:243, git-gui/render:639, git-gui/render:842, files-gui/shell:1054 | action | `Refresh` |
| `IconName::Replace` | Manager: apps a component replaces (card) (1): manager-gui/render_apps:612 | object | `Apps` |
| `IconName::Replace` | Gallery command: Rename (1): fluent-gallery/main:1837 | action | `Rename` |
| `IconName::Search` | search inside a text box / navigation pane (2): better-ui/fluent/navigation:730, better-ui/fluent/text_box:300 | symbol | stays |
| `IconName::Search` | Manager: Verify (retry check) (1): manager-gui/render_apps:718 | action | `Verify` |
| `IconName::Search` | Git graph: Find (1): git-gui/graph_pane:391 | action | `Search` |
| `IconName::Search` | Git: toggle the filter (1): git-gui/render:1893 | action | `Filter` |
| `IconName::Settings` | test fixture (1): better-ui/fluent/tests:721 | test fixture | stays |
| `IconName::Settings` | System: fan mode card (2): manager-gui/pages/system/render:493, manager-gui/pages/system/render:508 | object | `Fan` |
| `IconName::Settings` | settings (nav, buttons, cards, component, footer) (13): awake-gui/shell:70, manager-gui/components:94, manager-gui/shell:39, manager-gui/pages/touchpad/render_gestures:462, monitor-gui/render:47, touchpad-gui/pages:96, git-gui/render:577, git-gui/render:2154, fluent-gallery/main:157, fluent-gallery/main:621, fluent-gallery/main:625, fluent-gallery/main:1752, fluent-gallery/main:2417 | object | `Settings` |
| `IconName::Settings2` | Awake nav: session defaults (1): awake-gui/shell:66 | object | `Settings` |
| `IconName::Settings2` | Monitor nav: Services (1): monitor-gui/render:43 | object | `Services` |
| `IconName::Settings2` | Git settings nav: Git (1): git-gui/render:4342 | object | `Branch` |
| `IconName::SortAscending` | sort order (2): fluent-gallery/main:1845, files-gui/shell:1368 | symbol | stays |
| `IconName::SortDescending` | sort order (1): files-gui/shell:1369 | symbol | stays |
| `IconName::SquareTerminal` | System: keyboard card (2): manager-gui/pages/system/render:427, manager-gui/pages/system/render:449 | object | `Keyboard` |
| `IconName::SquareTerminal` | App chooser: a program row (1): app-chooser-gui/chooser:868 | object | `Window` |
| `IconName::SquareTerminal` | Monitor: Details (nav, Go to details menu) (2): monitor-gui/render:40, monitor-gui/render:493 | object | `Table` |
| `IconName::SquareTerminal` | terminal (command lines card, Git shortcut and settings, Open terminal here, Run in terminal) (6): monitor-gui/pages:2464, git-gui/render_parts:203, git-gui/render:4341, git-gui/render:4410, files-gui/shell:603, launcher-gui/overlay:728 | object | `Terminal` |
| `IconName::Star` | Use Better defaults (1): manager-gui/pages/default_apps/render:254 | action | `SetDefault` |
| `IconName::Star` | Pin to sidebar / Pin to Start (2): files-gui/shell:598, launcher-gui/overlay:729 | action | `Pin` |
| `IconName::Star` | favourites / default apps (nav, cards, Favorites section) (6): manager-gui/shell:37, manager-gui/render_apps:656, fluent-gallery/main:1673, fluent-gallery/main:1814, fluent-gallery/main:2366, files-gui/shell:1418 | object | `Star` |
| `IconName::StarFill` | Files: favourite emblem, and its tests (3): files-gui/tests:3274, files-gui/tests:3283, files-gui/shell:1404 | object | `Star` |
| `IconName::StarOff` | Launcher: Unpin from Start (1): launcher-gui/overlay:730 | action | `Unpin` |
| `IconName::Sun` | Brightness / light appearance (3): manager-gui/pages/system/render:380, manager-gui/pages/system/render:392, fluent-gallery/main:2602 | object | `Brightness` |
| `IconName::Sun` | Monitor: temperatures (1): monitor-gui/popup:96 | object | `Thermometer` |
| `IconName::Sun` | Keep awake (Awake hero and title bar, component, nav, cards) (8): awake-gui/shell:104, awake-gui/render:69, manager-gui/components:84, manager-gui/shell:35, manager-gui/pages/keep_awake/render:123, manager-gui/pages/keep_awake/render:251, manager-gui/pages/keep_awake/render:2029, fluent-gallery/main:1861 | object | `KeepAwake` |
| `IconName::TriangleAlert` | Fixes (Manager nav, Gallery nav demo) (2): manager-gui/shell:38, fluent-gallery/main:1867 | object | `Fixes` |
| `IconName::TriangleAlert` | Touchpad: safe mode card (1): manager-gui/pages/touchpad/render:722 | object | `Shield` |
| `IconName::TriangleAlert` | warning status mark (10): manager-gui/render:283, manager-gui/render_apps:1156, manager-gui/render_apps:1362, better-ui/fluent/info_bar:41, better-ui/fluent/settings_card:118, git-gui/render_parts:421, git-gui/render:1042, fluent-gallery/main:2252, files-gui/signin:110, files-gui/signin:127 | symbol (status mark) | stays |
| `IconName::Undo` | Files: Undo (1): files-gui/shell:537 | symbol | stays |
| `IconName::Undo` | Gallery selector demo: Recent (1): fluent-gallery/main:1812 | object | `Recent` |
| `IconName::Undo` | Restore (defaults, section, gestures, previous version, captured values) (5): manager-gui/pages/touchpad/render_gestures:112, manager-gui/pages/default_apps/render:261, manager-gui/render_apps:724, manager-gui/pages/touchpad/render:319, manager-gui/pages/touchpad/render:690 | action | `Restore` |
| `IconName::Undo2` | Files: Restore from trash (1): files-gui/shell:419 | action | `Restore` |
| `IconName::User` | Files sidebar: Home (1): files-gui/shell:1413 | object | `Home` |
| `IconName::User` | a person / account (manual session, sign in, accounts, Shared) (8): manager-gui/pages/keep_awake/render:309, git-gui/render:1549, git-gui/render:4340, git-gui/render:4357, git-gui/render:4367, fluent-gallery/main:1813, fluent-gallery/main:2372, files-gui/signin:93 | object | `User` |
| `IconName::WindowClose` | window button (1): better-ui/fluent/chrome:285 | symbol | stays |
| `IconName::WindowMaximize` | window button (1): better-ui/fluent/chrome:283 | symbol | stays |
| `IconName::WindowMinimize` | window button (1): better-ui/fluent/chrome:282 | symbol | stays |
| `IconName::WindowRestore` | window button (1): better-ui/fluent/chrome:284 | symbol | stays |

Glyphs drawn in this repository:

| Local name | Where, and what it means | Kind | Replaced by |
|---|---|---|---|
| `files-gui` `icons::HOME`, `DESKTOP`, `DOCUMENTS`, `DOWNLOADS`, `PICTURES`, `MUSIC`, `VIDEOS`, `TEMPLATES`, `PUBLIC`, `DEVELOPER`, `TRASH` | Files: a person's folders, through `FolderGlyph::svg()` (sidebar `shell:1400`, path bar `shell:208`, tabs `shell:782`, rows `views:536`) | object | `Home`, `Desktop`, `Documents`, `Downloads`, `Pictures`, `Music`, `Videos`, `Templates`, `Public`, `Developer`, `Trash` |
| `files-gui` `icons::FOLDER`, `DEVICE`, `NETWORK` (the kit's `folder`, `hard-drive`, `globe`) | Files: `FolderGlyph::{Folder, Device, Network}` and `drive_icon` | object | `Folder`, `Drive`, `Network` |
| `files-gui` `icons::COMPUTER` | Files: This PC (`shell:203`, `shell:976`) | object | `Computer` |
| `files-gui` `icons::SYSTEM_DRIVE`, `EXTERNAL_DRIVE`, `MEMORY_CARD`, `USB_DRIVE`, `DISC`, `PHONE` | Files: drives by kind, through `drive_icon` / `drive_row_icon` (`shell:1028`, `panels:1214`) | object | `SystemDrive`, `ExternalDrive`, `MemoryCard`, `UsbDrive`, `Disc`, `Phone` |
| `files-gui` `icons::OPERATIONS` | Files: the file operations panel button (`shell:509`) | object | `Transfers` |
| `files-gui` `icons::CODE_MARK`, `CODE_FOLDER_SMALL` | Files content area: the developer emblem on a themed folder | content area | stays (the ticket keeps the content area's theme icons) |
| `fluent::glyphs::REFRESH` | Refresh in Files, Git (3), Monitor (2), Awake | action | `Refresh` |
| `fluent::glyphs::WRAP` | Git diff: wrap lines | plain symbol | stays |
| `git-gui` `BRANCH_ICON` (11 uses), `WORKTREE_ICON` (3), `TAG_ICON` (2) | Git: a branch, a worktree, a tag | object | `Branch`, `Worktree`, `Tag` |

Decisions for the owner or stage 2 to confirm:

- A settings card's or navigation item's icon is an object icon even when
  the card is about an action's result (Restart requirement, Enhances);
  a card whose whole job is an action (Add an app, Remove) takes the action
  icon. Window title bars get an object for the window's subject, not the
  application icon, which is out of scope.
- Undo, Redo, Back, Forward, Up, move up/down, sort and lower priority stay
  plain symbols because they are arrows. Search inside a text box or the
  navigation pane stays the control's own glyph; a Find or Verify button
  gets the action icon.
- The GitHub button in Better Git keeps the kit's GitHub mark: it is a brand,
  and redrawing it in colour would misdraw it.
- Same picture, two names was avoided: "New" and "Add" are both `Add`;
  Copy, Copy path and Duplicate are all `Copy`.
- `KeepAwake` is an open eye with a blue iris, after the Better Awake
  application icon.
- `img` rasterises an SVG at its own 24px size times two (GPUI's
  `SMOOTH_SVG_SCALE_FACTOR`), so a 40px picture on a 2x screen is scaled up
  from 48px and is slightly soft. Rows at 16 and 20px are not affected.
