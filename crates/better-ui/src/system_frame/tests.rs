//! The frame's decisions, tested without GTK, GPUI, or the person's
//! settings: every setting comes from [`FixedSettings`].

use super::{
    ADWAITA_DARK, ADWAITA_LIGHT, Bitmap, ButtonLayout, ColorScheme, DRAG_THRESHOLD,
    DesktopSettings, Edge, FixedSettings, GSettingsListing, INTERFACE_SCHEMA, Insets,
    RESIZE_CORNER, RESIZE_HANDLE, SettingsSource as _, Tiled, TitlebarAction, TitlebarClick,
    TitlebarResponse, WM_SCHEMA, WindowButton, WindowState, X11_MAX_INSET, client_inset,
    fallback_look, fallback_requested, frame_insets, frame_thickness, gsettings_string,
    input_region, is_drag, monitored_key, render_scale, resize_edge, resize_zones, sticky_inset,
    strip_image_span, top_corner_radius,
};

use WindowButton::{Close, Maximize, Minimize};

#[test]
fn buttons_on_the_right_as_zorin_sets_them() {
    let layout = ButtonLayout::parse("appmenu:minimize,maximize,close");
    assert!(layout.start.is_empty());
    assert_eq!(layout.end, [Minimize, Maximize, Close]);
}

#[test]
fn buttons_on_the_left_in_the_order_named() {
    let layout = ButtonLayout::parse("close,minimize,maximize:appmenu");
    assert_eq!(layout.start, [Close, Minimize, Maximize]);
    assert!(layout.end.is_empty());
}

#[test]
fn only_close_as_gnome_sets_it() {
    let layout = ButtonLayout::parse("appmenu:close");
    assert!(layout.start.is_empty());
    assert_eq!(layout.end, [Close]);
    assert_eq!(layout, ButtonLayout::default());
}

#[test]
fn buttons_on_both_sides() {
    let layout = ButtonLayout::parse("close:minimize,maximize");
    assert_eq!(layout.start, [Close]);
    assert_eq!(layout.end, [Minimize, Maximize]);
}

#[test]
fn names_that_draw_nothing_are_skipped_and_a_repeat_is_drawn_once() {
    let layout = ButtonLayout::parse("icon,spacer,close:menu, minimize ,close,bogus");
    assert_eq!(layout.start, [Close]);
    assert_eq!(layout.end, [Minimize]);
}

#[test]
fn a_layout_without_a_colon_is_all_at_the_start_and_an_empty_one_has_no_buttons() {
    assert_eq!(
        ButtonLayout::parse("close,maximize").start,
        [Close, Maximize]
    );
    let empty = ButtonLayout::parse(":");
    assert!(empty.is_empty());
    assert!(ButtonLayout::parse("").is_empty());
}

#[test]
fn a_dialog_keeps_close_on_the_side_the_desktop_puts_it() {
    assert_eq!(
        ButtonLayout::parse("close:minimize").dialog(),
        ButtonLayout {
            start: vec![Close],
            end: vec![]
        }
    );
    // No close at all in the layout: a dialog still gets one, at the end.
    assert_eq!(ButtonLayout::parse("minimize:").dialog().end, [Close]);
}

#[test]
fn each_button_shows_its_symbolic_icon() {
    assert_eq!(Minimize.icon_name(false), "window-minimize-symbolic");
    assert_eq!(Maximize.icon_name(false), "window-maximize-symbolic");
    assert_eq!(Maximize.icon_name(true), "window-restore-symbolic");
    assert_eq!(Close.icon_name(true), "window-close-symbolic");
    assert_eq!(Close.css_class(), "close");
}

#[test]
fn every_title_bar_action_gnome_defines_is_read() {
    for (value, action) in [
        ("toggle-maximize", TitlebarAction::ToggleMaximize),
        (
            "toggle-maximize-horizontally",
            TitlebarAction::ToggleMaximizeHorizontally,
        ),
        (
            "toggle-maximize-vertically",
            TitlebarAction::ToggleMaximizeVertically,
        ),
        ("minimize", TitlebarAction::Minimize),
        ("lower", TitlebarAction::Lower),
        ("menu", TitlebarAction::Menu),
        ("none", TitlebarAction::None),
        ("toggle-shade", TitlebarAction::None),
    ] {
        assert_eq!(TitlebarAction::parse(value), Some(action), "{value}");
    }
    assert_eq!(TitlebarAction::parse("explode"), None);
}

