//! `better-files-preview <file>`: the large preview, in a process of its
//! own, as folio's `PreviewWindow.qml` is. A file that makes a decoder or a
//! converter misbehave takes this window down, never the file manager.
//!
//! Left and Right (or the arrows' Up and Down outside a PDF) step through
//! the file's folder; Space, Escape or Q close; `+` / `-` / `0` zoom an
//! image; in a PDF, Page Up / Page Down turn pages and Ctrl+F searches.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use better_ui::fluent::{self, BODY, BODY_STRONG, CAPTION};
use files_gui::i18n::{Copy, Locale, copy};
use files_preview::media::{self, AudioOut, MediaEvent, Player};
use files_preview::viewer::{self, ArchiveEntry, Language, Token, ViewKind, highlight, view_kind};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use smol::Timer;

/// Text past this is not shown: a preview, not an editor.
const TEXT_LIMIT: usize = 512 * 1024;

/// One line of code and its highlighted stretches.
type CodeLine = (SharedString, Vec<(std::ops::Range<usize>, Token)>);

enum Content {
    Loading,
    Image(PathBuf),
    Markdown(SharedString),
    Code {
        lines: Vec<CodeLine>,
        truncated: bool,
    },
    Pdf {
        file: PathBuf,
        pages: u32,
        page: u32,
        image: Option<PathBuf>,
        text: Option<Vec<String>>,
        hits: Vec<u32>,
    },
    Archive {
        entries: Vec<ArchiveEntry>,
        more: bool,
    },
    Media(Box<Media>),
    Folder {
        names: Vec<String>,
        total: usize,
    },
    Message(String),
}

/// An audio or video file, playing in this process through GStreamer.
struct Media {
    video: bool,
    /// The thumbnail cache's picture, shown for audio (an album cover made
    /// by a thumbnailer) and for video until its first frame arrives.
    poster: Option<PathBuf>,
    player: Option<Player>,
    /// The newest decoded frame, as the image GPUI draws.
    frame: Option<Arc<RenderImage>>,
    /// What to tell the person: a missing codec or why it cannot play.
    note: Option<String>,
}

/// How far Shift+Left and Shift+Right move.
const SEEK_STEP: Duration = Duration::from_secs(5);

/// A private folder for rendered pages and converted documents, removed
/// when the window closes.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(std::env::temp_dir);
        let path = base.join(format!("better-files-preview-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&path);
        let _ = std::fs::set_permissions(
            &path,
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
        );
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct PreviewApp {
    files: Vec<PathBuf>,
    at: usize,
    content: Content,
    zoom: f32,
    focus: FocusHandle,
    search: Entity<InputState>,
    searching: bool,
    scratch: Scratch,
    loaded: Option<mpsc::Receiver<(usize, Content)>>,
    locale: Locale,
    _subscriptions: Vec<Subscription>,
    _poll: Option<Task<()>>,
    _media: Task<()>,
}

fn mime_of(path: &Path) -> Option<String> {
    let detector = files_platform::detector_from_env();
    detector
        .detect(&path.file_name()?.to_string_lossy())
        .map(|mime| mime.as_str().to_string())
}

/// Builds what one file shows. Runs on a worker thread: converting an office
/// document or rendering a PDF page can take seconds.
/// `max_frame`: the window's size in pixels, which a video's frames are
/// scaled down to fit.
fn load(path: &Path, scratch: &Path, c: &'static Copy, max_frame: (u32, u32)) -> Content {
    let mime = mime_of(path);
    match view_kind(path, mime.as_deref()) {
        ViewKind::Image => Content::Image(path.to_path_buf()),
        ViewKind::Markdown => match read_text(path) {
            Some((text, _)) => Content::Markdown(text.into()),
            None => Content::Message(c.preview_unreadable.into()),
        },
        ViewKind::Code(language) => code(path, Some(language), c),
        ViewKind::Text => code(path, None, c),
        ViewKind::Pdf => pdf(path, scratch, c),
        ViewKind::Office => {
            if viewer::run("soffice", &["--version".into()], Duration::from_secs(20)).is_none() {
                return Content::Message(c.preview_needs_libreoffice.into());
            }
            match viewer::office_to_pdf(path, scratch) {
                Some(converted) => pdf(&converted, scratch, c),
                None => Content::Message(c.preview_convert_failed.into()),
            }
        }
        ViewKind::Archive(kind) => match viewer::archive_entries(path, kind) {
            Ok((entries, more)) => Content::Archive { entries, more },
            Err(_) => Content::Message(c.preview_decode_failed.into()),
        },
        kind @ (ViewKind::Audio | ViewKind::Video) => {
            let (player, note) = match Player::open(path, AudioOut::Speakers, max_frame) {
                Ok(mut player) => {
                    player.play();
                    (Some(player), None)
                }
                Err(error) => (
                    None,
                    Some(c.media_failed.replace("{reason}", &error.to_string())),
                ),
            };
            Content::Media(Box::new(Media {
                video: kind == ViewKind::Video,
                poster: files_preview::thumbnails::ThumbnailCache::from_env().and_then(|cache| {
                    match cache.lookup(path, files_preview::thumbnails::ThumbSize::Large) {
                        files_preview::thumbnails::Lookup::Found(poster) => Some(poster),
                        _ => None,
                    }
                }),
                player,
                frame: None,
                note,
            }))
        }
        ViewKind::Folder => {
            let mut names: Vec<String> = std::fs::read_dir(path)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .filter(|name| !name.starts_with('.'))
                        .collect()
                })
                .unwrap_or_default();
            names.sort_by_key(|name| name.to_lowercase());
            let total = names.len();
            names.truncate(500);
            Content::Folder { names, total }
        }
        ViewKind::Other => match read_text(path) {
            Some(_) => code(path, None, c),
            None => Content::Message(c.preview_not_previewable.into()),
        },
    }
}

