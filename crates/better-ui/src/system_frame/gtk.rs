//! Asking GTK 4 to draw the frame and the title buttons, offscreen.
//!
//! GTK is opened with `dlopen` at run time, so the binary does not link it and
//! a machine without it gets the built-in Adwaita look instead. libadwaita is
//! opened too when it is installed, because GNOME's own applications are
//! libadwaita applications and libadwaita, not plain GTK, decides how their
//! frame looks (Zorin's build of it applies the Zorin theme). Every call
//! happens on one thread, the [`Renderer`]'s owner, which also runs GLib's
//! default main context so GTK's display connection is read and theme changes
//! arrive. GTK never creates a window: each CSS node is an unrealized `GtkBox`
//! with the node's name, classes, and state, the way Chromium builds them
//! (`AppendCssNodeToStyleContextImpl` in `ui/gtk/gtk_util.cc`), and each part
//! is drawn with `gtk_render_background` and `gtk_render_frame` into a Cairo
//! image surface.

use std::ffi::{CStr, CString, c_char, c_double, c_int, c_uint, c_void};

use super::{Bitmap, ButtonState, Color, FrameKey, Insets, WindowButton};

type Ptr = *mut c_void;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct GtkBorder {
    left: i16,
    right: i16,
    top: i16,
    bottom: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct GdkRgba {
    red: f32,
    green: f32,
    blue: f32,
    alpha: f32,
}

/// GLib's `GPollFD` on Unix.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct GPollFd {
    fd: c_int,
    events: u16,
    revents: u16,
}

const STATE_NORMAL: c_uint = 0;
const STATE_ACTIVE: c_uint = 1;
const STATE_PRELIGHT: c_uint = 2;
const STATE_BACKDROP: c_uint = 64;
const ORIENTATION_HORIZONTAL: c_int = 0;
const ORIENTATION_VERTICAL: c_int = 1;
const CAIRO_FORMAT_ARGB32: c_int = 0;
const CAIRO_OPERATOR_IN: c_int = 5;
const ADW_FORCE_LIGHT: c_int = 1;
const ADW_FORCE_DARK: c_int = 4;
const PRIORITY_USER: c_uint = 800;

macro_rules! symbols {
    ($($name:ident : fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        #[allow(non_snake_case)]
        struct Gtk {
            $($name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
            g_object_new: unsafe extern "C" fn(usize, *const c_char, ...) -> Ptr,
            g_object_set: unsafe extern "C" fn(Ptr, *const c_char, ...),
        }

        impl Gtk {
            /// # Safety
            /// `handle` is a live `dlopen` handle of libgtk-4.
            unsafe fn load(handle: Ptr) -> Result<Gtk, String> {
                unsafe fn sym(handle: Ptr, name: &str) -> Result<*mut c_void, String> {
                    let c = CString::new(name).map_err(|e| e.to_string())?;
                    let found = unsafe { libc::dlsym(handle, c.as_ptr()) };
                    if found.is_null() {
                        Err(format!("{name} is missing"))
                    } else {
                        Ok(found)
                    }
                }
                Ok(Gtk {
                    $($name: unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(sym(handle, stringify!($name))?) },)*
                    g_object_new: unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(usize, *const c_char, ...) -> Ptr>(sym(handle, "g_object_new")?) },
                    g_object_set: unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(Ptr, *const c_char, ...)>(sym(handle, "g_object_set")?) },
                })
            }
        }
    };
}

