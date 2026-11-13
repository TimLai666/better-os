//! The operation center panel and the modal dialogs.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{ActiveTheme, *};

use crate::app::FilesApp;
use crate::i18n::{copy, resolution_label};
use crate::opcenter::{ConflictPrompt, JobRow};
use crate::session::PendingDialog;
use files_operations::Resolution;

impl FilesApp {
    /// The panel listing every job the shared engine knows about, in the
    /// Fluent kit: a card per job with a `ProgressBar` and standard buttons.
    pub(crate) fn operation_center(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use better_ui::fluent::{self, BODY, BODY_STRONG, CAPTION, button::Button as KitButton};
        use gpui_component::IconName;
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let rows = self.session.job_rows();
        let finished: Vec<JobRow> = self.session.finished.rows().into_iter().cloned().collect();
        let this = cx.entity().downgrade();

        v_flex()
            .id("operation-center")
            .w(px(420.0))
            .flex_shrink_0()
            .h_full()
            .gap(px(8.0))
            .p(px(12.0))
            .border_l_1()
            .border_color(p.divider)
            .bg(p.window_background)
            .overflow_y_scroll()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .child(fluent::label(c.jobs_panel, BODY_STRONG, p.text_primary).flex_1())
                    .child(
                        KitButton::icon_button("operations-close", IconName::Close, c.close)
                            .on_click(move |_window, cx| {
                                let _ = this.update(cx, |app, cx| {
                                    app.session.operations_open = false;
                                    cx.notify();
                                });
                            }),
                    ),
            )
            .when(rows.is_empty() && finished.is_empty(), |panel| {
                panel.child(fluent::label(c.no_operations, BODY, p.text_secondary))
            })
            .children(rows.into_iter().map(|row| self.job_card(row, cx)))
            .when(!finished.is_empty(), |panel| {
                panel.child(div().pt(px(8.0)).child(fluent::label(
                    c.completed_this_session,
                    CAPTION,
                    p.text_secondary,
                )))
            })
            .children(finished.into_iter().map(|row| self.finished_card(row, cx)))
            .into_any_element()
    }

    fn job_card(&self, row: JobRow, cx: &mut Context<Self>) -> AnyElement {
        use better_ui::fluent::{
            self, BODY_STRONG, CAPTION,
            button::Button as KitButton,
            progress::{Progress, ProgressBar, ProgressStatus},
        };
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let id = row.id;
        // An indeterminate job draws the moving segment rather than a bar at
        // zero: "no total yet" and "nothing done" mean different things.
        let progress = match row.fraction {
            Some(fraction) => Progress::Determinate(fraction.clamp(0.0, 1.0) as f32),
            None => Progress::Indeterminate,
        };
        let status = if !row.failures.is_empty() {
            ProgressStatus::Error
        } else if row.controls.resume {
            ProgressStatus::Paused
        } else {
            ProgressStatus::Running
        };
        let action =
            |key: &'static str,
             label: &'static str,
             accent: bool,
             run: fn(&mut crate::session::FilesSession, files_operations::JobId)| {
                let this = cx.entity().downgrade();
                let button_id = SharedString::from(format!("{key}-{}", id.value()));
                let button = if accent {
                    KitButton::accent(button_id, label)
                } else {
                    KitButton::standard(button_id, label)
                };
                button.on_click(move |_window, cx| {
                    let _ = this.update(cx, |app, cx| {
                        run(&mut app.session, id);
                        cx.notify();
                    });
                })
            };

        v_flex()
            .w_full()
            .min_w_0()
            .gap(px(6.0))
            .p(px(12.0))
            .rounded(fluent::radius::MD)
            .bg(p.card)
            .border_1()
            .border_color(p.card_stroke)
            .child(
                h_flex()
                    .w_full()
                    .child(fluent::label(row.title.clone(), BODY_STRONG, p.text_primary).flex_1())
                    .child(fluent::label(
                        row.state_label.clone(),
                        CAPTION,
                        p.text_secondary,
                    )),
            )
            .child(
                ProgressBar::new(
                    SharedString::from(format!("progress-{}", id.value())),
                    progress,
                )
                .status(status),
            )
            .child(fluent::label(
                format!(
                    "{} · {} · {} {} · {} {}",
                    row.items_label,
                    row.bytes_label,
                    c.throughput,
                    row.throughput_label,
                    c.remaining,
                    row.remaining_label
                ),
                CAPTION,
                p.text_secondary,
            ))
            .when_some(row.current.clone(), |element, current| {
                element.child(
                    fluent::label(
                        current.to_string_lossy().into_owned(),
                        CAPTION,
                        p.text_secondary,
                    )
                    .truncate(),
                )
            })
            .child(
                h_flex()
                    .gap(px(8.0))
                    .flex_wrap()
                    .when(row.controls.pause, |bar| {
                        bar.child(action("pause", c.pause, false, |session, id| {
                            session.pause_job(id)
                        }))
                    })
                    .when(row.controls.resume, |bar| {
                        bar.child(action("resume", c.resume, false, |session, id| {
                            session.resume_job(id)
                        }))
                    })
                    .when(row.controls.cancel, |bar| {
                        bar.child(action("cancel", c.cancel, false, |session, id| {
                            session.cancel_job(id)
                        }))
                    })
                    .when(row.controls.retry, |bar| {
                        bar.child(action("retry", c.retry_failed, true, |session, id| {
                            session.retry_job(id)
                        }))
                    }),
            )
            .when(!row.failures.is_empty(), |element| {
                element.child(
                    v_flex()
                        .gap(px(2.0))
                        .child(fluent::label(
                            format!("{} · {}", c.failures, row.failures.len()),
                            CAPTION,
                            p.critical,
                        ))
                        .children(row.failures.iter().take(5).map(|failure| {
                            fluent::label(
                                format!("{} — {}", failure.path.to_string_lossy(), failure.reason),
                                CAPTION,
                                p.text_secondary,
                            )
                            .truncate()
                        })),
                )
            })
            .when_some(row.conflict.clone(), |element, conflict| {
                element.child(self.conflict_card(conflict, cx))
            })
            .into_any_element()
    }

    fn finished_card(&self, row: JobRow, cx: &mut Context<Self>) -> AnyElement {
        use better_ui::fluent::{self, CAPTION};
        let p = fluent::palette(cx);
        h_flex()
            .w_full()
            .px(px(8.0))
            .py(px(4.0))
            .child(fluent::label(row.title.clone(), CAPTION, p.text_secondary).flex_1())
            .child(fluent::label(
                format!("{} · {}", row.items_label, row.state_label),
                CAPTION,
                p.text_secondary,
            ))
            .into_any_element()
    }

    /// The conflict prompt: the choices, and the switch that applies the next
    /// answer to every remaining conflict of the same kind.
    fn conflict_card(&self, conflict: ConflictPrompt, cx: &mut Context<Self>) -> AnyElement {
        use better_ui::fluent::{self, BODY, BODY_STRONG, CAPTION, button::Button as KitButton};
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let id = conflict.job;
        let apply = self.apply_to_remaining;
        // What the answers do to the one already there, in the words folio's
        // clash dialog uses: nothing is deleted.
        let note = if conflict.choices.contains(&Resolution::Merge) {
            Some(c.conflict_merge_note)
        } else if conflict.choices.contains(&Resolution::Overwrite) {
            Some(c.conflict_replace_note)
        } else {
            None
        };
        let this = cx.entity().downgrade();
        let toggle = this.clone();
        v_flex()
            .w_full()
            .gap(px(8.0))
            .mt(px(8.0))
            .p(px(12.0))
            .rounded(fluent::radius::MD)
            .bg(p.caution_background)
            .border_1()
            .border_color(p.stroke)
            .child(fluent::label(
                c.conflict_needs_a_decision,
                BODY_STRONG,
                p.text_primary,
            ))
            .child(fluent::label(conflict.title.clone(), BODY, p.text_primary))
            .child(
                fluent::label(
                    conflict.destination.to_string_lossy().into_owned(),
                    CAPTION,
                    p.text_secondary,
                )
                .truncate(),
            )
            .children(note.map(|note| fluent::label(note, CAPTION, p.text_secondary)))
            .child(
                KitButton::toggle(
                    SharedString::from(format!("apply-remaining-{}", id.value())),
                    c.apply_to_remaining,
                    apply,
                )
                .on_click(move |_window, cx| {
                    let _ = toggle.update(cx, |app, cx| {
                        app.apply_to_remaining = !app.apply_to_remaining;
                        cx.notify();
                    });
                }),
            )
            .child(h_flex().gap(px(8.0)).flex_wrap().children(
                conflict.choices.iter().copied().map(|resolution| {
                    let prompt = conflict.clone();
                    let this = this.clone();
                    let button_id =
                        SharedString::from(format!("resolve-{}-{resolution:?}", id.value()));
                    let label = resolution_label(resolution, c);
                    // The answer that keeps both is the safe default, as
                    // in folio; it is the accent button.
                    let button = if resolution == Resolution::Rename {
                        KitButton::accent(button_id, label)
                    } else {
                        KitButton::standard(button_id, label)
                    };
                    button.on_click(move |_window, cx| {
                        let _ = this.update(cx, |app, cx| {
                            let decision = prompt.decision(resolution, app.apply_to_remaining);
                            app.session.resolve_conflict(id, decision);
                            app.apply_to_remaining = false;
                            cx.notify();
                        });
                    })
                }),
            ))
            .into_any_element()
    }

    /// The modal dialogs: a name to type, or a permanent delete to confirm.
    /// The preview pane, on the right of the content area.
    ///
    /// Everything drawn here was produced on the preview worker thread. This
    /// method formats a value; it opens no file and decodes nothing.
    pub(crate) fn preview_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use better_ui::fluent::{self, BODY, BODY_STRONG, CAPTION};
        let c = crate::i18n::copy(self.locale());
        let p = fluent::palette(cx);
        let mut column = v_flex()
            .id("preview-pane")
            .w(px(320.0))
            .flex_shrink_0()
            .h_full()
            .gap(px(8.0))
            .p(px(16.0))
            .overflow_y_scroll()
            .border_l_1()
            .border_color(p.divider)
            .bg(p.window_background)
            .child(fluent::label(c.preview, CAPTION, p.text_secondary));

        // The details, as folio's pane has them: what it is, how big, when it
        // changed, what opens it, and for something in the Trash where it
        // came from and when it was deleted.
        if let Some(entry) = self.session.focused_entry().cloned() {
            let names = self.type_names.clone();
            let describe = |mime: &str| names.describe(mime);
            let type_label = crate::content::type_label_named(&entry, c, &describe);
            let row = |label: &'static str, value: String| {
                v_flex()
                    .gap(px(2.0))
                    .child(fluent::label(label, CAPTION, p.text_secondary))
                    .child(fluent::label(value, BODY, p.text_primary))
            };
            column = column
                .child(fluent::label(
                    entry.name.clone(),
                    BODY_STRONG,
                    p.text_primary,
                ))
                .child(row(c.details_type, type_label));
            if let files_core::EntrySize::Bytes(bytes) = entry.size {
                column = column.child(row(c.details_size, crate::format::bytes(bytes)));
            }
            match &entry.body {
                files_core::EntryBody::Trashed(facts) => {
                    column = column
                        .child(row(
                            c.details_original_location,
                            facts.original_path.display().to_string(),
                        ))
                        .child(row(
                            c.details_deleted,
                            crate::format::file_time(facts.deleted_at),
                        ));
                }
                _ => {
                    if entry.modified.is_some() {
                        column = column.child(row(
                            c.details_modified,
                            crate::format::file_time(entry.modified),
                        ));
                    }
                    if let Some(app) = self.session.opens_with(&entry) {
                        column = column.child(row(c.details_opens_with, app));
                    }
                }
            }
            // Plugins' rows (folio's `details`), fetched in the background.
            if let Some(path) = entry.as_local_path() {
                let mime = entry.mime.as_ref().map(|mime| mime.as_str().to_string());
                for (label, value) in self.plugins.details_for(path.as_path(), mime.as_deref()) {
                    column = column.child(row_owned(label, value, p));
                }
            }
            column = column.child(div().h(px(1.0)).bg(p.divider));
        }

        if let Some(placeholder) = self.session.preview.placeholder(c) {
            return column
                .child(fluent::label(placeholder, BODY, p.text_secondary))
                .into_any_element();
        }

        let crate::preview::PreviewSlot::Ready(preview) = self.session.preview.slot() else {
            return column.into_any_element();
        };
        match preview.as_ref() {
            files_preview::Preview::Image(image) => {
                column = column.child(fluent::label(
                    format!(
                        "{} · {} {} × {}",
                        image.format, c.preview_dimensions, image.source_width, image.source_height
                    ),
                    BODY,
                    p.text_primary,
                ));
            }
            files_preview::Preview::Text(text) => {
                column = column
                    .child(fluent::label(
                        format!(
                            "{}: {} · {} {}",
                            c.preview_encoding,
                            text.encoding.as_str(),
                            text.lines,
                            c.item_count
                        ),
                        CAPTION,
                        p.text_secondary,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .text_size(px(12.0))
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(p.text_primary)
                            .overflow_hidden()
                            .child(text.text.clone()),
                    );
                if text.truncated {
                    column = column.child(fluent::label(
                        c.preview_truncated,
                        CAPTION,
                        p.text_secondary,
                    ));
                }
            }
            files_preview::Preview::Folder(summary) => {
                column = column
                    .child(fluent::label(
                        format!(
                            "{} {} · {} {}",
                            summary.files,
                            c.preview_folder_files,
                            summary.directories,
                            c.preview_folder_folders
                        ),
                        BODY,
                        p.text_primary,
                    ))
                    .child(fluent::label(
                        format!(
                            "{}: {}",
                            c.preview_folder_size,
                            crate::format::bytes(summary.immediate_bytes)
                        ),
                        BODY,
                        p.text_primary,
                    ));
                if summary.truncated {
                    column = column.child(fluent::label(
                        c.preview_folder_truncated,
                        CAPTION,
                        p.text_secondary,
                    ));
                }
            }
            files_preview::Preview::Metadata(meta) => {
                column = column.child(fluent::label(
                    crate::preview::degrade_message(&meta.reason, c),
                    BODY,
                    p.text_secondary,
                ));
            }
        }
        column.into_any_element()
    }

    /// The application details panel.
    ///
    /// The desktop entry's path is here, under a heading that says what it is.
    /// It is a diagnostic, never the application's identity and never something
    /// a click opens.
    pub(crate) fn application_details(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use better_ui::fluent::{self, BODY, BODY_STRONG, CAPTION, button::Button as KitButton};
        let c = crate::i18n::copy(self.locale());
        let p = fluent::palette(cx);
        let details = self.session.details.clone()?;
        let this = cx.entity().downgrade();
        let line = |text: String| fluent::label(text, CAPTION, p.text_secondary);
        let mut column = v_flex()
            .id("app-details")
            .w(px(360.0))
            .flex_shrink_0()
            .h_full()
            .gap(px(8.0))
            .p(px(16.0))
            .overflow_y_scroll()
            .border_l_1()
            .border_color(p.divider)
            .bg(p.window_background)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .child(
                        fluent::label(details.name.clone(), BODY_STRONG, p.text_primary).flex_1(),
                    )
                    .child(
                        KitButton::icon_button("close-details", IconName::Close, c.close).on_click(
                            move |_window, cx| {
                                let _ = this.update(cx, |app, cx| {
                                    app.session.close_details();
                                    cx.notify();
                                });
                            },
                        ),
                    ),
            );
        if let Some(comment) = details.comment.as_ref() {
            column = column.child(fluent::label(comment.clone(), BODY, p.text_primary));
        }
        column = column.child(line(format!(
            "{} · {}",
            crate::i18n::source_kind_label(details.source_kind, c),
            crate::i18n::scope_label_for(details.scope, c)
        )));
        if !details.categories.is_empty() {
            column = column.child(line(format!(
                "{}: {}",
                c.app_details_categories,
                details.categories.join(", ")
            )));
        }
        if !details.mime_types.is_empty() {
            column = column.child(line(format!(
                "{}: {}",
                c.app_details_mime_types,
                details.mime_types.join(", ")
            )));
        }
        column = column.child(line(match &details.executable {
            crate::apps::ExecutableSummary::Resolved(path) => {
                format!("{}: {path}", c.app_details_executable)
            }
            crate::apps::ExecutableSummary::Unresolved(program) => {
                format!("{}: {program}", c.app_details_executable)
            }
            crate::apps::ExecutableSummary::NotApplicable(_) => {
                c.app_details_no_executable.to_string()
            }
        }));
        if details.dbus_activatable {
            column = column.child(line(c.app_details_dbus_activatable.to_string()));
        }
        // The diagnostic, labelled as one.
        column = column.child(line(format!(
            "{}: {}",
            c.app_details_source, details.source_path
        )));
        for warning in &details.warnings {
            column = column.child(fluent::label(
                format!("{}: {warning}", c.app_details_warnings),
                CAPTION,
                p.caution,
            ));
        }
        Some(column.into_any_element())
    }

    /// Better App Chooser, embedded.
    pub(crate) fn chooser_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let entity = self.chooser.clone()?;
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(cx.theme().background.opacity(0.8))
                .child(
                    div()
                        .w(px(680.0))
                        .max_h(px(560.0))
                        .rounded_lg()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .child(entity),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use better_ui::fluent::{
            self, BODY,
            dialog::{ContentDialog, DefaultButton, DialogButtons, DialogResult},
            text_box::TextBox,
        };
        let c = copy(self.locale());
        let dialog = self.session.dialog.clone()?;
        if let PendingDialog::Compress(chooser) = &dialog {
            return Some(self.compress_dialog(chooser, cx));
        }
        let (title, confirm_label) = match &dialog {
            PendingDialog::NewFolder => (c.new_folder_name, c.new_folder),
            PendingDialog::NewFile => (c.new_file_name, c.new_file),
            PendingDialog::Rename(_) => (c.rename_to, c.rename),
            PendingDialog::RenameBookmark(_) => (c.bookmark_label_placeholder, c.rename_bookmark),
            PendingDialog::ConfirmDelete { .. } => (c.confirm_delete_title, c.confirm),
            PendingDialog::Compress(_) => (c.compress_title, c.compress),
            PendingDialog::ServerLogin { .. } => (c.server_sign_in, c.server_connect),
            PendingDialog::Format { .. } => (c.format_title, c.format_confirm),
            PendingDialog::WriteImage { .. } => (c.write_image_title, c.write_image_confirm),
        };
        let title: SharedString = match &dialog {
            PendingDialog::Format { device_label, .. } => {
                title.replace("{device}", device_label).into()
            }
            PendingDialog::WriteImage {
                device_label,
                image,
                ..
            } => title
                .replace("{device}", device_label)
                .replace("{image}", &image.file_name())
                .into(),
            _ => title.into(),
        };
        let p = fluent::palette(cx);
        let this = cx.entity().downgrade();
        let mut content = ContentDialog::new("files-dialog", title)
            .buttons(DialogButtons {
                primary: Some(confirm_label.into()),
                secondary: None,
                close: Some(c.dismiss.into()),
                default: DefaultButton::Primary,
            })
            .on_result(move |result, _window, cx| {
                let _ = this.update(cx, |app, cx| match result {
                    DialogResult::Primary => app.submit_dialog(cx),
                    _ => app.dismiss_dialog(cx),
                });
            });
        content = match &dialog {
            PendingDialog::ConfirmDelete { targets } => content.child(fluent::label(
                format!("{} ({})", c.confirm_delete_body, targets.len()),
                BODY,
                p.text_primary,
            )),
            PendingDialog::ServerLogin { address } => content
                .child(fluent::label(address.clone(), BODY, p.text_secondary))
                .child(TextBox::new("server-user", &self.dialog_input).header(c.server_user))
                .child(
                    TextBox::password("server-password", &self.password_input)
                        .header(c.server_password),
                ),
            PendingDialog::Format { filesystem, .. } => {
                use better_ui::fluent::toggle::{RadioButtons, RadioOption};
                use storage_platform::FilesystemKind;
                let kinds = FilesystemKind::ALL;
                let options = kinds
                    .iter()
                    .map(|kind| {
                        RadioOption::new(match kind {
                            FilesystemKind::Exfat => c.format_exfat,
                            FilesystemKind::Fat32 => c.format_fat32,
                            FilesystemKind::Ext4 => c.format_ext4,
                        })
                    })
                    .collect();
                let selected = kinds.iter().position(|kind| kind == filesystem);
                let this = cx.entity().downgrade();
                content
                    .child(fluent::label(c.format_warning, BODY, p.critical))
                    .child(
                        RadioButtons::new("format-filesystem", options, selected).on_select(
                            move |index, _window, cx| {
                                let _ = this.update(cx, |app, cx| {
                                    app.session.choose_filesystem(kinds[index]);
                                    cx.notify();
                                });
                            },
                        ),
                    )
                    .child(TextBox::new("format-name", &self.dialog_input).header(c.format_name))
            }
            PendingDialog::WriteImage { .. } => {
                content.child(fluent::label(c.format_warning, BODY, p.critical))
            }
            _ => content.child(TextBox::new("dialog-field", &self.dialog_input)),
        };
        Some(content.into_any_element())
    }

    /// The format chooser for Compress: one button per format, each labelled
    /// with the archive's file name, so the choice shows what will be made.
    /// The one the keyboard is on is the accent button, which is the one
    /// Enter presses; `keys::chooser_key_for` moves it.
    fn compress_dialog(
        &mut self,
        chooser: &crate::commands::CompressChooser,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use better_ui::fluent::{
            button::Button as KitButton,
            dialog::{ContentDialog, DefaultButton, DialogButtons},
        };
        let c = copy(self.locale());
        let focused = chooser.focused();
        let mut formats = v_flex().gap(px(8.0));
        for format in files_operations::ArchiveFormat::ALL {
            let label = chooser
                .request
                .file_name(format)
                .to_string_lossy()
                .into_owned();
            let button_id = SharedString::from(format!("compress-{}", format.extension()));
            let button = if format == focused {
                KitButton::accent(button_id, label)
            } else {
                KitButton::standard(button_id, label)
            };
            let this = cx.entity().downgrade();
            formats = formats.child(button.full_width().on_click(move |_window, cx| {
                let _ = this.update(cx, |app, cx| {
                    app.session.compress(format);
                    cx.notify();
                });
            }));
        }
        let this = cx.entity().downgrade();
        ContentDialog::new("compress-dialog", c.compress_title)
            .buttons(DialogButtons {
                primary: None,
                secondary: None,
                close: Some(c.dismiss.into()),
                default: DefaultButton::None,
            })
            .on_result(move |_result, _window, cx| {
                let _ = this.update(cx, |app, cx| app.dismiss_dialog(cx));
            })
            .child(formats)
            .into_any_element()
    }

    /// The Performance mode confirmation: every risk in words, each one ticked
    /// by hand, and a confirm button that stays disabled until all of them
    /// are. The decisions are `crate::policy`'s; this only draws them.
    pub(crate) fn policy_dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use better_ui::fluent::{
            self, BODY,
            button::Button as KitButton,
            toggle::{CheckBox, CheckState},
        };
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let confirmation = self.session.policy_confirmation.clone()?;
        let can_confirm = confirmation.can_confirm();

        let mut risks = v_flex().gap(px(8.0));
        for (key, ticked) in confirmation.risks() {
            let text = crate::policy::risk_text(key, c).unwrap_or(key);
            let this = cx.entity().downgrade();
            risks = risks.child(
                CheckBox::new(
                    SharedString::from(format!("policy-risk-{key}")),
                    if ticked {
                        CheckState::Checked
                    } else {
                        CheckState::Unchecked
                    },
                )
                .label(text)
                .on_change(move |_state, _window, cx| {
                    let _ = this.update(cx, |app, cx| {
                        app.session.toggle_policy_risk(key);
                        cx.notify();
                    });
                }),
            );
        }
        let cancel = cx.entity().downgrade();
        let confirm = cancel.clone();
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(p.smoke)
                .child(
                    v_flex()
                        .w(px(460.0))
                        .max_w_full()
                        .gap(px(12.0))
                        .p(px(24.0))
                        .rounded(fluent::radius::MD)
                        .border_1()
                        .border_color(p.stroke)
                        .bg(p.dialog)
                        .child(fluent::label(
                            c.policy_title.replace("{device}", &confirmation.label),
                            fluent::SUBTITLE,
                            p.text_primary,
                        ))
                        .child(fluent::label(c.policy_intro, BODY, p.text_secondary))
                        .child(risks)
                        .child(
                            h_flex()
                                .gap(px(8.0))
                                .justify_end()
                                .flex_wrap()
                                .child(
                                    KitButton::standard("policy-cancel", c.policy_cancel).on_click(
                                        move |_window, cx| {
                                            let _ = cancel.update(cx, |app, cx| {
                                                app.session.cancel_policy();
                                                cx.notify();
                                            });
                                        },
                                    ),
                                )
                                .child(
                                    KitButton::accent("policy-confirm", c.policy_turn_on)
                                        .disabled(!can_confirm)
                                        .on_click(move |_window, cx| {
                                            let _ = confirm.update(cx, |app, cx| {
                                                app.session.confirm_policy();
                                                cx.notify();
                                            });
                                        }),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn row_owned(label: String, value: String, p: &better_ui::fluent::Palette) -> Div {
    use better_ui::fluent::{self, BODY, CAPTION};
    v_flex()
        .gap(px(2.0))
        .child(fluent::label(label, CAPTION, p.text_secondary))
        .child(fluent::label(value, BODY, p.text_primary))
}
