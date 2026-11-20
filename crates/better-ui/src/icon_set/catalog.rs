//! Every picture icon by name: its kind, the asset path a window's
//! `AssetSource` serves it under, and its SVG. No GPUI here, so the set can
//! be checked by plain tests.
//!
//! Each SVG is its own file under `crates/better-ui/assets/icons/object/` or
//! `.../action/`, embedded at build time, so a person can open and edit it.

/// What a picture icon stands for (DESIGN.md, Icons).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Kind {
    /// A thing: a place, a device, a file kind, a category. A filled,
    /// full-colour picture.
    Object,
    /// What a button does. Two layers of one colour that the window gives
    /// it: the body, and a minor part in a lighter shade. Its files are
    /// masks and carry no colour.
    Action,
}

impl Kind {
    /// The folder its files live in, under `assets/icons/`.
    pub const fn directory(self) -> &'static str {
        match self {
            Kind::Object => "object",
            Kind::Action => "action",
        }
    }
}

/// The prefix every picture's asset path starts with.
pub const ASSET_PREFIX: &str = "better-ui/icons/";

/// The size an object's file declares: 24px, the grid it is drawn on.
pub const INTRINSIC: f32 = 24.0;

/// The size the large copy of an object declares. GPUI rasterises an `img`
/// SVG once, at twice the size the file declares, whatever size it is drawn
/// at and whatever the display's scale, so a 24px file drawn at 64px on a
/// 2x display is a 48px raster stretched to 128. The large copy is the same
/// drawing with this size in its header, for anything drawn above
/// [`INTRINSIC`].
pub const LARGE_INTRINSIC: f32 = 64.0;

/// An action's second layer, for an entry marked `two`.
macro_rules! second_layer {
    ($file:literal) => {
        None
    };
    ($file:literal two) => {
        Some((
            concat!("better-ui/icons/action/", $file, ".2.svg"),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/icons/action/",
                $file,
                ".2.svg"
            )),
        ))
    };
}

macro_rules! pictures {
    (
        objects { $($object:ident $object_file:literal,)* }
        actions { $($action:ident $action_file:literal $($two:ident)?,)* }
    ) => {
        /// Every picture icon Better OS draws. Plain symbols (arrows,
        /// chevrons, close, check, window buttons) are not here: they stay
        /// the toolkit's 2px line icons.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        pub enum Picture {
            $($object,)*
            $($action,)*
        }

        impl Picture {
            /// Every picture, objects first.
            pub const ALL: &'static [Picture] = &[
                $(Picture::$object,)*
                $(Picture::$action,)*
            ];

            /// The variant's name, as written in code.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Picture::$object => stringify!($object),)*
                    $(Picture::$action => stringify!($action),)*
                }
            }

            pub const fn kind(self) -> Kind {
                match self {
                    $(Picture::$object => Kind::Object,)*
                    $(Picture::$action => Kind::Action,)*
                }
            }

            /// The path a window's `AssetSource` serves it under, for
            /// `img(path)`.
            pub const fn path(self) -> &'static str {
                match self {
                    $(Picture::$object => concat!("better-ui/icons/object/", $object_file, ".svg"),)*
                    $(Picture::$action => concat!("better-ui/icons/action/", $action_file, ".svg"),)*
                }
            }

            /// The SVG text, as the file under `assets/icons/` holds it.
            pub const fn svg(self) -> &'static str {
                match self {
                    $(Picture::$object => include_str!(concat!(
                        env!("CARGO_MANIFEST_DIR"), "/assets/icons/object/", $object_file, ".svg"
                    )),)*
                    $(Picture::$action => include_str!(concat!(
                        env!("CARGO_MANIFEST_DIR"), "/assets/icons/action/", $action_file, ".svg"
                    )),)*
                }
            }

            /// An action icon's second layer, the minor part drawn in the
            /// lighter shade: its asset path and its SVG. `None` for an
            /// object, and for an action drawn in one layer.
            pub const fn second_layer(self) -> Option<(&'static str, &'static str)> {
                match self {
                    $(Picture::$object => None,)*
                    $(Picture::$action => second_layer!($action_file $($two)?),)*
                }
            }

            /// Where the large copy of an object is served (see
            /// [`LARGE_INTRINSIC`]); an action, drawn as a mask at the size of
            /// its element, has none and answers its own path.
            pub const fn large_path(self) -> &'static str {
                match self {
                    $(Picture::$object => concat!("better-ui/icons/object/large/", $object_file, ".svg"),)*
                    $(Picture::$action => concat!("better-ui/icons/action/", $action_file, ".svg"),)*
                }
            }

            /// The picture whose main file is served under `path`, if any.
            pub fn from_path(path: &str) -> Option<Picture> {
                match path {
                    $(concat!("better-ui/icons/object/", $object_file, ".svg") => Some(Picture::$object),)*
                    $(concat!("better-ui/icons/action/", $action_file, ".svg") => Some(Picture::$action),)*
                    _ => None,
                }
            }
        }
    };
}