symbols! {
    gtk_disable_setlocale: fn();
    gtk_init_check: fn() -> c_int;
    gtk_get_minor_version: fn() -> c_uint;
    gtk_box_get_type: fn() -> usize;
    gtk_widget_set_css_classes: fn(Ptr, *const *const c_char);
    gtk_widget_set_state_flags: fn(Ptr, c_uint, c_int);
    gtk_widget_set_parent: fn(Ptr, Ptr);
    gtk_widget_get_style_context: fn(Ptr) -> Ptr;
    gtk_widget_measure: fn(Ptr, c_int, c_int, *mut c_int, *mut c_int, *mut c_int, *mut c_int);
    gtk_style_context_add_provider: fn(Ptr, Ptr, c_uint);
    gtk_style_context_get_padding: fn(Ptr, *mut GtkBorder);
    gtk_style_context_get_border: fn(Ptr, *mut GtkBorder);
    gtk_style_context_get_margin: fn(Ptr, *mut GtkBorder);
    gtk_style_context_get_color: fn(Ptr, *mut GdkRgba);
    gtk_style_context_lookup_color: fn(Ptr, *const c_char, *mut GdkRgba) -> c_int;
    gtk_style_context_set_scale: fn(Ptr, c_int);
    gtk_render_background: fn(Ptr, Ptr, c_double, c_double, c_double, c_double);
    gtk_render_frame: fn(Ptr, Ptr, c_double, c_double, c_double, c_double);
    gtk_css_provider_new: fn() -> Ptr;
    gtk_css_provider_load_from_data: fn(Ptr, *const c_char, isize);
    gtk_settings_get_default: fn() -> Ptr;
    gtk_icon_theme_get_for_display: fn(Ptr) -> Ptr;
    gtk_icon_theme_lookup_icon: fn(Ptr, *const c_char, *const *const c_char, c_int, c_int, c_int, c_uint) -> Ptr;
    gtk_snapshot_new: fn() -> Ptr;
    gtk_snapshot_free_to_node: fn(Ptr) -> Ptr;
    gdk_display_get_default: fn() -> Ptr;
    gdk_paintable_snapshot: fn(Ptr, Ptr, c_double, c_double);
    gsk_render_node_draw: fn(Ptr, Ptr);
    gsk_render_node_unref: fn(Ptr);
    g_object_ref_sink: fn(Ptr) -> Ptr;
    g_object_unref: fn(Ptr);
    g_main_context_iteration: fn(Ptr, c_int) -> c_int;
    g_main_context_default: fn() -> Ptr;
    g_main_context_acquire: fn(Ptr) -> c_int;
    g_main_context_prepare: fn(Ptr, *mut c_int) -> c_int;
    g_main_context_query: fn(Ptr, c_int, *mut c_int, *mut GPollFd, c_int) -> c_int;
    g_main_context_check: fn(Ptr, c_int, *mut GPollFd, c_int) -> c_int;
    g_main_context_dispatch: fn(Ptr);
    cairo_image_surface_create: fn(c_int, c_int, c_int) -> Ptr;
    cairo_image_surface_get_data: fn(Ptr) -> *mut u8;
    cairo_image_surface_get_stride: fn(Ptr) -> c_int;
    cairo_surface_flush: fn(Ptr);
    cairo_surface_destroy: fn(Ptr);
    cairo_create: fn(Ptr) -> Ptr;
    cairo_destroy: fn(Ptr);
    cairo_scale: fn(Ptr, c_double, c_double);
    cairo_translate: fn(Ptr, c_double, c_double);
    cairo_save: fn(Ptr);
    cairo_restore: fn(Ptr);
    cairo_set_source_rgba: fn(Ptr, c_double, c_double, c_double, c_double);
    cairo_set_operator: fn(Ptr, c_int);
    cairo_paint: fn(Ptr);
    cairo_push_group: fn(Ptr);
    cairo_pop_group_to_source: fn(Ptr);
    cairo_new_path: fn(Ptr);
    cairo_move_to: fn(Ptr, c_double, c_double);
    cairo_line_to: fn(Ptr, c_double, c_double);
    cairo_arc: fn(Ptr, c_double, c_double, c_double, c_double, c_double);
    cairo_close_path: fn(Ptr);
    cairo_clip: fn(Ptr);
}

struct Adw {
    style_manager_get_default: unsafe extern "C" fn() -> Ptr,
    style_manager_set_color_scheme: unsafe extern "C" fn(Ptr, c_int),
}

/// How far outside the window the frame image reaches, in logical pixels.
/// A theme whose shadow is wider is clipped, as in Chromium
/// (`kMaxFrameSizeDip`).
pub const MAX_FRAME: u32 = super::MAX_SHADOW as u32;
/// The widest top corner the radius probe measures, as in Chromium.
const MAX_CORNER: u32 = 32;
/// Icon size in a title button: GTK's header bar uses 16px.
const ICON_SIZE: u32 = 16;

/// A button image set: one per button state, focused and in the background.
#[derive(Clone, Debug)]
pub struct ButtonImages {
    pub button: WindowButton,
    /// Drawn with `window-restore-symbolic`.
    pub restore: bool,
    /// Indexed by [`ButtonState`] order; `[focused, backdrop]`.
    pub images: [[Bitmap; 2]; 3],
}

/// Everything GTK drew for one [`FrameKey`].
#[derive(Clone, Debug)]
pub struct Rendered {
    pub key: FrameKey,
    /// The whole scale the images are drawn at.
    pub scale: u32,
    /// The frame images are `4 * frame` pixels square, the window's content
    /// edge `frame` pixels in from each side. `[focused, backdrop]`.
    pub frame: [Bitmap; 2],
    pub frame_px: u32,
    /// How far the drawn frame reaches past the window, logical pixels.
    pub thickness: Insets,
    /// The window's top corner radius, logical pixels.
    pub corner_radius: f32,
    /// The header bar's height, logical pixels.
    pub header_height: f32,
    /// The header bar background, `2 * corner + 1` logical pixels wide with
    /// its top corners cut by the window's radius, `[focused, backdrop]`.
    pub header: [Bitmap; 2],
    /// The header's text colour, `[focused, backdrop]`.
    pub header_text: [Color; 2],
    /// The opacity GTK gives the header's contents in the background
    /// (libadwaita fades them).
    pub backdrop_opacity: f32,
    /// A title button's box, logical pixels.
    pub button_size: (f32, f32),
    /// Space between title buttons, logical pixels.
    pub button_spacing: f32,
    /// Space between the header's edge and the buttons, logical pixels.
    pub header_padding: Insets,
    pub buttons: Vec<ButtonImages>,
    /// The theme's accent fill and the text drawn on it, when it has one.
    pub accent: Option<(Color, Color)>,
    /// Which library drew it, for the log and the gallery.
    pub drawn_by: &'static str,
}