/// UTF-8 text, at most [`TEXT_LIMIT`], and whether it was cut. `None` for
/// something that is not text.
fn read_text(path: &Path) -> Option<(String, bool)> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(TEXT_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let truncated = bytes.len() > TEXT_LIMIT;
    bytes.truncate(TEXT_LIMIT);
    if bytes.contains(&0) {
        return None;
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // Cut in the middle of a character: drop the partial one.
        Err(error) if truncated => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).ok()?
        }
        Err(_) => return None,
    };
    Some((text, truncated))
}

fn code(path: &Path, language: Option<Language>, c: &'static Copy) -> Content {
    let Some((text, truncated)) = read_text(path) else {
        return Content::Message(c.preview_binary.into());
    };
    let marks = language
        .map(|language| highlight(language, &text))
        .unwrap_or_default();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut mark = 0;
    for line in text.split('\n') {
        let end = start + line.len();
        let mut own = Vec::new();
        while mark < marks.len() && marks[mark].0.start < end + 1 {
            let (range, token) = &marks[mark];
            let from = range.start.max(start);
            let to = range.end.min(end);
            if from < to {
                own.push((from - start..to - start, *token));
            }
            if range.end <= end + 1 {
                mark += 1;
            } else {
                break;
            }
        }
        lines.push((SharedString::from(line.replace('\t', "    ")), own));
        start = end + 1;
    }
    // Tabs were widened, so the ranges of a line with tabs no longer fit;
    // such a line is drawn plain rather than wrongly.
    for (line, own) in &mut lines {
        if own.iter().any(|(range, _)| range.end > line.len()) {
            own.clear();
        }
    }
    Content::Code { lines, truncated }
}

fn pdf(path: &Path, scratch: &Path, c: &'static Copy) -> Content {
    let Some(pages) = viewer::pdf_page_count(path) else {
        return Content::Message(c.preview_needs_poppler.into());
    };
    Content::Pdf {
        file: path.to_path_buf(),
        pages,
        page: 1,
        image: viewer::render_pdf_page(path, 1, 1000, scratch),
        text: viewer::pdf_text(path),
        hits: Vec::new(),
    }
}