#[test]
fn what_each_title_bar_action_does_on_wayland() {
    use TitlebarResponse as R;
    assert_eq!(TitlebarAction::ToggleMaximize.response(), R::ToggleMaximize);
    assert_eq!(
        TitlebarAction::ToggleMaximizeVertically.response(),
        R::ToggleMaximize
    );
    assert_eq!(TitlebarAction::Minimize.response(), R::Minimize);
    assert_eq!(TitlebarAction::Menu.response(), R::WindowMenu);
    assert_eq!(TitlebarAction::Lower.response(), R::Nothing);
    assert_eq!(TitlebarAction::None.response(), R::Nothing);
}

fn owners_desktop() -> FixedSettings {
    FixedSettings::default()
        .with(INTERFACE_SCHEMA, "gtk-theme", "'ZorinBlue-Dark'")
        .with(INTERFACE_SCHEMA, "icon-theme", "'ZorinBlue-Dark'")
        .with(INTERFACE_SCHEMA, "color-scheme", "'prefer-dark'")
        .with(
            WM_SCHEMA,
            "button-layout",
            "'appmenu:minimize,maximize,close'",
        )
        .with(
            WM_SCHEMA,
            "action-double-click-titlebar",
            "'toggle-maximize'",
        )
        .with(WM_SCHEMA, "action-middle-click-titlebar", "'none'")
        .with(WM_SCHEMA, "action-right-click-titlebar", "'menu'")
}

#[test]
fn the_owners_settings_are_read() {
    let settings = DesktopSettings::read(&owners_desktop());
    assert_eq!(settings.gtk_theme.as_deref(), Some("ZorinBlue-Dark"));
    assert_eq!(settings.icon_theme.as_deref(), Some("ZorinBlue-Dark"));
    assert_eq!(settings.color_scheme, ColorScheme::PreferDark);
    assert!(settings.color_scheme.is_dark());
    assert_eq!(settings.button_layout.end, [Minimize, Maximize, Close]);
    assert_eq!(
        settings.titlebar_response(TitlebarClick::Double),
        TitlebarResponse::ToggleMaximize
    );
    assert_eq!(
        settings.titlebar_response(TitlebarClick::Middle),
        TitlebarResponse::Nothing
    );
    assert_eq!(
        settings.titlebar_response(TitlebarClick::Right),
        TitlebarResponse::WindowMenu
    );
}

#[test]
fn changed_click_actions_are_followed() {
    let settings = DesktopSettings::read(
        &FixedSettings::default()
            .with(WM_SCHEMA, "action-double-click-titlebar", "'minimize'")
            .with(
                WM_SCHEMA,
                "action-middle-click-titlebar",
                "'toggle-maximize'",
            )
            .with(WM_SCHEMA, "action-right-click-titlebar", "'none'"),
    );
    assert_eq!(
        settings.titlebar_response(TitlebarClick::Double),
        TitlebarResponse::Minimize
    );
    assert_eq!(
        settings.titlebar_response(TitlebarClick::Middle),
        TitlebarResponse::ToggleMaximize
    );
    assert_eq!(
        settings.titlebar_response(TitlebarClick::Right),
        TitlebarResponse::Nothing
    );
}

#[test]
fn a_desktop_without_the_settings_gets_gnomes_defaults() {
    let settings = DesktopSettings::read(&FixedSettings::default());
    assert_eq!(settings, DesktopSettings::default());
    assert_eq!(settings.gtk_theme, None);
    assert_eq!(settings.button_layout.end, [Close]);
    assert_eq!(settings.color_scheme, ColorScheme::Default);
    assert!(!settings.color_scheme.is_dark());
}

#[test]
fn values_gnome_does_not_define_fall_back_to_the_default() {
    let settings = DesktopSettings::read(
        &FixedSettings::default()
            .with(INTERFACE_SCHEMA, "color-scheme", "'purple'")
            .with(INTERFACE_SCHEMA, "gtk-theme", "'../../etc'")
            .with(INTERFACE_SCHEMA, "icon-theme", "''")
            .with(WM_SCHEMA, "action-double-click-titlebar", "'launch'")
            .with(WM_SCHEMA, "button-layout", "@as []"),
    );
    assert_eq!(settings, DesktopSettings::default());
}