#[cfg(test)]
impl Rendered {
    /// A drawing with blank images of the real sizes, for tests that need one
    /// without GTK: the frame image `4 * MAX_FRAME` square at `key.scale`, a
    /// 12px corner, a 47px header.
    pub fn placeholder(key: FrameKey) -> Rendered {
        let scale = key.scale.max(1);
        let frame_px = MAX_FRAME * scale;
        let frame = Bitmap::new(4 * frame_px, 4 * frame_px);
        let header = Bitmap::new(25 * scale, 47 * scale);
        let button = Bitmap::new(34 * scale, 34 * scale);
        let buttons = [
            (WindowButton::Minimize, false),
            (WindowButton::Maximize, false),
            (WindowButton::Maximize, true),
            (WindowButton::Close, false),
        ]
        .into_iter()
        .map(|(button_kind, restore)| ButtonImages {
            button: button_kind,
            restore,
            images: std::array::from_fn(|_| [button.clone(), button.clone()]),
        })
        .collect();
        Rendered {
            key,
            scale,
            frame: [frame.clone(), frame],
            frame_px,
            thickness: Insets::uniform(40.0),
            corner_radius: 12.0,
            header_height: 47.0,
            header: [header.clone(), header],
            header_text: [Color::rgb(0); 2],
            backdrop_opacity: 1.0,
            button_size: (34.0, 34.0),
            button_spacing: 3.0,
            header_padding: Insets::uniform(6.0),
            buttons,
            accent: None,
            drawn_by: "test",
        }
    }
}

/// The loaded libraries. Not `Send`: it lives on the renderer thread.
pub struct Renderer {
    gtk: Gtk,
    adw: Option<Adw>,
}

fn color(rgba: GdkRgba) -> Color {
    Color {
        r: rgba.red,
        g: rgba.green,
        b: rgba.blue,
        a: rgba.alpha,
    }
}

fn insets(border: GtkBorder) -> Insets {
    Insets {
        top: border.top as f32,
        right: border.right as f32,
        bottom: border.bottom as f32,
        left: border.left as f32,
    }
}

/// One CSS node: a `GtkBox` named and classed like the node. The root owns
/// its descendants; dropping the root releases the tree.
struct Node<'a> {
    gtk: &'a Gtk,
    widget: Ptr,
    owned: bool,
    providers: Vec<Ptr>,
}

impl Drop for Node<'_> {
    fn drop(&mut self) {
        unsafe {
            for provider in &self.providers {
                (self.gtk.g_object_unref)(*provider);
            }
            if self.owned {
                (self.gtk.g_object_unref)(self.widget);
            }
        }
    }
}

impl Renderer {
    /// Opens GTK 4 (and libadwaita when installed) and connects GTK to the
    /// display. Call on the thread that will do every later call.
    pub fn open() -> Result<Renderer, super::FallbackReason> {
        use super::FallbackReason;
        let open = |name: &str| {
            let c = CString::new(name).expect("a library name has no NUL");
            let handle = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            (!handle.is_null()).then_some(handle)
        };
        let handle = open("libgtk-4.so.1")
            .ok_or_else(|| FallbackReason::NoGtk("libgtk-4.so.1 is not installed".into()))?;
        let gtk = unsafe { Gtk::load(handle) }.map_err(FallbackReason::NoGtk)?;
        unsafe {
            // GPUI owns the locale; GTK must not call setlocale(LC_ALL, "").
            (gtk.gtk_disable_setlocale)();
            if (gtk.gtk_init_check)() == 0 {
                return Err(FallbackReason::GtkInitFailed);
            }
            // This thread runs GLib's default main context from now on.
            (gtk.g_main_context_acquire)((gtk.g_main_context_default)());
        }
        let adw = open("libadwaita-1.so.0").and_then(|handle| unsafe {
            let sym = |name: &str| {
                let c = CString::new(name).expect("a symbol name has no NUL");
                let found = libc::dlsym(handle, c.as_ptr());
                (!found.is_null()).then_some(found)
            };
            let init = sym("adw_init")?;
            let get = sym("adw_style_manager_get_default")?;
            let set = sym("adw_style_manager_set_color_scheme")?;
            std::mem::transmute::<*mut c_void, unsafe extern "C" fn()>(init)();
            Some(Adw {
                style_manager_get_default: std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn() -> Ptr,
                >(get),
                style_manager_set_color_scheme: std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(Ptr, c_int),
                >(set),
            })
        });
        Ok(Renderer { gtk, adw })
    }