impl PreviewApp {
    fn new(file: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let locale = Locale::System;
        let (files, at) = viewer::siblings(&file);
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(copy(locale).preview_search_pdf));
        let subscriptions = vec![cx.subscribe_in(
            &search,
            window,
            |app, state, event: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let query = state.read(cx).value().to_string();
                    app.find_in_pdf(&query, cx);
                }
            },
        )];
        // The same appearance as Better Files, kept in step with the desktop
        // while it is `system` (ticket 110).
        let appearance = better_ui::fluent::theme::Appearance::default_path("files")
            .map(|path| better_ui::fluent::theme::Appearance::load(&path))
            .unwrap_or_default();
        appearance.apply_and_follow(window, cx);
        let mut app = Self {
            files,
            at,
            content: Content::Loading,
            zoom: 1.0,
            focus: cx.focus_handle(),
            search,
            searching: false,
            scratch: Scratch::new(),
            loaded: None,
            locale,
            _subscriptions: subscriptions,
            _poll: None,
            _media: cx.spawn_in(window, async move |this, cx| {
                loop {
                    Timer::after(Duration::from_millis(16)).await;
                    if this
                        .update_in(cx, |app, window, cx| app.pump_media(window, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            }),
        };
        app.start_loading(window, cx);
        app
    }

    fn current(&self) -> &Path {
        &self.files[self.at]
    }

    fn start_loading(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.release_frame(window, cx);
        self.content = Content::Loading;
        self.zoom = 1.0;
        let (sender, receiver) = mpsc::channel();
        let file = self.current().to_path_buf();
        let scratch = self.scratch.path().to_path_buf();
        let viewport = window.viewport_size();
        let scale = window.scale_factor();
        let max_frame = (
            (f32::from(viewport.width) * scale).ceil() as u32,
            (f32::from(viewport.height) * scale).ceil() as u32,
        );
        let index = self.at;
        let c = copy(self.locale);
        std::thread::spawn(move || {
            let _ = sender.send((index, load(&file, &scratch, c, max_frame)));
        });
        self.loaded = Some(receiver);
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(30)).await;
                let Ok(done) = this.update(cx, |app, cx| {
                    let Some(receiver) = &app.loaded else {
                        return true;
                    };
                    match receiver.try_recv() {
                        Ok((index, content)) => {
                            // A step taken while this one loaded wins.
                            if index == app.at {
                                app.content = content;
                                app.loaded = None;
                                cx.notify();
                            }
                            true
                        }
                        Err(mpsc::TryRecvError::Empty) => false,
                        Err(mpsc::TryRecvError::Disconnected) => true,
                    }
                }) else {
                    break;
                };
                if done {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.files.len();
        if len < 2 {
            return;
        }
        self.at = if forward {
            (self.at + 1) % len
        } else {
            (self.at + len - 1) % len
        };
        window.set_window_title(&self.title());
        self.start_loading(window, cx);
    }

    fn title(&self) -> String {
        self.current()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Takes the player's news and its newest frame. Runs every 16 ms; does
    /// nothing unless a media file is open.
    fn pump_media(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = copy(self.locale);
        let Content::Media(media) = &mut self.content else {
            return;
        };
        let Some(player) = media.player.as_mut() else {
            return;
        };
        let mut changed = player.is_playing();
        let events = player.poll();
        let failed = events
            .iter()
            .any(|event| matches!(event, MediaEvent::Failed(_)));
        for event in events {
            changed = true;
            match event {
                MediaEvent::Finished => {}
                MediaEvent::MissingCodec(names) => {
                    let names: Vec<_> = names.into_iter().filter(|name| !name.is_empty()).collect();
                    media.note = Some(if names.is_empty() {
                        c.media_missing_codec_unnamed.to_string()
                    } else {
                        c.media_missing_codec.replace("{names}", &names.join(", "))
                    });
                }
                MediaEvent::Failed(reason) => {
                    // A missing codec already said the useful thing.
                    if media.note.is_none() {
                        media.note = Some(c.media_failed.replace("{reason}", &reason));
                    }
                }
            }
        }
        if failed {
            // Nothing left to play: the controls go, the message stays.
            media.player = None;
            cx.notify();
            return;
        }
        if let Some(frame) = player.take_frame()
            && let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
        {
            let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
            if let Some(old) = media.frame.replace(image) {
                // The window being drawn is not in the app's list while it
                // is updated, so it is named, or its texture stays behind.
                cx.drop_image(old, Some(window));
            }
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// Gives the shown video frame's texture back before the content goes.
    fn release_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Content::Media(media) = &mut self.content
            && let Some(old) = media.frame.take()
        {
            cx.drop_image(old, Some(window));
        }
    }

    fn with_player(&mut self, cx: &mut Context<Self>, act: impl FnOnce(&mut Player)) {
        if let Content::Media(media) = &mut self.content
            && let Some(player) = media.player.as_mut()
        {
            act(player);
            cx.notify();
        }
    }

    fn turn_page(&mut self, to: u32, cx: &mut Context<Self>) {
        if let Content::Pdf {
            file,
            pages,
            page,
            image,
            ..
        } = &mut self.content
        {
            let to = to.clamp(1, *pages);
            if to != *page {
                *page = to;
                *image = viewer::render_pdf_page(file, to, 1000, self.scratch.path());
                cx.notify();
            }
        }
    }

    fn find_in_pdf(&mut self, query: &str, cx: &mut Context<Self>) {
        let next = if let Content::Pdf {
            text: Some(text),
            hits,
            page,
            ..
        } = &mut self.content
        {
            *hits = viewer::search_pages(text, query);
            hits.iter()
                .copied()
                .find(|hit| *hit > *page)
                .or(hits.first().copied())
        } else {
            None
        };
        if let Some(next) = next {
            self.turn_page(next, cx);
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let control = event.keystroke.modifiers.control;
        if self.searching {
            if key == "escape" {
                self.searching = false;
                window.focus(&self.focus, cx);
                cx.notify();
            }
            return;
        }
        let page = match &self.content {
            Content::Pdf { page, .. } => Some(*page),
            _ => None,
        };
        if matches!(self.content, Content::Media(_)) {
            let shift = event.keystroke.modifiers.shift;
            match key {
                "enter" => return self.with_player(cx, Player::toggle),
                "left" | "right" if shift => {
                    let forward = key == "right";
                    return self.with_player(cx, |player| {
                        let at = player.position().unwrap_or_default();
                        player.seek(if forward {
                            at + SEEK_STEP
                        } else {
                            at.saturating_sub(SEEK_STEP)
                        });
                    });
                }
                _ => {}
            }
        }
        match (key, page) {
            ("space" | "escape" | "q", _) if !control => cx.quit(),
            ("right", _) => self.step(true, window, cx),
            ("left", _) => self.step(false, window, cx),
            ("down", None) => self.step(true, window, cx),
            ("up", None) => self.step(false, window, cx),
            ("pagedown" | "down", Some(page)) => self.turn_page(page + 1, cx),
            ("pageup" | "up", Some(page)) => self.turn_page(page.saturating_sub(1), cx),
            ("f", Some(_)) if control => {
                self.searching = true;
                self.search.update(cx, |state, cx| state.focus(window, cx));
                cx.notify();
            }
            ("+" | "=", _) => {
                self.zoom = (self.zoom * 1.25).min(8.0);
                cx.notify();
            }
            ("-", _) => {
                self.zoom = (self.zoom / 1.25).max(0.125);
                cx.notify();
            }
            ("0", _) => {
                self.zoom = 1.0;
                cx.notify();
            }
            _ => {}
        }
    }

    fn body(&self, window: &Window, this: WeakEntity<Self>, cx: &App) -> AnyElement {
        let c = copy(self.locale);
        let p = fluent::palette(cx);
        let message = |text: String| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(fluent::label(text, BODY, p.text_secondary))
                .into_any_element()
        };
        let viewport = window.viewport_size();
        match &self.content {
            Content::Loading => message(c.preview_loading.into()),
            Content::Message(text) => message(text.clone()),
            Content::Image(path) => {
                let width = f32::from(viewport.width) * self.zoom;
                let height = (f32::from(viewport.height) - 48.0) * self.zoom;
                div()
                    .id("image")
                    .size_full()
                    .overflow_scroll()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(path.clone())
                            .w(px(width))
                            .h(px(height))
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element()
            }
            Content::Markdown(source) => div()
                .id("markdown")
                .size_full()
                .overflow_y_scroll()
                .p(px(32.0))
                .child(gpui_component::text::markdown(source.clone()))
                .into_any_element(),
            Content::Code { lines, truncated } => {
                let lines = lines.clone();
                let palette = *p;
                let mono = gpui_component::ActiveTheme::theme(cx)
                    .mono_font_family
                    .clone();
                let count = lines.len();
                let truncated = *truncated;
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        uniform_list("code", count, move |range, _window, _cx| {
                            range
                                .map(|index| {
                                    let (text, marks) = &lines[index];
                                    let highlights = marks
                                        .iter()
                                        .map(|(range, token)| {
                                            let color = match token {
                                                Token::Keyword => palette.accent_text,
                                                Token::String => palette.success,
                                                Token::Comment => palette.text_tertiary,
                                                Token::Number => palette.caution,
                                            };
                                            (
                                                range.clone(),
                                                HighlightStyle {
                                                    color: Some(color.into()),
                                                    ..Default::default()
                                                },
                                            )
                                        })
                                        .collect::<Vec<_>>();
                                    div()
                                        .flex()
                                        .gap(px(16.0))
                                        .px(px(16.0))
                                        .font_family(mono.clone())
                                        .text_size(px(13.0))
                                        .child(
                                            div()
                                                .w(px(48.0))
                                                .flex_none()
                                                .text_color(palette.text_tertiary)
                                                .child(format!("{}", index + 1)),
                                        )
                                        .child(
                                            div()
                                                .whitespace_nowrap()
                                                .text_color(palette.text_primary)
                                                .child(
                                                    StyledText::new(text.clone())
                                                        .with_highlights(highlights),
                                                ),
                                        )
                                })
                                .collect()
                        })
                        .flex_1(),
                    )
                    .when(truncated, |column| {
                        column.child(div().p(px(8.0)).child(fluent::label(
                            c.preview_truncated,
                            CAPTION,
                            p.text_secondary,
                        )))
                    })
                    .into_any_element()
            }
            Content::Pdf {
                pages,
                page,
                image,
                text,
                hits,
                ..
            } => {
                let status = if hits.is_empty() {
                    c.preview_page_of
                        .replace("{page}", &page.to_string())
                        .replace("{pages}", &pages.to_string())
                } else {
                    format!(
                        "{} · {}",
                        c.preview_page_of
                            .replace("{page}", &page.to_string())
                            .replace("{pages}", &pages.to_string()),
                        c.preview_found_on.replace(
                            "{pages}",
                            &hits
                                .iter()
                                .map(u32::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    )
                };
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .px(px(16.0))
                            .py(px(8.0))
                            .child(fluent::label(status, CAPTION, p.text_secondary))
                            .child(div().flex_1())
                            .when(text.is_some(), |bar| {
                                bar.child(div().w(px(260.0)).child(
                                    fluent::text_box::TextBox::search("pdf-search", &self.search),
                                ))
                            }),
                    )
                    .child(
                        div()
                            .id("pdf-page")
                            .flex_1()
                            .min_h_0()
                            .overflow_scroll()
                            .flex()
                            .justify_center()
                            .children(image.clone().map(|page| {
                                img(page)
                                    .w(px(f32::from(viewport.width).min(1000.0) * self.zoom))
                                    .h(px((f32::from(viewport.height) - 96.0) * self.zoom))
                                    .object_fit(ObjectFit::Contain)
                            })),
                    )
                    .into_any_element()
            }
            Content::Archive { entries, more } => {
                let entries = entries.clone();
                let palette = *p;
                let more = *more;
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        uniform_list("archive", entries.len(), move |range, _window, _cx| {
                            range
                                .map(|index| {
                                    let entry = &entries[index];
                                    div()
                                        .w_full()
                                        .flex()
                                        .gap(px(16.0))
                                        .px(px(16.0))
                                        .h(px(28.0))
                                        .items_center()
                                        .child(
                                            fluent::label(
                                                entry.name.clone(),
                                                BODY,
                                                palette.text_primary,
                                            )
                                            .flex_1()
                                            .truncate(),
                                        )
                                        .child(fluent::label(
                                            if entry.is_folder {
                                                String::new()
                                            } else {
                                                files_gui::format::bytes(entry.size)
                                            },
                                            CAPTION,
                                            palette.text_secondary,
                                        ))
                                })
                                .collect()
                        })
                        .flex_1(),
                    )
                    .when(more, |column| {
                        column.child(div().p(px(8.0)).child(fluent::label(
                            c.preview_archive_more,
                            CAPTION,
                            p.text_secondary,
                        )))
                    })
                    .into_any_element()
            }
            Content::Media(media) => self.media_body(media, window, this, cx),
            Content::Folder { names, total } => div()
                .id("folder")
                .size_full()
                .overflow_y_scroll()
                .p(px(24.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(fluent::label(
                    files_gui::i18n::counted(*total, c.item_one, c.item_count),
                    BODY_STRONG,
                    p.text_primary,
                ))
                .children(
                    names
                        .iter()
                        .map(|name| fluent::label(name.clone(), BODY, p.text_primary)),
                )
                .into_any_element(),
        }
    }
}

impl PreviewApp {
    fn media_body(
        &self,
        media: &Media,
        window: &Window,
        this: WeakEntity<Self>,
        cx: &App,
    ) -> AnyElement {
        let c = copy(self.locale);
        let p = fluent::palette(cx);
        let viewport = window.viewport_size();
        let picture: Option<AnyElement> = match (&media.frame, &media.poster) {
            (Some(frame), _) if media.video => Some(
                img(ImageSource::Render(frame.clone()))
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
            ),
            (_, Some(poster)) => Some(
                img(poster.clone())
                    .w(px(480.0))
                    .h(px(320.0))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
            ),
            _ => None,
        };
        let mut column = div().size_full().flex().flex_col().child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(16.0))
                .bg(if media.video {
                    gpui::black()
                } else {
                    p.window_background.into()
                })
                .children(picture)
                .when(!media.video && media.poster.is_none(), |area| {
                    area.child(fluent::label(self.title(), BODY_STRONG, p.text_primary))
                }),
        );
        if let Some(note) = &media.note {
            column = column.child(div().px(px(16.0)).py(px(8.0)).child(fluent::label(
                note.clone(),
                BODY,
                p.text_secondary,
            )));
        }
        let Some(player) = &media.player else {
            return column.into_any_element();
        };
        let at = player.position().unwrap_or_default();
        let length = player.duration();
        let playing = player.is_playing();
        let toggle = {
            let this = this.clone();
            fluent::button::Button::icon_button(
                "media-toggle",
                if playing {
                    gpui_component::IconName::Pause
                } else {
                    gpui_component::IconName::Play
                },
                if playing { c.media_pause } else { c.media_play },
            )
            .on_click(move |_, cx| {
                let _ = this.update(cx, |app, cx| app.with_player(cx, Player::toggle));
            })
        };
        let seek_width = (f32::from(viewport.width) - 420.0).max(120.0);
        let seek = {
            let this = this.clone();
            let mut model = fluent::selection::SliderModel::new(
                0.0,
                length.map(|length| length.as_secs_f64()).unwrap_or(0.0),
            );
            model.step = 0.1;
            model.large_change = SEEK_STEP.as_secs_f64();
            fluent::selection::Slider::new("media-seek", at.as_secs_f64(), model)
                .width(px(seek_width))
                .disabled(length.is_none())
                .on_change(move |seconds, _, cx| {
                    let _ = this.update(cx, |app, cx| {
                        app.with_player(cx, |player| {
                            player.seek(Duration::from_secs_f64(seconds.max(0.0)))
                        })
                    });
                })
        };
        let volume = {
            let mut model = fluent::selection::SliderModel::new(0.0, 100.0);
            model.step = 5.0;
            fluent::selection::Slider::new("media-volume", (player.volume() * 100.0).round(), model)
                .width(px(120.0))
                .on_change(move |level, _, cx| {
                    let _ = this.update(cx, |app, cx| {
                        app.with_player(cx, |player| player.set_volume(level / 100.0))
                    });
                })
        };
        let time = match length {
            Some(length) => format!("{} / {}", media::clock(at), media::clock(length)),
            None => media::clock(at),
        };
        column
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .py(px(8.0))
                    .border_t_1()
                    .border_color(p.divider)
                    .child(toggle)
                    .child(fluent::label(time, CAPTION, p.text_secondary).w(px(96.0)))
                    .child(seek)
                    .child(div().flex_1())
                    .child(fluent::label(c.media_volume, CAPTION, p.text_secondary))
                    .child(volume),
            )
            .into_any_element()
    }
}

impl Render for PreviewApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = fluent::palette(cx);
        let c = copy(self.locale);
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        let position = format!("{} / {}", self.at + 1, self.files.len());
        let keys = match &self.content {
            Content::Media(media) if media.player.is_some() => {
                format!("{} · {}", c.preview_keys, c.media_keys)
            }
            _ => c.preview_keys.to_string(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            // The window frame paints the window background, with the
            // desktop's top corners (ticket 108).
            .text_color(p.text_primary)
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|app, event: &KeyDownEvent, window, cx| app.on_key(event, window, cx)),
            )
            .child(better_ui::window_chrome::title_bar(
                gpui_component::Icon::new(gpui_component::IconName::Eye).size(px(16.0)),
                format!("{} — {}", self.title(), c.preview),
                p.text_primary.into(),
            ))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.body(window, cx.weak_entity(), cx)),
            )
            .child(
                div()
                    .h(px(28.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(p.divider)
                    .child(fluent::label(
                        format!("{position} · {keys}"),
                        CAPTION,
                        p.text_secondary,
                    )),
            )
    }
}

fn main() {
    let Some(file) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: better-files-preview <file>");
        std::process::exit(2);
    };
    let file = std::fs::canonicalize(&file).unwrap_or(file);
    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);
    app.run(move |cx| {
        gpui_component::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1_100.0), px(780.0)), cx)),
            ..better_ui::window_chrome::window_options("io.betteros.Files.Preview")
        };
        cx.spawn(async move |cx| {
            let _ = cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| PreviewApp::new(file, window, cx));
                cx.new(|cx| better_ui::window_chrome::root(view, window, cx))
            });
        })
        .detach();
    });
}
