//! The few glyphs the kit's icon set does not have, drawn in the same 24px,
//! 2px-stroke grid as the rest (Lucide's shapes, ISC), and served beside
//! the kit's own icons: a trash can, and one icon for each of the person's
//! own folders so a compact sidebar tells them apart, and the file operations
//! panel (two arrows passing, where the kit's nearest was a spinner). The
//! shared ones every window has (Refresh) come from `better_ui::fluent::glyphs`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub const TRASH: &str = "better-files/trash.svg";
pub const DOCUMENTS: &str = "better-files/documents.svg";
pub const DOWNLOADS: &str = "better-files/downloads.svg";
pub const PICTURES: &str = "better-files/pictures.svg";
pub const MUSIC: &str = "better-files/music.svg";
pub const VIDEOS: &str = "better-files/videos.svg";
pub const DESKTOP: &str = "better-files/desktop.svg";
pub const OPERATIONS: &str = "better-files/operations.svg";
pub const HOME: &str = "better-files/home.svg";
pub const TEMPLATES: &str = "better-files/templates.svg";
pub const PUBLIC: &str = "better-files/public.svg";
pub const DEVELOPER: &str = "better-files/developer.svg";
/// The developer glyph's brackets alone, the mark the content view puts on a
/// full-colour folder.
pub const CODE_MARK: &str = "better-files/code-mark.svg";
/// The kit's own, named here so every folder glyph is a path.
pub const FOLDER: &str = "icons/folder.svg";
pub const DEVICE: &str = "icons/hard-drive.svg";
pub const NETWORK: &str = "icons/globe.svg";

const OPEN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#;

fn svg(path: &str) -> Option<String> {
    let body = match path {
        TRASH => {
            r#"<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M10 11v6"/><path d="M14 11v6"/>"#
        }
        DOCUMENTS => {
            r#"<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/><path d="M16 13H8"/><path d="M16 17H8"/><path d="M10 9H8"/>"#
        }
        DOWNLOADS => {
            r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="M7 10l5 5 5-5"/><path d="M12 15V3"/>"#
        }
        PICTURES => {
            r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="M21 15l-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>"#
        }
        MUSIC => {
            r#"<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>"#
        }
        VIDEOS => {
            r#"<path d="M16 13l5.2 3.5a.5.5 0 0 0 .8-.4V7.9a.5.5 0 0 0-.8-.4L16 11"/><rect x="2" y="6" width="14" height="12" rx="2"/>"#
        }
        DESKTOP => {
            r#"<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>"#
        }
        // Lucide's arrow-right-left: things moving between two places.
        OPERATIONS => {
            r#"<path d="m16 3 4 4-4 4"/><path d="M20 7H4"/><path d="m8 21-4-4 4-4"/><path d="M4 17h16"/>"#
        }
        // A house: a roof and a door.
        HOME => {
            r#"<path d="M3 10.5 12 3l9 7.5"/><path d="M5 9v10.5a1.5 1.5 0 0 0 1.5 1.5h11a1.5 1.5 0 0 0 1.5-1.5V9"/><path d="M10 21v-6h4v6"/>"#
        }
        // A sheet laid out in blocks.
        TEMPLATES => {
            r#"<rect x="4" y="3" width="16" height="18" rx="2"/><path d="M4 9h16"/><path d="M11 9v12"/>"#
        }
        // A folder with a person on it: what is shared.
        PUBLIC => {
            r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><circle cx="12" cy="11.5" r="2"/><path d="M8.5 17.5a3.5 3.5 0 0 1 7 0"/>"#
        }
        // A folder with angle brackets on it: where code lives.
        DEVELOPER => {
            r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="m10 11-2.5 2.5L10 16"/><path d="m14 11 2.5 2.5L14 16"/>"#
        }
        CODE_MARK => r#"<path d="m8 7-5 5 5 5"/><path d="m16 7 5 5-5 5"/><path d="m14 4-4 16"/>"#,
        _ => return None,
    };
    Some(format!("{OPEN}{body}</svg>"))
}

/// The kit's icons and the window's own.
pub struct FilesAssets;

impl AssetSource for FilesAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(svg) = svg(path) {
            return Ok(Some(Cow::Owned(svg.into_bytes())));
        }
        if let Some(svg) = better_ui::fluent::glyphs::load(path) {
            return Ok(Some(Cow::Borrowed(svg)));
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

    /// The monochrome glyph the sidebar, path bar and tabs draw.
    pub fn svg(self) -> &'static str {
        match self {
            FolderGlyph::Home => HOME,
            FolderGlyph::Desktop => DESKTOP,
            FolderGlyph::Documents => DOCUMENTS,
            FolderGlyph::Downloads => DOWNLOADS,
            FolderGlyph::Music => MUSIC,
            FolderGlyph::Pictures => PICTURES,
            FolderGlyph::Videos => VIDEOS,
            FolderGlyph::Templates => TEMPLATES,
            FolderGlyph::Public => PUBLIC,
            FolderGlyph::Trash => TRASH,
            FolderGlyph::Device => DEVICE,
            FolderGlyph::Network => NETWORK,
            FolderGlyph::Developer => DEVELOPER,
            FolderGlyph::Folder => FOLDER,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_served_and_is_an_svg() {
        for path in [
            TRASH, DOCUMENTS, DOWNLOADS, PICTURES, MUSIC, VIDEOS, DESKTOP, OPERATIONS, HOME,
            TEMPLATES, PUBLIC, DEVELOPER, CODE_MARK,
        ] {
            let bytes = FilesAssets.load(path).unwrap().expect(path);
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(
                text.starts_with("<svg") && text.ends_with("</svg>"),
                "{path}"
            );
        }
        // Every folder glyph is served, the kit's ones included.
        for glyph in FolderGlyph::ALL {
            assert!(
                FilesAssets.load(glyph.svg()).unwrap().is_some(),
                "{glyph:?}"
            );
        }
        // The shared ones and the kit's own still come through.
        assert!(
            FilesAssets
                .load(better_ui::fluent::glyphs::REFRESH)
                .unwrap()
                .is_some()
        );
        assert!(FilesAssets.load("icons/folder.svg").unwrap().is_some());
    }
}
