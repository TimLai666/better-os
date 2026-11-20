//! Better Files' icons. Every place, device and folder is a picture from
//! `better_ui::icon_set` (ticket 147); the window keeps only the developer
//! folder's two glyphs, which the content area draws over the desktop
//! theme's folder pictures.

use std::borrow::Cow;

use better_ui::icon_set::Picture;
use gpui::{AssetSource, Result, SharedString};

/// The developer glyph's brackets alone, the mark the content view puts on a
/// full-colour folder.
pub const CODE_MARK: &str = "better-files/code-mark.svg";
/// The developer folder drawn whole for small sizes (16px list and column
/// rows), in full colour: a folder with `</>` inside it, so a small row
/// never stacks a mark on a folder. Adwaita's folder colours, which the
/// themes' small folders share.
pub const CODE_FOLDER_SMALL: &str = "better-files/code-folder-16.svg";
/// "This PC" (ticket 122).
pub const COMPUTER: Picture = Picture::Computer;
/// The file operations panel: things moving between two places.
pub const OPERATIONS: Picture = Picture::Transfers;

/// [`CODE_FOLDER_SMALL`]: drawn on the 16px grid, full colour.
const CODE_FOLDER_SMALL_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path d="M1 3.5A1.5 1.5 0 0 1 2.5 2h3.3l1.5 1.5h6.2A1.5 1.5 0 0 1 15 5v.5H1z" fill="#438de6"/><rect x="1" y="4.5" width="14" height="9.5" rx="1.5" fill="#a4caee"/><path d="M6.4 7.1 4.5 9.25l1.9 2.15M9.6 7.1l1.9 2.15-1.9 2.15" fill="none" stroke="#1c71d8" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// [`CODE_MARK`], in the 24px, 2px-stroke grid (Lucide's shape, ISC).
const CODE_MARK_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m8 7-5 5 5 5"/><path d="m16 7 5 5-5 5"/><path d="m14 4-4 16"/></svg>"#;

fn svg(path: &str) -> Option<&'static str> {
    match path {
        CODE_FOLDER_SMALL => Some(CODE_FOLDER_SMALL_SVG),
        CODE_MARK => Some(CODE_MARK_SVG),
        _ => None,
    }
}

/// The icon set, the window's two developer glyphs, and the kit's icons.
pub struct FilesAssets;

impl AssetSource for FilesAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(svg) = better_ui::icon_set::load(path) {
            return Ok(Some(Cow::Borrowed(svg)));
        }
        if let Some(svg) = svg(path) {
            return Ok(Some(Cow::Borrowed(svg.as_bytes())));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

/// What a folder is, for its icon (ticket 120). One decision, made from the
/// path and what the session already knows (its user directories and mount
/// table), never by reading the disk, and used wherever a folder is drawn:
/// the sidebar, the path bar, tabs, the content view and the preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderGlyph {
    Home,
    Desktop,
    Documents,
    Downloads,
    Music,
    Pictures,
    Videos,
    Templates,
    Public,
    Trash,
    /// A mounted drive's own top folder.
    Device,
    /// A server's folder: a network location, or a GVfs mount's top folder.
    Network,
    /// Where a person keeps code ([`DEVELOPER_NAMES`]).
    Developer,
    Folder,
}

/// The folder names, directly under home and in any case, that are a
/// person's code folder: `~/Developer` (macOS's and the owner's), and the
/// spellings people already use for the same thing.
pub const DEVELOPER_NAMES: [&str; 8] = [
    "developer",
    "development",
    "dev",
    "projects",
    "code",
    "src",
    "source",
    "repos",
];

impl FolderGlyph {
    pub const ALL: [FolderGlyph; 14] = [
        FolderGlyph::Home,
        FolderGlyph::Desktop,
        FolderGlyph::Documents,
        FolderGlyph::Downloads,
        FolderGlyph::Music,
        FolderGlyph::Pictures,
        FolderGlyph::Videos,
        FolderGlyph::Templates,
        FolderGlyph::Public,
        FolderGlyph::Trash,
        FolderGlyph::Device,
        FolderGlyph::Network,
        FolderGlyph::Developer,
        FolderGlyph::Folder,
    ];