#[test]
fn a_gsettings_string_is_unquoted_and_anything_else_is_not_a_string() {
    assert_eq!(gsettings_string("'close:'\n").as_deref(), Some("close:"));
    assert_eq!(gsettings_string("\"a\"").as_deref(), Some("a"));
    assert_eq!(gsettings_string("uint32 3"), None);
    assert_eq!(gsettings_string("true"), None);
}

#[test]
fn a_monitor_line_names_the_key_that_changed() {
    assert_eq!(
        monitored_key("button-layout: 'close:'"),
        Some("button-layout")
    );
    assert_eq!(
        monitored_key("color-scheme: 'prefer-dark'"),
        Some("color-scheme")
    );
    assert_eq!(monitored_key("cursor-size: 24"), None);
    assert_eq!(monitored_key("no colon here"), None);
}

#[test]
fn the_settings_are_only_ever_read() {
    // Ticket 108: tests and the kit never change the person's GNOME
    // settings. The only commands the frame runs are `gsettings get`,
    // `gsettings list-recursively`, and `gsettings monitor`, and GTK is never
    // asked to write one. (`g_object_set` on `GtkSettings` changes that
    // in-process object only; it is not a stored setting.)
    let source = include_str!("../system_frame.rs");
    let service = include_str!("service.rs");
    let gtk = include_str!("gtk.rs");
    for text in [source, service, gtk] {
        for verb in [
            "\"set\"",
            "\"reset\"",
            "\"reset-recursively\"",
            "\"writable\"",
            "dconf write",
            "dconf reset",
            "g_settings_set",
            "g_settings_reset",
            "g_settings_apply",
            "dconf_client_write",
        ] {
            assert!(!text.contains(verb), "{verb} must not appear");
        }
    }
    assert!(source.contains(r#".args(["get", schema, key])"#));
    assert!(source.contains(r#".args(["list-recursively", schema])"#));
    assert!(service.contains(r#".args(["monitor", schema])"#));
}

#[test]
fn a_settings_listing_is_read_like_single_gets() {
    let listing = GSettingsListing::parse(
        "org.gnome.desktop.interface gtk-theme 'ZorinBlue-Dark'\n\
         org.gnome.desktop.interface color-scheme 'prefer-dark'\n\
         org.gnome.desktop.wm.preferences button-layout 'appmenu:minimize,maximize,close'\n\
         org.gnome.desktop.wm.preferences action-right-click-titlebar 'none'\n\
         garbage\n",
    );
    assert_eq!(
        listing.get(WM_SCHEMA, "button-layout").as_deref(),
        Some("'appmenu:minimize,maximize,close'")
    );
    assert_eq!(listing.get(WM_SCHEMA, "missing"), None);
    let settings = DesktopSettings::read(&listing);
    assert_eq!(settings.gtk_theme.as_deref(), Some("ZorinBlue-Dark"));
    assert!(settings.color_scheme.is_dark());
    assert_eq!(settings.button_layout.end, [Minimize, Maximize, Close]);
    assert_eq!(
        settings.titlebar_response(TitlebarClick::Right),
        TitlebarResponse::Nothing
    );
    assert_eq!(
        DesktopSettings::read(&GSettingsListing::parse("")),
        DesktopSettings::default()
    );
}

const SIZE: f32 = 1000.0;

fn inset() -> Insets {
    Insets::uniform(40.0)
}

fn edge_at(x: f32, y: f32) -> Option<Edge> {
    resize_edge(x, y, SIZE, SIZE, inset(), Tiled::default(), false)
}

#[test]
fn every_edge_and_corner_resizes_within_gtks_handle() {
    let (l, t, r, b) = (40.0, 40.0, 960.0, 960.0);
    assert_eq!(edge_at(l - 1.0, 500.0), Some(Edge::Left));
    assert_eq!(edge_at(r + 1.0, 500.0), Some(Edge::Right));
    assert_eq!(edge_at(500.0, t - 1.0), Some(Edge::Top));
    assert_eq!(edge_at(500.0, b + 1.0), Some(Edge::Bottom));
    assert_eq!(edge_at(l - 1.0, t - 1.0), Some(Edge::TopLeft));
    assert_eq!(edge_at(r + 1.0, t - 1.0), Some(Edge::TopRight));
    assert_eq!(edge_at(l - 1.0, b + 1.0), Some(Edge::BottomLeft));
    assert_eq!(edge_at(r + 1.0, b + 1.0), Some(Edge::BottomRight));
    // A corner reaches RESIZE_CORNER along each edge next to it.
    assert_eq!(
        edge_at(l + RESIZE_CORNER - 1.0, t - 1.0),
        Some(Edge::TopLeft)
    );
    assert_eq!(edge_at(l + RESIZE_CORNER + 1.0, t - 1.0), Some(Edge::Top));
    assert_eq!(
        edge_at(l - 1.0, b - RESIZE_CORNER + 1.0),
        Some(Edge::BottomLeft)
    );
    // The handle is RESIZE_HANDLE wide, outside the window only.
    assert_eq!(edge_at(l - RESIZE_HANDLE, 500.0), Some(Edge::Left));
    assert_eq!(edge_at(l - RESIZE_HANDLE - 1.0, 500.0), None);
    assert_eq!(edge_at(l + 1.0, 500.0), None);
    assert_eq!(edge_at(500.0, 500.0), None);
}

#[test]
fn a_maximized_window_or_a_tiled_side_does_not_resize() {
    assert_eq!(
        resize_edge(39.0, 500.0, SIZE, SIZE, inset(), Tiled::default(), true),
        None
    );
    assert_eq!(
        resize_edge(500.0, 39.0, SIZE, SIZE, inset(), Tiled::ALL, false),
        None
    );
    let left = Tiled {
        left: true,
        ..Tiled::default()
    };
    assert_eq!(
        resize_edge(39.0, 500.0, SIZE, SIZE, inset(), left, false),
        None
    );
    assert_eq!(
        resize_edge(39.0, 39.0, SIZE, SIZE, inset(), left, false),
        None
    );
    assert_eq!(
        resize_edge(961.0, 500.0, SIZE, SIZE, inset(), left, false),
        Some(Edge::Right)
    );
}

#[test]
fn the_handle_never_reaches_past_the_surface() {
    let narrow = Insets::uniform(5.0);
    assert_eq!(
        resize_edge(0.0, 500.0, SIZE, SIZE, narrow, Tiled::default(), false),
        Some(Edge::Left)
    );
    assert_eq!(
        resize_edge(-1.0, 500.0, SIZE, SIZE, narrow, Tiled::default(), false),
        None
    );
}

#[test]
fn every_resize_zone_resizes_from_its_own_edge() {
    let zones = resize_zones(SIZE, SIZE, inset(), Tiled::default(), false);
    let mut edges: Vec<Edge> = zones.iter().map(|(edge, _)| *edge).collect();
    edges.dedup();
    for edge in [
        Edge::Top,
        Edge::Right,
        Edge::Bottom,
        Edge::Left,
        Edge::TopLeft,
        Edge::TopRight,
        Edge::BottomLeft,
        Edge::BottomRight,
    ] {
        assert!(edges.contains(&edge), "{edge:?} has a zone");
    }
    for (edge, (x, y, w, h)) in &zones {
        // Corners are painted after edges and win where they overlap, so a
        // corner's every inner point is that corner; an edge's middle is it.
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let probe = match edge {
            Edge::Top | Edge::Bottom | Edge::Left | Edge::Right => (cx, cy),
            _ => (x + w.min(2.0) / 2.0, y + h.min(2.0) / 2.0),
        };
        let found = edge_at(probe.0, probe.1);
        if matches!(edge, Edge::Top | Edge::Bottom | Edge::Left | Edge::Right) {
            assert_eq!(found, Some(*edge), "middle of the {edge:?} zone");
        } else {
            assert_eq!(found, Some(*edge), "inside the {edge:?} zone");
        }
    }
    assert!(resize_zones(SIZE, SIZE, inset(), Tiled::default(), true).is_empty());
    assert!(resize_zones(SIZE, SIZE, inset(), Tiled::ALL, false).is_empty());
}

#[test]
fn input_reaches_only_the_window_and_its_handles() {
    assert_eq!(
        input_region(SIZE, SIZE, inset()),
        (28.0, 28.0, 944.0, 944.0)
    );
    assert_eq!(
        input_region(SIZE, SIZE, Insets::ZERO),
        (0.0, 0.0, SIZE, SIZE)
    );
}

#[test]
fn the_inset_is_the_widest_shadow_and_never_less_than_the_handle() {
    let shadow = Insets {
        top: 22.0,
        right: 30.5,
        bottom: 40.0,
        left: 30.5,
    };
    assert_eq!(client_inset(shadow), 40.0);
    assert_eq!(client_inset(Insets::uniform(3.0)), RESIZE_HANDLE);
}

#[test]
fn cairo_pixels_become_straight_alpha_rgba() {
    // One premultiplied ARGB32 pixel: 50% white is 0x80808080.
    let half_white = 0x8080_8080u32.to_ne_bytes();
    let opaque_red = 0xffff_0000u32.to_ne_bytes();
    let mut data = Vec::new();
    data.extend_from_slice(&half_white);
    data.extend_from_slice(&opaque_red);
    data.extend_from_slice(&[0; 8]); // padding to the stride
    let bitmap = Bitmap::from_cairo_argb32(&data, 2, 1, 16);
    assert_eq!(bitmap.pixel(0, 0), [255, 255, 255, 128]);
    assert_eq!(bitmap.pixel(1, 0), [255, 0, 0, 255]);
    assert_eq!(bitmap.pixel(5, 5), [0, 0, 0, 0]);
}

/// A frame image as GTK draws one: `frame` pixels of shadow fading in from
/// each edge, `reach` pixels deep, around an opaque window.
fn synthetic_frame(frame: u32, reach: Insets) -> Bitmap {
    let size = 4 * frame;
    let mut bitmap = Bitmap::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let inside_x = x >= frame && x < 3 * frame;
            let inside_y = y >= frame && y < 3 * frame;
            let alpha = if inside_x && inside_y {
                255
            } else {
                let dx = if x < frame {
                    (frame - x) as f32 <= reach.left
                } else if x >= 3 * frame {
                    (x - 3 * frame + 1) as f32 <= reach.right
                } else {
                    true
                };
                let dy = if y < frame {
                    (frame - y) as f32 <= reach.top
                } else if y >= 3 * frame {
                    (y - 3 * frame + 1) as f32 <= reach.bottom
                } else {
                    true
                };
                if dx && dy { 40 } else { 0 }
            };
            let at = ((y * size + x) * 4 + 3) as usize;
            bitmap.rgba[at] = alpha;
        }
    }
    bitmap
}

#[test]
fn the_shadow_reach_is_measured_on_each_side() {
    let reach = Insets {
        top: 20.0,
        right: 25.0,
        bottom: 31.0,
        left: 25.0,
    };
    assert_eq!(frame_thickness(&synthetic_frame(64, reach), 64), reach);
    assert_eq!(
        frame_thickness(&synthetic_frame(64, Insets::ZERO), 64),
        Insets::ZERO
    );
}

#[test]
fn the_corner_radius_is_where_the_sample_turns_opaque() {
    let size = 32;
    let radius = 12.0f32;
    let mut sample = Bitmap::new(size, size);
    for y in 0..size {
        for x in 0..size {
            // Opaque only where the whole pixel is inside the arc, as GTK's
            // antialiased corner leaves it: its outer corner is inside.
            let (dx, dy) = (radius - x as f32, radius - y as f32);
            let outside =
                x < radius as u32 && y < radius as u32 && (dx * dx + dy * dy).sqrt() > radius;
            let at = ((y * size + x) * 4 + 3) as usize;
            sample.rgba[at] = if outside { 0 } else { 255 };
        }
    }
    assert_eq!(top_corner_radius(&sample), 12);
    assert_eq!(top_corner_radius(&Bitmap::new(8, 8)), 8);
    let square = Bitmap {
        width: 4,
        height: 4,
        rgba: vec![255; 64],
    };
    assert_eq!(top_corner_radius(&square), 0);
}

#[test]
fn gtk_draws_at_the_next_whole_scale() {
    assert_eq!(render_scale(1.0), 1);
    assert_eq!(render_scale(1.25), 2);
    assert_eq!(render_scale(2.0), 2);
    assert_eq!(render_scale(0.0), 1);
    assert_eq!(render_scale(f32::NAN), 1);
    assert_eq!(render_scale(9.0), 4);
}

#[test]
fn without_gtk_the_frame_is_adwaitas_and_never_the_windows_one() {
    for (look, dark) in [(&ADWAITA_LIGHT, false), (&ADWAITA_DARK, true)] {
        assert_eq!(fallback_look(dark), look);
        // libadwaita 1.5's window radius and header bar height.
        assert_eq!(look.corner_radius, 12.0);
        assert_eq!(look.header_height, 47.0);
        // Round buttons in the text colour, not Windows' 46px caption
        // buttons with a red close.
        assert_eq!(look.button_fill, [0.1, 0.15, 0.3]);
        assert!(look.button_size < 46.0);
        assert!(look.header.g == look.header.r && look.header.b == look.header.r);
    }
    assert_eq!(ADWAITA_LIGHT.header, super::Color::rgb(0xffffff));
    assert_eq!(ADWAITA_DARK.header, super::Color::rgb(0x303030));
}

#[test]
fn the_fallback_can_be_asked_for_by_name() {
    assert!(fallback_requested(Some("adwaita")));
    assert!(fallback_requested(Some("fallback")));
    assert!(!fallback_requested(Some("gtk")));
    assert!(!fallback_requested(None));
}

#[test]
fn every_resize_zone_lies_outside_the_window_and_is_one_handle_thick() {
    for inset in [
        Insets::uniform(40.0),
        Insets::uniform(5.0),
        Insets::uniform(12.0),
    ] {
        let (l, t, r, b) = (
            inset.left,
            inset.top,
            SIZE - inset.right,
            SIZE - inset.bottom,
        );
        let handle = inset.left.min(RESIZE_HANDLE);
        for (edge, (x, y, w, h)) in resize_zones(SIZE, SIZE, inset, Tiled::default(), false) {
            // Entirely outside the content rectangle.
            let outside = x + w <= l || x >= r || y + h <= t || y >= b;
            assert!(outside, "{edge:?} zone {x},{y} {w}x{h} overlaps the window");
            // And exactly one handle thick across its edge.
            let thickness = match edge {
                Edge::Top | Edge::Bottom => h,
                Edge::Left | Edge::Right => w,
                _ => w.min(h),
            };
            assert_eq!(thickness, handle, "{edge:?} at inset {}", inset.left);
            // Never past the surface.
            assert!(x >= 0.0 && y >= 0.0 && x + w <= SIZE && y + h <= SIZE);
        }
    }
}

#[test]
fn a_tiled_side_has_no_zone() {
    let left = Tiled {
        left: true,
        ..Tiled::default()
    };
    let zones = resize_zones(SIZE, SIZE, frame_insets(40.0, left, false), left, false);
    assert!(
        zones
            .iter()
            .all(|(edge, _)| !matches!(edge, Edge::Left | Edge::TopLeft | Edge::BottomLeft))
    );
    assert!(zones.iter().any(|(edge, _)| *edge == Edge::Right));
}

#[test]
fn a_window_state_names_maximized_fullscreen_and_tiled_windows() {
    let tiled_left = Tiled {
        left: true,
        ..Tiled::default()
    };
    let floating = WindowState::new(Some(Tiled::default()), false, false, true);
    assert!(floating.client_decorated && !floating.maximized && floating.active);
    assert!(WindowState::new(Some(Tiled::default()), true, false, true).maximized);
    assert!(WindowState::new(Some(Tiled::default()), false, true, false).maximized);
    assert!(WindowState::new(Some(Tiled::ALL), false, false, true).maximized);
    let half = WindowState::new(Some(tiled_left), false, false, true);
    assert!(!half.maximized && half.tiled.left);
    let server = WindowState::new(None, false, false, true);
    assert!(!server.client_decorated);
    assert_eq!(half.restored().tiled, Tiled::default());
    assert!(
        !WindowState::new(Some(Tiled::ALL), true, false, true)
            .restored()
            .maximized
    );
}

#[test]
fn a_frame_key_names_the_theme_scheme_scale_and_state() {
    let settings = DesktopSettings::read(&owners_desktop());
    let floating = WindowState::new(Some(Tiled::default()), false, false, true);
    let key = floating.key(&settings, true, 1.25);
    assert_eq!(key.gtk_theme.as_deref(), Some("ZorinBlue-Dark"));
    assert_eq!(key.icon_theme.as_deref(), Some("ZorinBlue-Dark"));
    assert!(key.dark);
    assert_eq!(key.scale, 2);
    assert!(!key.maximized && !key.tiled);
    let left = Tiled {
        left: true,
        ..Tiled::default()
    };
    let tiled = WindowState::new(Some(left), false, false, true).key(&settings, false, 1.0);
    assert!(tiled.tiled && !tiled.maximized);
    let maximized =
        WindowState::new(Some(Tiled::ALL), true, false, true).key(&settings, false, 1.0);
    assert!(maximized.maximized && !maximized.tiled);
    // Focus is not part of the key: both states are in one drawing.
    let unfocused = WindowState::new(Some(Tiled::default()), false, false, false);
    assert_eq!(unfocused.key(&settings, true, 1.25), key);
}

#[test]
fn the_siblings_of_a_key_are_its_other_states_and_scheme() {
    let settings = DesktopSettings::read(&owners_desktop());
    let key =
        WindowState::new(Some(Tiled::default()), false, false, true).key(&settings, true, 1.0);
    let siblings = key.siblings();
    assert_eq!(siblings.len(), 5);
    assert!(!siblings.contains(&key));
    assert!(
        siblings
            .iter()
            .all(|s| s.scale == key.scale && s.gtk_theme == key.gtk_theme)
    );
    assert!(siblings.iter().any(|s| s.maximized && s.dark));
    assert!(siblings.iter().any(|s| s.tiled && !s.dark));
    assert!(siblings.iter().any(|s| !s.maximized && !s.tiled && !s.dark));
}

#[test]
fn a_tiled_or_maximized_side_has_no_frame() {
    let left = Tiled {
        left: true,
        ..Tiled::default()
    };
    assert_eq!(
        frame_insets(47.0, Tiled::default(), false),
        Insets::uniform(47.0)
    );
    let insets = frame_insets(47.0, left, false);
    assert_eq!((insets.left, insets.top, insets.right), (0.0, 47.0, 47.0));
    assert_eq!(frame_insets(47.0, Tiled::default(), true), Insets::ZERO);
}

#[test]
fn the_inset_never_shrinks_and_stays_small_on_x11() {
    assert_eq!(sticky_inset(None, 47.0, false), 47.0);
    // GTK arrives after the fallback asked for more: keep the larger one.
    assert_eq!(sticky_inset(Some(64.0), 47.0, false), 64.0);
    assert_eq!(sticky_inset(Some(40.0), 47.0, false), 47.0);
    assert_eq!(sticky_inset(None, 64.0, true), X11_MAX_INSET);
    assert_eq!(sticky_inset(Some(20.0), 64.0, true), X11_MAX_INSET);
    // The fallback asks for the most GTK's drawing can need.
    assert_eq!(client_inset(ADWAITA_LIGHT.shadow), super::MAX_SHADOW);
}

#[test]
fn a_press_becomes_a_drag_past_gtks_threshold() {
    assert!(!is_drag((10.0, 10.0), (10.0, 10.0)));
    assert!(!is_drag((10.0, 10.0), (10.0 + DRAG_THRESHOLD, 10.0)));
    assert!(is_drag((10.0, 10.0), (10.0 + DRAG_THRESHOLD + 1.0, 10.0)));
    assert!(is_drag((10.0, 10.0), (17.0, 17.0)));
}

#[test]
fn a_strip_is_sampled_only_at_its_middle_texel() {
    // Four texels over 400px: the 100px painted start 150px into the image,
    // which is texel 1.5 to 2.5.
    assert_eq!(strip_image_span(100.0, 100.0), (-50.0, 400.0));
    let (start, length) = strip_image_span(30.0, 10.0);
    let texel = length / super::STRIP_TEXELS as f32;
    assert_eq!((30.0 - start) / texel, 1.5);
    assert_eq!((40.0 - start) / texel, 2.5);
}