pictures! {
    objects {
        // Places
        Home "home",
        Desktop "desktop",
        Documents "documents",
        Downloads "downloads",
        Pictures "pictures",
        Music "music",
        Videos "videos",
        Templates "templates",
        Public "public",
        Developer "developer",
        Trash "trash",
        Recent "recent",
        Folder "folder",
        FolderOpen "folder-open",
        File "file",
        Cloud "cloud",
        Network "network",
        // Devices
        Computer "computer",
        RemoteComputer "remote-computer",
        SystemDrive "system-drive",
        Drive "drive",
        ExternalDrive "external-drive",
        MemoryCard "memory-card",
        UsbDrive "usb-drive",
        Disc "disc",
        Phone "phone",
        Touchpad "touchpad",
        Keyboard "keyboard",
        Cpu "cpu",
        Memory "memory",
        Gpu "gpu",
        Fan "fan",
        Thermometer "thermometer",
        // Power
        Battery "battery",
        BatteryFull "battery-full",
        BatteryMedium "battery-medium",
        BatteryLow "battery-low",
        BatteryCharging "battery-charging",
        Gauge "gauge",
        Brightness "brightness",
        Moon "moon",
        KeepAwake "keep-awake",
        // Categories and things
        Settings "settings",
        Services "services",
        Terminal "terminal",
        CodeEditor "code-editor",
        Window "window",
        Apps "apps",
        Dashboard "dashboard",
        Package "package",
        Updates "updates",
        Health "health",
        Shield "shield",
        Lock "lock",
        Fixes "fixes",
        Diagnostics "diagnostics",
        Automation "automation",
        Timer "timer",
        History "history",
        Calendar "calendar",
        Chart "chart",
        ProcessList "process-list",
        Table "table",
        Checklist "checklist",
        Startup "startup",
        Restart "restart",
        Enhance "enhance",
        Transfers "transfers",
        Inbox "inbox",
        Globe "globe",
        User "user",
        LoginScreen "login-screen",
        Bell "bell",
        Palette "palette",
        Font "font",
        Star "star",
        Tag "tag",
        Book "book",
        About "about",
        Map "map",
        // Touch input
        Pointer "pointer",
        Click "click",
        Scrolling "scrolling",
        Gesture "gesture",
        // Git
        Branch "branch",
        Worktree "worktree",
    }
    actions {
        Add "add",
        NewFolder "new-folder" two,
        NewFile "new-file" two,
        NewWindow "new-window" two,
        Copy "copy" two,
        Cut "cut" two,
        Paste "paste" two,
        Rename "rename" two,
        Delete "delete" two,
        Remove "remove",
        Restore "restore",
        Refresh "refresh",
        OpenExternal "open-external" two,
        OpenFolder "open-folder" two,
        Compress "compress" two,
        Extract "extract" two,
        Pin "pin" two,
        Unpin "unpin" two,
        SetDefault "set-default",
        SelectAll "select-all" two,
        ShowPane "show-pane" two,
        SplitView "split-view",
        ViewGrid "view-grid",
        ViewList "view-list" two,
        ViewColumns "view-columns",
        TreeView "tree-view" two,
        Eject "eject",
        Play "play",
        Pause "pause",
        EndTask "end-task",
        Enable "enable",
        Disable "disable",
        Search "search" two,
        Verify "verify" two,
        Properties "properties",
        Filter "filter" two,
        Download "download" two,
        Push "push",
        Pull "pull",
    }
}

impl Picture {
    /// The path to draw an object at `size` from: its own file up to
    /// [`INTRINSIC`], the large copy above it.
    pub fn path_for(self, size: f32) -> &'static str {
        if self.kind() == Kind::Object && size > INTRINSIC {
            self.large_path()
        } else {
            self.path()
        }
    }

    /// The large copy's SVG: the file with [`LARGE_INTRINSIC`] in place of
    /// [`INTRINSIC`] in its header, the drawing and viewBox unchanged.
    pub fn large_svg(self) -> &'static str {
        static LARGE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        let all = LARGE.get_or_init(|| {
            Picture::ALL
                .iter()
                .map(|picture| {
                    picture.svg().replacen(
                        r#"width="24" height="24""#,
                        r#"width="64" height="64""#,
                        1,
                    )
                })
                .collect()
        });
        let index = Picture::ALL
            .iter()
            .position(|picture| *picture == self)
            .expect("every picture is in ALL");
        &all[index]
    }

    /// Whether the action destroys something, and so is drawn in the
    /// critical colour wherever it is.
    pub const fn destructive(self) -> bool {
        matches!(self, Picture::Delete | Picture::EndTask)
    }
}

/// The SVG for a picture's asset path (an action's second layer included),
/// or `None` for any other path. A window's `AssetSource` calls this before
/// falling back to the kit's own assets.
pub fn load(path: &str) -> Option<&'static [u8]> {
    if let Some(picture) = Picture::from_path(path) {
        return Some(picture.svg().as_bytes());
    }
    if let Some(picture) = Picture::ALL
        .iter()
        .find(|picture| picture.kind() == Kind::Object && picture.large_path() == path)
    {
        return Some(picture.large_svg().as_bytes());
    }
    Picture::ALL
        .iter()
        .filter_map(|picture| picture.second_layer())
        .find(|(second, _)| *second == path)
        .map(|(_, svg)| svg.as_bytes())
}