    /// The picture the sidebar, path bar and tabs draw.
    pub fn picture(self) -> Picture {
        match self {
            FolderGlyph::Home => Picture::Home,
            FolderGlyph::Desktop => Picture::Desktop,
            FolderGlyph::Documents => Picture::Documents,
            FolderGlyph::Downloads => Picture::Downloads,
            FolderGlyph::Music => Picture::Music,
            FolderGlyph::Pictures => Picture::Pictures,
            FolderGlyph::Videos => Picture::Videos,
            FolderGlyph::Templates => Picture::Templates,
            FolderGlyph::Public => Picture::Public,
            FolderGlyph::Trash => Picture::Trash,
            FolderGlyph::Device => Picture::Drive,
            FolderGlyph::Network => Picture::Network,
            FolderGlyph::Developer => Picture::Developer,
            FolderGlyph::Folder => Picture::Folder,
        }
    }

    /// The icon theme's names for the full-colour icon the content view
    /// draws, best first; every list ends with the plain folder. The
    /// developer folder has no name in the common themes, so the view draws
    /// the plain folder with [`DEVELOPER`] as an emblem on it.
    pub fn theme_names(self) -> Vec<(&'static str, String)> {
        let named: &[(&str, &str)] = match self {
            FolderGlyph::Home => &[("places", "user-home")],
            FolderGlyph::Desktop => &[("places", "user-desktop")],
            FolderGlyph::Documents => &[("places", "folder-documents")],
            FolderGlyph::Downloads => &[("places", "folder-download")],
            FolderGlyph::Music => &[("places", "folder-music")],
            FolderGlyph::Pictures => &[("places", "folder-pictures")],
            FolderGlyph::Videos => &[("places", "folder-videos")],
            FolderGlyph::Templates => &[("places", "folder-templates")],
            FolderGlyph::Public => &[("places", "folder-publicshare")],
            FolderGlyph::Trash => &[("places", "user-trash")],
            FolderGlyph::Device => &[("devices", "drive-harddisk")],
            FolderGlyph::Network => &[("places", "folder-remote")],
            FolderGlyph::Developer => {
                &[("places", "folder-development"), ("places", "folder-code")]
            }
            FolderGlyph::Folder => &[],
        };
        named
            .iter()
            .map(|(context, name)| (*context, (*name).to_string()))
            .chain(std::iter::once(("places", "folder".to_string())))
            .collect()
    }

    /// The emblem the content view puts on a plain themed folder, when the
    /// theme has no icon of this folder's own.
    pub fn emblem(self) -> Option<&'static str> {
        (self == FolderGlyph::Developer).then_some(CODE_MARK)
    }
}

/// What the folder at `location` is. Reads nothing: the user directories and
/// mount table are the session's, already read.
pub fn folder_glyph(
    location: &files_core::Location,
    directories: &files_platform::UserDirectories,
    mounts: &files_platform::MountTable,
) -> FolderGlyph {
    use files_core::Location;
    use files_platform::UserDirectory as D;
    let path = match location {
        Location::Trash(_) => return FolderGlyph::Trash,
        Location::Network(_) => return FolderGlyph::Network,
        Location::Local(path) => path.as_path(),
        _ => return FolderGlyph::Folder,
    };
    for (directory, glyph) in [
        (D::Desktop, FolderGlyph::Desktop),
        (D::Documents, FolderGlyph::Documents),
        (D::Downloads, FolderGlyph::Downloads),
        (D::Music, FolderGlyph::Music),
        (D::Pictures, FolderGlyph::Pictures),
        (D::Videos, FolderGlyph::Videos),
        (D::Templates, FolderGlyph::Templates),
        (D::PublicShare, FolderGlyph::Public),
        // Last: a user directory set to home itself is home's own.
        (D::Home, FolderGlyph::Home),
    ] {
        if directories
            .location(directory)
            .and_then(Location::as_local_path)
            .is_some_and(|known| known.as_path() == path)
        {
            // XDG sets an unused user directory to home itself; home wins.
            if glyph != FolderGlyph::Home
                && directories
                    .home()
                    .and_then(Location::as_local_path)
                    .is_some_and(|home| home.as_path() == path)
            {
                return FolderGlyph::Home;
            }
            return glyph;
        }
    }
    if let Some(mount) = mounts.mount_for(path) {
        if mount.mount_point == path && mount.is_probably_external() {
            return FolderGlyph::Device;
        }
        // A GVfs mount is a folder directly inside GVfs's own FUSE mount.
        if mount.filesystem == "fuse.gvfsd-fuse" && path.parent() == Some(&mount.mount_point) {
            return FolderGlyph::Network;
        }
        if mount.mount_point == path && mounts.changes_may_go_unannounced(path) {
            return FolderGlyph::Network;
        }
    }
    let home = directories.home().and_then(Location::as_local_path);
    if let (Some(home), Some(name)) = (home, path.file_name())
        && path.parent() == Some(home.as_path())
    {
        let name = name.to_string_lossy().to_lowercase();
        if DEVELOPER_NAMES.contains(&name.as_str()) {
            return FolderGlyph::Developer;
        }
    }
    FolderGlyph::Folder
}

