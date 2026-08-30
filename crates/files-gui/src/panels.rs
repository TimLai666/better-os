//! The operation center panel and the modal dialogs.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    input::Input,
    scroll::ScrollableElement,
    *,
};

use crate::app::FilesApp;
use crate::i18n::{copy, resolution_label};
use crate::opcenter::{ConflictPrompt, JobRow};
use crate::session::PendingDialog;

impl FilesApp {
    /// The panel listing every job the shared engine knows about.
    pub(crate) fn operation_center(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let rows = self.session.job_rows();
        let finished: Vec<JobRow> = self.session.finished.rows().into_iter().cloned().collect();

        v_flex()
            .w(px(420.0))
            .flex_shrink_0()
            .h_full()
            .gap_2()
            .p_3()
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .overflow_y_scrollbar()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(div().font_semibold().child(c.operation_center))
                    .child(
                        Button::new("operations-close")
                            .label(c.dismiss)
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.session.operations_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .when(rows.is_empty() && finished.is_empty(), |panel| {
                panel.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.no_operations),
                )
            })
            .children(rows.into_iter().map(|row| self.job_card(row, cx)))
            .when(!finished.is_empty(), |panel| {
                panel.child(
                    div()
                        .pt_2()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.completed_this_session),
                )
            })
            .children(finished.into_iter().map(|row| self.finished_card(row, cx)))
            .into_any_element()
    }

    fn job_card(&self, row: JobRow, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let id = row.id;
        let fraction = row.fraction.unwrap_or(0.0).clamp(0.0, 1.0);
        let indeterminate = row.fraction.is_none();

        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .p_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child(row.title.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(row.state_label.clone()),
                    ),
            )
            // The bar. An indeterminate job draws an empty track rather than a
            // bar at zero, because "no total yet" and "nothing done" look the
            // same at zero and mean different things.
            .child(
                div()
                    .w_full()
                    .h(px(6.0))
                    .rounded_full()
                    .bg(cx.theme().muted)
                    .when(!indeterminate, |track| {
                        track.child(
                            div()
                                .h_full()
                                .w(relative(fraction as f32))
                                .rounded_full()
                                .bg(cx.theme().primary),
                        )
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} · {} · {} {} · {} {}",
                        row.items_label,
                        row.bytes_label,
                        c.throughput,
                        row.throughput_label,
                        c.remaining,
                        row.remaining_label
                    )),
            )
            .when_some(row.current.clone(), |element, current| {
                element.child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(current.to_string_lossy().into_owned()),
                )
            })
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .when(row.controls.pause, |bar| {
                        bar.child(
                            Button::new(SharedString::from(format!("pause-{}", id.value())))
                                .label(c.pause)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.session.pause_job(id);
                                    cx.notify();
                                })),
                        )
                    })
                    .when(row.controls.resume, |bar| {
                        bar.child(
                            Button::new(SharedString::from(format!("resume-{}", id.value())))
                                .label(c.resume)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.session.resume_job(id);
                                    cx.notify();
                                })),
                        )
                    })
                    .when(row.controls.cancel, |bar| {
                        bar.child(
                            Button::new(SharedString::from(format!("cancel-{}", id.value())))
                                .label(c.cancel)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.session.cancel_job(id);
                                    cx.notify();
                                })),
                        )
                    })
                    .when(row.controls.retry, |bar| {
                        bar.child(
                            Button::new(SharedString::from(format!("retry-{}", id.value())))
                                .label(c.retry_failed)
                                .xsmall()
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.session.retry_job(id);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(!row.failures.is_empty(), |element| {
                element.child(
                    v_flex()
                        .gap_0p5()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().danger)
                                .child(format!("{} · {}", c.failures, row.failures.len())),
                        )
                        .children(row.failures.iter().take(5).map(|failure| {
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} — {}",
                                    failure.path.to_string_lossy(),
                                    failure.reason
                                ))
                        })),
                )
            })
            .when_some(row.conflict.clone(), |element, conflict| {
                element.child(self.conflict_card(conflict, cx))
            })
            .into_any_element()
    }

    fn finished_card(&self, row: JobRow, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .justify_between()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().child(row.title.clone()))
            .child(div().child(format!("{} · {}", row.items_label, row.state_label)))
            .into_any_element()
    }

    /// The conflict prompt: the choices, and the switch that applies the next
    /// answer to every remaining conflict of the same kind.
    fn conflict_card(&self, conflict: ConflictPrompt, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let id = conflict.job;
        let apply = self.apply_to_remaining;
        v_flex()
            .w_full()
            .gap_1()
            .mt_1()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().warning)
            .text_color(cx.theme().warning_foreground)
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .child(c.conflict_needs_a_decision),
            )
            .child(div().text_xs().child(conflict.title.clone()))
            .child(
                div()
                    .truncate()
                    .text_xs()
                    .child(conflict.destination.to_string_lossy().into_owned()),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "apply-remaining-{}",
                    id.value()
                )))
                .when(apply, |button| button.primary())
                .label(c.apply_to_remaining)
                .xsmall()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.apply_to_remaining = !this.apply_to_remaining;
                    cx.notify();
                })),
            )
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .children(conflict.choices.iter().copied().map(|resolution| {
                        let prompt = conflict.clone();
                        Button::new(SharedString::from(format!(
                            "resolve-{}-{resolution:?}",
                            id.value()
                        )))
                        .label(resolution_label(resolution, c))
                        .xsmall()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let decision = prompt.decision(resolution, this.apply_to_remaining);
                            this.session.resolve_conflict(id, decision);
                            this.apply_to_remaining = false;
                            cx.notify();
                        }))
                    })),
            )
            .into_any_element()
    }

    /// The modal dialogs: a name to type, or a permanent delete to confirm.
    pub(crate) fn dialog(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.locale());
        let dialog = self.session.dialog.clone()?;
        let (title, confirm_label) = match &dialog {
            PendingDialog::NewFolder => (c.new_folder_name, c.new_folder),
            PendingDialog::NewFile => (c.new_file_name, c.new_file),
            PendingDialog::Rename(_) => (c.rename_to, c.rename),
            PendingDialog::RenameBookmark(_) => (c.bookmark_label_placeholder, c.rename_bookmark),
            PendingDialog::ConfirmDelete { .. } => (c.confirm_delete_title, c.confirm),
        };
        let confirming_delete = matches!(dialog, PendingDialog::ConfirmDelete { .. });
        let count = match &dialog {
            PendingDialog::ConfirmDelete { targets } => targets.len(),
            _ => 0,
        };

        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.4))
                .child(
                    v_flex()
                        .w(px(420.0))
                        .gap_3()
                        .p_4()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .child(div().font_semibold().child(title))
                        .when(confirming_delete, |element| {
                            element.child(
                                div()
                                    .text_sm()
                                    .child(format!("{} ({count})", c.confirm_delete_body)),
                            )
                        })
                        .when(!confirming_delete, |element| {
                            element.child(Input::new(&self.dialog_input))
                        })
                        .child(
                            h_flex()
                                .gap_2()
                                .justify_end()
                                .child(Button::new("dialog-cancel").label(c.dismiss).on_click(
                                    cx.listener(|this, _, _, cx| this.dismiss_dialog(cx)),
                                ))
                                .child(
                                    Button::new("dialog-confirm")
                                        .primary()
                                        .when(confirming_delete, |button| button.danger())
                                        .label(confirm_label)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.submit_dialog(cx)),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
