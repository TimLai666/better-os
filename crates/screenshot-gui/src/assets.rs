//! The interface's symbolic pictures (area, screen, window, camera, video
//! camera, pointer, stop, close, check), drawn for Better Screenshot and
//! served before the icon set and the kit's own assets. Each is a 16 px mask
//! the button tints.

use std::borrow::Cow;

/// Where each picture is served.
pub mod path {
    pub const AREA: &str = "screenshot/area.svg";
    pub const SCREEN: &str = "screenshot/screen.svg";
    pub const WINDOW: &str = "screenshot/window.svg";
    pub const CAMERA: &str = "screenshot/camera.svg";
    pub const VIDEO: &str = "screenshot/video.svg";
    pub const POINTER: &str = "screenshot/pointer.svg";
    pub const STOP: &str = "screenshot/stop.svg";
    pub const CLOSE: &str = "screenshot/close.svg";
    pub const CHECK: &str = "screenshot/check.svg";
}

const FILES: [(&str, &str); 9] = [
    (path::AREA, include_str!("../assets/icons/area.svg")),
    (path::SCREEN, include_str!("../assets/icons/screen.svg")),
    (path::WINDOW, include_str!("../assets/icons/window.svg")),
    (path::CAMERA, include_str!("../assets/icons/camera.svg")),
    (path::VIDEO, include_str!("../assets/icons/video.svg")),
    (path::POINTER, include_str!("../assets/icons/pointer.svg")),
    (path::STOP, include_str!("../assets/icons/stop.svg")),
    (path::CLOSE, include_str!("../assets/icons/close.svg")),
    (path::CHECK, include_str!("../assets/icons/check.svg")),
];

/// The pictures, then the icon set, then the kit's assets.
pub struct Assets;

impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, svg)) = FILES.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(svg.as_bytes())));
        }
        if let Some(svg) = better_ui::icon_set::load(path) {
            return Ok(Some(Cow::Borrowed(svg)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

#[cfg(test)]
mod tests {
    use super::FILES;

    #[test]
    fn every_picture_is_an_svg_that_starts_early() {
        for (path, svg) in FILES {
            assert!(path.starts_with("screenshot/") && path.ends_with(".svg"));
            // gdk-pixbuf and resvg both sniff the first bytes (AGENTS.md).
            assert!(svg.find("<svg").is_some_and(|at| at < 100), "{path}");
            assert!(svg.contains("viewBox=\"0 0 16 16\""), "{path}");
        }
    }
}