    /// Lets GTK read its display connection and settings changes.
    pub fn pump(&self) {
        unsafe { while (self.gtk.g_main_context_iteration)(std::ptr::null_mut(), 0) != 0 {} }
    }

    /// Sleeps until GLib's main context has something to do (GTK's display
    /// connection, a settings change, a timer) or `wake` becomes readable,
    /// then runs what is due. This is GLib's own loop with one more file
    /// descriptor in its poll, so an idle window costs no wakeups.
    pub fn wait(&self, wake: c_int) {
        let gtk = &self.gtk;
        unsafe {
            let context = (gtk.g_main_context_default)();
            let mut priority: c_int = 0;
            (gtk.g_main_context_prepare)(context, &mut priority);
            let mut timeout: c_int = -1;
            let mut fds = vec![GPollFd::default(); 16];
            loop {
                let needed = (gtk.g_main_context_query)(
                    context,
                    priority,
                    &mut timeout,
                    fds.as_mut_ptr(),
                    fds.len() as c_int,
                )
                .max(0) as usize;
                if needed <= fds.len() {
                    fds.truncate(needed);
                    break;
                }
                fds.resize(needed, GPollFd::default());
            }
            let mut polled: Vec<libc::pollfd> = fds
                .iter()
                .map(|f| libc::pollfd {
                    fd: f.fd,
                    events: f.events as i16,
                    revents: 0,
                })
                .collect();
            polled.push(libc::pollfd {
                fd: wake,
                events: libc::POLLIN,
                revents: 0,
            });
            libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, timeout);
            for (fd, result) in fds.iter_mut().zip(&polled) {
                fd.revents = result.revents as u16;
            }
            if (gtk.g_main_context_check)(context, priority, fds.as_mut_ptr(), fds.len() as c_int)
                != 0
            {
                (gtk.g_main_context_dispatch)(context);
            }
        }
    }

    fn set_string(&self, object: Ptr, property: &str, value: &str) {
        let (Ok(property), Ok(value)) = (CString::new(property), CString::new(value)) else {
            return;
        };
        unsafe {
            (self.gtk.g_object_set)(
                object,
                property.as_ptr(),
                value.as_ptr(),
                std::ptr::null::<c_char>(),
            )
        };
    }

    fn set_bool(&self, object: Ptr, property: &str, value: bool) {
        let Ok(property) = CString::new(property) else {
            return;
        };
        unsafe {
            (self.gtk.g_object_set)(
                object,
                property.as_ptr(),
                value as c_int,
                std::ptr::null::<c_char>(),
            )
        };
    }

    /// Points GTK at the theme, icon theme, and scheme in `key`.
    fn apply(&self, key: &FrameKey) {
        unsafe {
            let settings = (self.gtk.gtk_settings_get_default)();
            if settings.is_null() {
                return;
            }
            if let Some(icons) = &key.icon_theme {
                self.set_string(settings, "gtk-icon-theme-name", icons);
            }
            match &self.adw {
                // libadwaita draws its own stylesheet and follows the desktop
                // theme itself; it only needs to be told light or dark.
                Some(adw) => {
                    let manager = (adw.style_manager_get_default)();
                    if !manager.is_null() {
                        (adw.style_manager_set_color_scheme)(
                            manager,
                            if key.dark {
                                ADW_FORCE_DARK
                            } else {
                                ADW_FORCE_LIGHT
                            },
                        );
                    }
                }
                None => {
                    if let Some(theme) = &key.gtk_theme {
                        self.set_string(settings, "gtk-theme-name", theme);
                    }
                    self.set_bool(settings, "gtk-application-prefer-dark-theme", key.dark);
                }
            }
        }
        self.pump();
    }

    /// A node `selector`, written as Chromium writes them:
    /// `name.class.class:state`, under `parent`.
    fn node<'a>(&'a self, parent: Option<&Node<'a>>, selector: &str, scale: u32) -> Node<'a> {
        let gtk = &self.gtk;
        let (path, state) = match selector.split_once(':') {
            Some((path, states)) => (path, states),
            None => (selector, ""),
        };
        let mut parts = path.split('.');
        let name = parts.next().unwrap_or("box");
        let classes: Vec<CString> = parts
            .filter(|c| !c.is_empty())
            .filter_map(|c| CString::new(c).ok())
            .collect();
        let mut flags = STATE_NORMAL;
        for pseudo in state.split(':') {
            flags |= match pseudo {
                "hover" => STATE_PRELIGHT,
                "active" => STATE_ACTIVE,
                "backdrop" => STATE_BACKDROP,
                _ => 0,
            };
        }
        let css_name = CString::new(name).unwrap_or_else(|_| c"box".to_owned());
        unsafe {
            let widget = (gtk.g_object_new)(
                (gtk.gtk_box_get_type)(),
                c"css-name".as_ptr(),
                css_name.as_ptr(),
                std::ptr::null::<c_char>(),
            );
            let mut list: Vec<*const c_char> = classes.iter().map(|c| c.as_ptr()).collect();
            list.push(std::ptr::null());
            (gtk.gtk_widget_set_css_classes)(widget, list.as_ptr());
            (gtk.gtk_widget_set_state_flags)(widget, flags, 0);
            let owned = match parent {
                Some(parent) => {
                    (gtk.gtk_widget_set_parent)(widget, parent.widget);
                    false
                }
                None => {
                    (gtk.g_object_ref_sink)(widget);
                    true
                }
            };
            (gtk.gtk_style_context_set_scale)(
                (gtk.gtk_widget_get_style_context)(widget),
                scale as c_int,
            );
            Node {
                gtk,
                widget,
                owned,
                providers: Vec::new(),
            }
        }
    }

    fn context(&self, node: &Node) -> Ptr {
        unsafe { (self.gtk.gtk_widget_get_style_context)(node.widget) }
    }

    /// Adds `css` over the theme for `node` alone.
    fn add_css(&self, node: &mut Node, css: &str) {
        let Ok(text) = CString::new(css) else { return };
        unsafe {
            let provider = (self.gtk.gtk_css_provider_new)();
            (self.gtk.gtk_css_provider_load_from_data)(provider, text.as_ptr(), -1);
            (self.gtk.gtk_style_context_add_provider)(
                self.context(node),
                provider,
                PRIORITY_USER + 1,
            );
            node.providers.push(provider);
        }
    }

    fn padding(&self, node: &Node) -> Insets {
        let mut b = GtkBorder::default();
        unsafe { (self.gtk.gtk_style_context_get_padding)(self.context(node), &mut b) };
        insets(b)
    }

    fn border(&self, node: &Node) -> Insets {
        let mut b = GtkBorder::default();
        unsafe { (self.gtk.gtk_style_context_get_border)(self.context(node), &mut b) };
        insets(b)
    }

    fn margin(&self, node: &Node) -> Insets {
        let mut b = GtkBorder::default();
        unsafe { (self.gtk.gtk_style_context_get_margin)(self.context(node), &mut b) };
        insets(b)
    }

    fn color(&self, node: &Node) -> Color {
        let mut c = GdkRgba::default();
        unsafe { (self.gtk.gtk_style_context_get_color)(self.context(node), &mut c) };
        color(c)
    }

    fn lookup(&self, node: &Node, name: &CStr) -> Option<Color> {
        let mut c = GdkRgba::default();
        let found = unsafe {
            (self.gtk.gtk_style_context_lookup_color)(self.context(node), name.as_ptr(), &mut c)
        };
        (found != 0).then(|| color(c))
    }

    /// The node's minimum border box: GTK 4 cannot report `min-width` or
    /// `min-height`, so the widget is measured, as Chromium does.
    fn min_size(&self, node: &Node) -> (f32, f32) {
        let mut w = 0;
        let mut h = 0;
        unsafe {
            (self.gtk.gtk_widget_measure)(
                node.widget,
                ORIENTATION_HORIZONTAL,
                -1,
                &mut w,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (self.gtk.gtk_widget_measure)(
                node.widget,
                ORIENTATION_VERTICAL,
                -1,
                &mut h,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
        let m = self.margin(node);
        (
            (w as f32 - m.left - m.right).max(0.0),
            (h as f32 - m.top - m.bottom).max(0.0),
        )
    }

    /// Draws into a fresh `width` by `height` surface (device pixels) and
    /// returns the pixels.
    fn draw(&self, width: u32, height: u32, paint: impl FnOnce(Ptr)) -> Bitmap {
        let gtk = &self.gtk;
        unsafe {
            let surface = (gtk.cairo_image_surface_create)(
                CAIRO_FORMAT_ARGB32,
                width as c_int,
                height as c_int,
            );
            let cr = (gtk.cairo_create)(surface);
            paint(cr);
            (gtk.cairo_destroy)(cr);
            (gtk.cairo_surface_flush)(surface);
            let data = (gtk.cairo_image_surface_get_data)(surface);
            let stride = (gtk.cairo_image_surface_get_stride)(surface).max(0) as usize;
            let bitmap = if data.is_null() {
                Bitmap::new(width, height)
            } else {
                let bytes = std::slice::from_raw_parts(data, stride * height as usize);
                Bitmap::from_cairo_argb32(bytes, width, height, stride)
            };
            (gtk.cairo_surface_destroy)(surface);
            bitmap
        }
    }

    fn render_box(&self, node: &Node, cr: Ptr, x: f64, y: f64, w: f64, h: f64) {
        let ctx = self.context(node);
        unsafe {
            (self.gtk.gtk_render_background)(ctx, cr, x, y, w, h);
            (self.gtk.gtk_render_frame)(ctx, cr, x, y, w, h);
        }
    }

    fn window_selector(key: &FrameKey, focused: bool) -> String {
        let mut selector = String::from("window.background.csd");
        if key.tiled {
            selector.push_str(".tiled");
        }
        if key.maximized {
            selector.push_str(".maximized");
        }
        if !focused {
            selector.push_str(":backdrop");
        }
        selector
    }

    /// The frame around a `2 * MAX_FRAME` square window, with its shadow,
    /// border, and window background. Bottom corners are square, as in
    /// Chromium: the content is painted over the bottom edge and would
    /// otherwise cover their border.
    fn frame(&self, key: &FrameKey, focused: bool, scale: u32) -> Bitmap {
        let mut window = self.node(None, &Self::window_selector(key, focused), scale);
        self.add_css(
            &mut window,
            "* { border-bottom-left-radius: 0; border-bottom-right-radius: 0; }",
        );
        let pad = self.padding(&window);
        let border = self.border(&window);
        let f = MAX_FRAME as f64;
        let x = f - (pad.left + border.left) as f64;
        let y = f - (pad.top + border.top) as f64;
        let w = 2.0 * f + (pad.left + pad.right + border.left + border.right) as f64;
        let h = 2.0 * f + (pad.top + pad.bottom + border.top + border.bottom) as f64;
        let size = 4 * MAX_FRAME * scale;
        self.draw(size, size, |cr| unsafe {
            (self.gtk.cairo_scale)(cr, scale as f64, scale as f64);
            self.render_box(&window, cr, x, y, w, h);
        })
    }

    /// The top corner radius, measured from an opaque sample as Chromium
    /// does (`ComputeTopCornerRadius`): GTK 4 does not report CSS values.
    fn corner_radius(&self, key: &FrameKey) -> u32 {
        let mut window = self.node(None, &Self::window_selector(key, false), 1);
        self.add_css(
            &mut window,
            "window { background-image: none; background-color: black; box-shadow: none; \
             border: none; outline: none; border-bottom-left-radius: 0; \
             border-bottom-right-radius: 0; border-top-right-radius: 0; }",
        );
        let size = MAX_CORNER;
        let sample = self.draw(size, size, |cr| {
            self.render_box(&window, cr, 0.0, 0.0, size as f64, size as f64)
        });
        super::top_corner_radius(&sample)
    }

    fn header_selector(key: &FrameKey, focused: bool) -> (String, String) {
        let state = if focused { "" } else { ":backdrop" };
        (
            Self::window_selector(key, focused),
            // A plain header bar, as libadwaita applications show it in a
            // toolbar view: `window > headerbar.titlebar` is the older
            // GtkWindow title bar, a pixel shorter with its shade outside.
            format!("headerbar{state}"),
        )
    }

    /// The header bar's background and bottom shade, `width` logical pixels
    /// wide, clipped by the window's top corners.
    fn header(
        &self,
        key: &FrameKey,
        focused: bool,
        scale: u32,
        width: u32,
        height: u32,
        radius: f32,
    ) -> (Bitmap, Color) {
        let (window, header) = Self::header_selector(key, focused);
        let window = self.node(None, &window, scale);
        let header = self.node(Some(&window), &header, scale);
        let text = self.color(&header);
        let bitmap = self.draw(width * scale, height * scale, |cr| unsafe {
            let g = &self.gtk;
            (g.cairo_scale)(cr, scale as f64, scale as f64);
            if radius > 0.0 {
                let r = radius as f64;
                let w = width as f64;
                let h = height as f64;
                (g.cairo_new_path)(cr);
                (g.cairo_move_to)(cr, 0.0, h);
                (g.cairo_line_to)(cr, 0.0, r);
                (g.cairo_arc)(
                    cr,
                    r,
                    r,
                    r,
                    std::f64::consts::PI,
                    1.5 * std::f64::consts::PI,
                );
                (g.cairo_line_to)(cr, w - r, 0.0);
                (g.cairo_arc)(cr, w - r, r, r, 1.5 * std::f64::consts::PI, 0.0);
                (g.cairo_line_to)(cr, w, h);
                (g.cairo_close_path)(cr);
                (g.cairo_clip)(cr);
            }
            // The header's own side margins are -1px in Adwaita, so it is
            // drawn 1px wider each side, as it sits in a window.
            let m = self.margin(&header);
            self.render_box(
                &header,
                cr,
                m.left as f64,
                0.0,
                width as f64 - (m.left + m.right) as f64,
                height as f64,
            );
        });
        (bitmap, text)
    }

    fn header_height(&self, key: &FrameKey) -> (f32, Insets) {
        let (window, header) = Self::header_selector(key, true);
        let window = self.node(None, &window, 1);
        let header = self.node(Some(&window), &header, 1);
        let handle = self.node(Some(&header), "windowhandle", 1);
        let inner = self.node(Some(&handle), "box", 1);
        let (_, min) = self.min_size(&header);
        let padding = self.padding(&inner);
        // The header's own margin (Adwaita's is -1px at each side) moves its
        // contents toward the window edge by as much.
        let margin = self.margin(&header);
        let header_padding = self.padding(&header);
        (
            min,
            Insets {
                top: padding.top + header_padding.top,
                right: padding.right + header_padding.right + margin.right,
                bottom: padding.bottom + header_padding.bottom,
                left: padding.left + header_padding.left + margin.left,
            },
        )
    }

    fn button_nodes<'a>(
        &'a self,
        key: &FrameKey,
        button: WindowButton,
        state: ButtonState,
        focused: bool,
        scale: u32,
    ) -> [Node<'a>; 5] {
        let (window, header) = Self::header_selector(key, focused);
        let window = self.node(None, &window, scale);
        let header = self.node(Some(&window), &header, scale);
        let backdrop = if focused { "" } else { ":backdrop" };
        let controls = self.node(
            Some(&header),
            &format!("windowcontrols.end{backdrop}"),
            scale,
        );
        let pseudo = match state {
            ButtonState::Normal => "",
            ButtonState::Hover => ":hover",
            ButtonState::Pressed => ":hover:active",
        };
        let selector = format!(
            "button.titlebutton.{}{pseudo}{backdrop}",
            button.css_class()
        );
        let node = self.node(Some(&controls), &selector, scale);
        let image = self.node(Some(&node), &format!("image{pseudo}{backdrop}"), scale);
        [window, header, controls, node, image]
    }

    /// One title button's box and spacing, from the theme.
    fn button_metrics(&self, key: &FrameKey) -> ((f32, f32), f32) {
        let [_w, _h, controls, button, image] =
            self.button_nodes(key, WindowButton::Close, ButtonState::Normal, true, 1);
        let image_pad = self.padding(&image);
        let image_border = self.border(&image);
        let (iw, ih) = self.min_size(&image);
        let iw = iw.max(
            ICON_SIZE as f32
                + image_pad.left
                + image_pad.right
                + image_border.left
                + image_border.right,
        );
        let ih = ih.max(
            ICON_SIZE as f32
                + image_pad.top
                + image_pad.bottom
                + image_border.top
                + image_border.bottom,
        );
        let (bw, bh) = self.min_size(&button);
        let width = bw.max(iw);
        let height = bh.max(ih);
        // A second button: the controls box grows by the button and the
        // theme's spacing.
        let one = self.min_size(&controls).0;
        let _second = self.node(Some(&controls), "button.titlebutton.minimize", 1);
        let two = self.min_size(&controls).0;
        let spacing = (two - one - width).max(0.0);
        ((width, height), spacing)
    }

    /// A title button in one state, as Chromium's `NavButtonImageSource`
    /// draws it: the button's background and frame, then (GTK 4 themes style
    /// the image child) the image's, then the symbolic icon in the image's
    /// colour.
    fn button(
        &self,
        key: &FrameKey,
        (button, restore): (WindowButton, bool),
        (state, focused): (ButtonState, bool),
        scale: u32,
        size: (f32, f32),
    ) -> Bitmap {
        let [_w, _h, _c, node, image] = self.button_nodes(key, button, state, focused, scale);
        let (bw, bh) = size;
        let pad = self.padding(&image);
        let border = self.border(&image);
        let (iw, ih) = self.min_size(&image);
        // GTK gives the image the button's whole content box, so a theme
        // that styles the image (libadwaita's round fill) draws it that size.
        let button_pad = self.padding(&node);
        let button_border = self.border(&node);
        let content_w =
            bw - button_pad.left - button_pad.right - button_border.left - button_border.right;
        let content_h =
            bh - button_pad.top - button_pad.bottom - button_border.top - button_border.bottom;
        let img_w = iw
            .max(ICON_SIZE as f32 + pad.left + pad.right + border.left + border.right)
            .max(content_w);
        let img_h = ih
            .max(ICON_SIZE as f32 + pad.top + pad.bottom + border.top + border.bottom)
            .max(content_h);
        let ix = ((bw - img_w) / 2.0) as f64;
        let iy = ((bh - img_h) / 2.0) as f64;
        let fg = self.color(&image);
        let icon_name = CString::new(button.icon_name(restore)).expect("icon names have no NUL");
        let gtk = &self.gtk;
        let width = (bw * scale as f32).round() as u32;
        let height = (bh * scale as f32).round() as u32;
        self.draw(width, height, |cr| unsafe {
            (gtk.cairo_save)(cr);
            (gtk.cairo_scale)(cr, scale as f64, scale as f64);
            self.render_box(&node, cr, 0.0, 0.0, bw as f64, bh as f64);
            self.render_box(&image, cr, ix, iy, img_w as f64, img_h as f64);
            (gtk.cairo_restore)(cr);
            let display = (gtk.gdk_display_get_default)();
            if display.is_null() {
                return;
            }
            let theme = (gtk.gtk_icon_theme_get_for_display)(display);
            let paintable = (gtk.gtk_icon_theme_lookup_icon)(
                theme,
                icon_name.as_ptr(),
                std::ptr::null(),
                ICON_SIZE as c_int,
                scale as c_int,
                0,
                0,
            );
            if paintable.is_null() {
                return;
            }
            let px = (ICON_SIZE * scale) as f64;
            let snapshot = (gtk.gtk_snapshot_new)();
            (gtk.gdk_paintable_snapshot)(paintable, snapshot, px, px);
            let rendered = (gtk.gtk_snapshot_free_to_node)(snapshot);
            (gtk.cairo_save)(cr);
            (gtk.cairo_translate)(
                cr,
                ((width as f64 - px) / 2.0).round(),
                ((height as f64 - px) / 2.0).round(),
            );
            // Draw the icon, then keep its shape in the image's colour, as
            // Chromium does for symbolic icons in GTK 4.
            (gtk.cairo_push_group)(cr);
            if !rendered.is_null() {
                (gtk.gsk_render_node_draw)(rendered, cr);
                (gtk.gsk_render_node_unref)(rendered);
            }
            (gtk.cairo_set_source_rgba)(cr, fg.r as f64, fg.g as f64, fg.b as f64, fg.a as f64);
            (gtk.cairo_set_operator)(cr, CAIRO_OPERATOR_IN);
            (gtk.cairo_paint)(cr);
            (gtk.cairo_pop_group_to_source)(cr);
            (gtk.cairo_set_operator)(cr, 2);
            (gtk.cairo_paint)(cr);
            (gtk.cairo_restore)(cr);
            (gtk.g_object_unref)(paintable);
        })
    }

    /// Draws every part for `key`.
    pub fn render(&self, key: &FrameKey) -> Rendered {
        self.apply(key);
        let scale = key.scale.max(1);
        let frame = [self.frame(key, true, scale), self.frame(key, false, scale)];
        let thickness = super::frame_thickness(&frame[0], MAX_FRAME * scale);
        let thickness = Insets {
            top: thickness.top / scale as f32,
            right: thickness.right / scale as f32,
            bottom: thickness.bottom / scale as f32,
            left: thickness.left / scale as f32,
        };
        let radius = if key.maximized || key.tiled {
            0
        } else {
            self.corner_radius(key)
        };
        let (header_height, header_padding) = self.header_height(key);
        let header_height = header_height.max(1.0).round();
        let header_width = 2 * radius + 1;
        let (focused_header, focused_text) = self.header(
            key,
            true,
            scale,
            header_width,
            header_height as u32,
            radius as f32,
        );
        let (backdrop_header, backdrop_text) = self.header(
            key,
            false,
            scale,
            header_width,
            header_height as u32,
            radius as f32,
        );
        // libadwaita fades a background window's header contents to half
        // (`headerbar:backdrop > windowhandle { filter: opacity(0.5) }`), a
        // filter GTK cannot report. Themes that recolour the backdrop header
        // instead (Zorin's sets `filter: none` and its own backdrop text
        // colour) are told apart by that colour.
        let backdrop_opacity = if self.adw.is_some() && focused_text == backdrop_text {
            0.5
        } else {
            1.0
        };
        let (button_size, button_spacing) = self.button_metrics(key);
        let mut buttons = Vec::new();
        for (button, restore) in [
            (WindowButton::Minimize, false),
            (WindowButton::Maximize, false),
            (WindowButton::Maximize, true),
            (WindowButton::Close, false),
        ] {
            let images = ButtonState::ALL.map(|state| {
                [true, false].map(|focused| {
                    self.button(key, (button, restore), (state, focused), scale, button_size)
                })
            });
            buttons.push(ButtonImages {
                button,
                restore,
                images,
            });
        }
        let accent = {
            let window = self.node(None, "window.background", 1);
            match (
                self.lookup(&window, c"accent_bg_color"),
                self.lookup(&window, c"accent_fg_color"),
            ) {
                (Some(bg), Some(fg)) => Some((bg, fg)),
                _ => None,
            }
        };
        Rendered {
            key: key.clone(),
            scale,
            frame,
            frame_px: MAX_FRAME * scale,
            thickness,
            corner_radius: radius as f32,
            header_height,
            header: [focused_header, backdrop_header],
            header_text: [focused_text, backdrop_text],
            backdrop_opacity,
            button_size,
            button_spacing,
            header_padding,
            buttons,
            accent,
            drawn_by: if self.adw.is_some() {
                "libadwaita"
            } else {
                "GTK 4"
            },
        }
    }

    /// GTK's minor version, for the log.
    pub fn gtk_minor(&self) -> u32 {
        unsafe { (self.gtk.gtk_get_minor_version)() }
    }
}