/// A drive's icon: its kind, and for a removable drive what it is.
pub fn drive_row_icon(drive: &crate::drives::Drive) -> Picture {
    use crate::drives::RemovableForm as F;
    match drive.form {
        Some(F::Stick) => Picture::UsbDrive,
        Some(F::Card) => Picture::MemoryCard,
        Some(F::Disk) => Picture::ExternalDrive,
        None => drive_icon(drive.kind),
    }
}

/// A drive's icon by its kind (ticket 122).
pub fn drive_icon(kind: crate::drives::DriveKind) -> Picture {
    use crate::drives::DriveKind as K;
    match kind {
        K::System => Picture::SystemDrive,
        K::Internal => Picture::Drive,
        K::Removable => Picture::UsbDrive,
        K::Optical => Picture::Disc,
        K::Phone => Picture::Phone,
        K::Network => Picture::Network,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system drive, other internal drives and removable drives each
    /// look different in the sidebar.
    #[test]
    fn the_system_drive_and_a_usb_stick_have_their_own_icons() {
        use crate::drives::DriveKind as K;
        assert_eq!(drive_icon(K::System), Picture::SystemDrive);
        assert_eq!(drive_icon(K::Internal), Picture::Drive);
        assert_eq!(drive_icon(K::Removable), Picture::UsbDrive);
        assert_ne!(drive_icon(K::System), drive_icon(K::Internal));
    }

    /// A USB stick, a memory card and an external disk each have their own
    /// icon, and a drive UDisks2 says nothing about is drawn as a disk.
    #[test]
    fn removable_drives_are_drawn_by_what_they_are() {
        use crate::drives::{RemovableForm as F, removable_form};
        use storage_platform::model::DriveInfo;
        let with = |media: &[&str], bus: &str| DriveInfo {
            media: media.iter().map(|m| m.to_string()).collect(),
            connection_bus: bus.to_string(),
            ..DriveInfo::default()
        };
        assert_eq!(removable_form(&with(&["thumb"], "usb")), F::Stick);
        assert_eq!(removable_form(&with(&["flash_sd"], "usb")), F::Card);
        assert_eq!(removable_form(&with(&[], "sdio")), F::Card);
        assert_eq!(removable_form(&with(&[], "usb")), F::Disk);
        let icons = [
            Picture::UsbDrive,
            Picture::MemoryCard,
            Picture::ExternalDrive,
            Picture::Drive,
            Picture::SystemDrive,
        ];
        for (i, a) in icons.iter().enumerate() {
            for b in &icons[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn every_glyph_is_served_and_is_an_svg() {
        for path in [CODE_MARK, CODE_FOLDER_SMALL] {
            let bytes = FilesAssets.load(path).unwrap().expect(path);
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(
                text.starts_with("<svg") && text.ends_with("</svg>"),
                "{path}"
            );
        }
        // Every folder's picture, and every other picture the window draws,
        // is served from the icon set.
        let pictures = FolderGlyph::ALL
            .iter()
            .map(|glyph| glyph.picture())
            .chain([COMPUTER, OPERATIONS]);
        for picture in pictures {
            assert!(
                FilesAssets.load(picture.path()).unwrap().is_some(),
                "{picture:?}"
            );
        }
        // The kit's own still come through.
        assert!(FilesAssets.load("icons/folder.svg").unwrap().is_some());
    }
}
