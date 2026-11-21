//! COSMIC's own protocols that Better Screenshot needs, generated from the
//! XML in `protocols/` (copied from `pop-os/cosmic-protocols` at commit
//! 815b2c5, `unstable/`; both files carry the X11-style permission notice at
//! their top). `cosmic-comp` 0.1~1795167359 offers
//! `zcosmic_toplevel_info_v1` at version 3: it says which output and
//! workspace a window is on and whether it has the keyboard, which the
//! standard `ext_foreign_toplevel_list_v1` does not. The workspace protocol
//! is here only because version 1 of the toplevel events names its handle.

#![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#![allow(non_upper_case_globals, non_snake_case, unused_imports)]
#![allow(missing_docs, clippy::all)]

pub mod workspace_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/cosmic-workspace-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/cosmic-workspace-unstable-v1.xml");
}

pub mod toplevel_info_v1 {
    use super::workspace_v1::*;
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols::ext::foreign_toplevel_list::v1::client::*;
    use wayland_protocols::ext::workspace::v1::client::*;

    pub mod __interfaces {
        use super::super::workspace_v1::__interfaces::*;
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols::ext::foreign_toplevel_list::v1::client::__interfaces::*;
        use wayland_protocols::ext::workspace::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/cosmic-toplevel-info-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/cosmic-toplevel-info-unstable-v1.xml");
}
