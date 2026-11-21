//! What GVfs asks while it opens a server (ticket 117), in the content area
//! where the folder will be: the sign-in form, a host key it does not know,
//! or any other question, in the Fluent look.

use better_ui::icon_set::{Glyph, Picture, Placement};
use gpui::*;
use gpui_component::IconName;

use crate::app::FilesApp;
use crate::i18n::copy;
use crate::network::{Answer, HostKeyQuestion, Prompt, Remember, server_name};

impl FilesApp {
    /// Readies the form for a question that has just arrived: the user and
    /// domain GVfs suggests (or what the person typed, when it asks again
    /// after a wrong password), an empty password, nothing remembered.
    pub(crate) fn prepare_server_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.server_prepared == self.session.server_prompt_serial {
            return;
        }
        self.server_prepared = self.session.server_prompt_serial;
        let Some(ask) = &self.session.server_prompt else {
            return;
        };
        let Prompt::SignIn {
            user,
            domain,
            again,
            ..
        } = &ask.prompt
        else {
            return;
        };
        if !again {
            let (user, domain) = (user.clone(), domain.clone());
            self.server_user_input
                .update(cx, |state, cx| state.set_value(user, window, cx));
            self.server_domain_input
                .update(cx, |state, cx| state.set_value(domain, window, cx));
            self.server_remember = Remember::Never;
            self.server_anonymous = false;
        }
        crate::secret::forget(&self.server_password_input, window, cx);
        // The password is what is left to type: it takes the keys.
        self.server_password_input
            .update(cx, |state, cx| state.focus(window, cx));
    }

    /// Answers the sign-in with what was typed. The password is taken out
    /// of its field (and its undo history) and goes to GVfs only.
    pub(crate) fn submit_server_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ask) = &self.session.server_prompt else {
            return;
        };
        if !matches!(ask.prompt, Prompt::SignIn { .. }) {
            return;
        }
        let user = self.server_user_input.read(cx).value().to_string();
        let domain = self.server_domain_input.read(cx).value().to_string();
        let password = crate::secret::take(&self.server_password_input, window, cx);
        self.session.answer_server(Answer::SignIn {
            user,
            domain,
            password,
            anonymous: self.server_anonymous,
            remember: self.server_remember,
        });
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Cancels what is asked: the server is not opened, and nothing is said.
    pub(crate) fn cancel_server_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::secret::forget(&self.server_password_input, window, cx);
        self.session.cancel_server_prompt();
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The page for the question shown, if one is.
    pub(crate) fn server_page(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use better_ui::fluent::{self, CAPTION, SUBTITLE};
        let ask = self.session.server_prompt.as_ref()?;
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let server = server_name(&ask.address);
        let (icon, title, body): (Glyph, String, AnyElement) = match &ask.prompt {
            Prompt::SignIn {
                message,
                fields,
                again,
                ..
            } => (
                Picture::User.into(),
                c.server_sign_in_to.replace("{server}", &server),
                self.sign_in_form(message, *fields, *again, cx),
            ),
            Prompt::Question { message, choices } => {
                match HostKeyQuestion::read(&ask.address, message, choices) {
                    Some(HostKeyQuestion::Unknown { fingerprint }) => (
                        IconName::Info.into(),
                        c.server_key_unknown_title.replace("{server}", &server),
                        self.host_key_body(
                            c.server_key_unknown_body.replace("{server}", &server),
                            Some(fingerprint),
                            false,
                            cx,
                        ),
                    ),
                    Some(HostKeyQuestion::Mismatch) => (
                        IconName::TriangleAlert.into(),
                        c.server_key_mismatch_title.replace("{server}", &server),
                        self.host_key_body(
                            c.server_key_mismatch_body.replace("{server}", &server),
                            None,
                            true,
                            cx,
                        ),
                    ),
                    None => (
                        IconName::Info.into(),
                        c.server_question_title.replace("{server}", &server),
                        self.question_body(message, choices, cx),
                    ),
                }
            }
        };
        let warning = matches!(icon, Glyph::Symbol(IconName::TriangleAlert));
        Some(
            div()
                .id("server-prompt")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .justify_center()
                .p(px(32.0))
                .child(
                    div()
                        .w_full()
                        .max_w(px(480.0))
                        .flex()
                        .flex_col()
                        .gap(px(16.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .flex()
                                        .text_color(if warning { p.caution } else { p.accent })
                                        .child(icon.element(
                                            px(24.0),
                                            Placement::Labelled,
                                            p,
                                            false,
                                        )),
                                )
                                .child(
                                    fluent::label(title, SUBTITLE, p.text_primary)
                                        .min_w_0()
                                        .whitespace_normal(),
                                ),
                        )
                        .child(body)
                        .child(fluent::label(
                            ask.address.clone(),
                            CAPTION,
                            p.text_secondary,
                        )),
                )
                .into_any_element(),
        )
    }

    fn sign_in_form(
        &self,
        message: &str,
        fields: crate::network::SignInFields,
        again: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use better_ui::fluent::{
            self, CAPTION,
            button::Button,
            info_bar::{InfoBar, Severity},
            text_box::TextBox,
            toggle::{RadioButtons, RadioOption},
        };
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let this = cx.entity().downgrade();
        let anonymous = self.server_anonymous;
        let mut form = div().flex().flex_col().gap(px(12.0));
        if again {
            form = form.child(InfoBar::new(
                "server-wrong-password",
                Severity::Error,
                c.server_wrong_password,
            ));
        }
        if !message.trim().is_empty() {
            // GVfs's own words, in the desktop's language: which password it
            // wants (the account's, or a key's passphrase).
            form = form.child(
                fluent::label(message.trim().to_string(), CAPTION, p.text_secondary)
                    .whitespace_normal(),
            );
        }
        if fields.anonymous {
            let weak = this.clone();
            form = form.child(
                RadioButtons::new(
                    "server-anonymous",
                    vec![
                        RadioOption::new(c.server_as_user),
                        RadioOption::new(c.server_anonymous),
                    ],
                    Some(usize::from(anonymous)),
                )
                .on_select(move |index, _window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.server_anonymous = index == 1;
                        cx.notify();
                    });
                }),
            );
        }
        if !anonymous {
            if fields.user {
                form = form.child(
                    TextBox::new("server-user", &self.server_user_input).header(c.server_user),
                );
            }
            if fields.domain {
                form = form.child(
                    TextBox::new("server-domain", &self.server_domain_input)
                        .header(c.server_domain),
                );
            }
            if fields.password {
                form = form.child(
                    TextBox::password("server-password", &self.server_password_input)
                        .header(c.server_password),
                );
            }
            if fields.remember {
                let weak = this.clone();
                let options = Remember::ALL;
                form = form.child(
                    RadioButtons::new(
                        "server-remember",
                        vec![
                            RadioOption::new(c.server_remember_never),
                            RadioOption::new(c.server_remember_session),
                            RadioOption::new(c.server_remember_forever),
                        ],
                        options
                            .iter()
                            .position(|kind| *kind == self.server_remember),
                    )
                    .on_select(move |index, _window, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.server_remember = options[index];
                            cx.notify();
                        });
                    }),
                );
            }
        }
        let connect = this.clone();
        let cancel = this;
        form.child(
            div()
                .flex()
                .gap(px(8.0))
                .pt(px(8.0))
                .child(Button::accent("server-connect", c.server_connect).on_click(
                    move |window, cx| {
                        let _ = connect.update(cx, |app, cx| app.submit_server_sign_in(window, cx));
                    },
                ))
                .child(Button::standard("server-cancel", c.server_cancel).on_click(
                    move |window, cx| {
                        let _ = cancel.update(cx, |app, cx| app.cancel_server_prompt(window, cx));
                    },
                )),
        )
        .into_any_element()
    }

    /// A host key's page: what it means, the fingerprint (monospace,
    /// wrapping, selectable), Connect anyway and Cancel.
    fn host_key_body(
        &self,
        explanation: String,
        fingerprint: Option<String>,
        warning: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use better_ui::fluent::{self, BODY, CAPTION, button::Button};
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let this = cx.entity().downgrade();
        let anyway = this.clone();
        let mut body = div().flex().flex_col().gap(px(12.0)).child(
            fluent::label(
                explanation,
                BODY,
                if warning { p.caution } else { p.text_primary },
            )
            .whitespace_normal(),
        );
        if let Some(fingerprint) = fingerprint {
            body = body
                .child(fluent::label(
                    c.server_key_fingerprint,
                    CAPTION,
                    p.text_secondary,
                ))
                .child(
                    div()
                        .p(px(8.0))
                        .rounded(fluent::radius::SM)
                        .bg(p.control_fill)
                        .border_1()
                        .border_color(p.stroke)
                        .child(
                            gpui_component::text::TextView::markdown(
                                "server-fingerprint",
                                format!("`{fingerprint}`"),
                            )
                            .selectable(true),
                        ),
                );
        }
        body.child(
            div()
                .flex()
                .gap(px(8.0))
                .pt(px(8.0))
                .child(
                    Button::standard("server-key-anyway", c.server_connect_anyway).on_click(
                        move |_window, cx| {
                            let _ = anyway.update(cx, |app, cx| {
                                app.session
                                    .answer_server(Answer::Choice(crate::network::CONNECT_ANYWAY));
                                cx.notify();
                            });
                        },
                    ),
                )
                .child(
                    Button::accent("server-key-cancel", c.server_cancel).on_click(
                        move |window, cx| {
                            let _ = this.update(cx, |app, cx| app.cancel_server_prompt(window, cx));
                        },
                    ),
                ),
        )
        .into_any_element()
    }

    /// The page a tab whose folder is on a server shows while it is not
    /// connected (ticket 310): connecting with Cancel, why it could not with
    /// Try again (and Network settings where that helps), not connected
    /// with Connect, or the address it needs.
    pub(crate) fn remote_page(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::remote::Reason;
        use crate::session::LinkState;
        use better_ui::fluent::{
            self, BODY, CAPTION, SUBTITLE,
            button::Button,
            progress::{Progress, ProgressRing},
        };
        let link = self.session.link()?.clone();
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let server = link.server();
        let fill = |text: &str| {
            text.replace("{server}", &server).replace(
                "{share}",
                link.remote
                    .as_ref()
                    .and_then(|remote| remote.share())
                    .unwrap_or(&server),
            )
        };
        let this = cx.entity().downgrade();
        let settings = crate::remote::network_settings(&crate::terminal::on_path).is_some();
        let (title, body, warning, busy) = match &link.state {
            LinkState::Connecting { again: false } => (
                fill(c.remote_connecting_title),
                c.remote_connecting_body.to_string(),
                false,
                true,
            ),
            LinkState::Connecting { again: true } => (
                fill(c.remote_again_title),
                c.remote_again_body.to_string(),
                false,
                true,
            ),
            LinkState::Cancelled => (
                fill(c.remote_cancelled_title),
                fill(c.remote_cancelled_body),
                false,
                false,
            ),
            LinkState::NeedsAddress => (
                c.remote_address_title.to_string(),
                c.remote_address_body.to_string(),
                true,
                false,
            ),
            LinkState::Failed { reason, .. } => {
                let sentence = match reason {
                    Reason::NoAnswer => c.remote_no_answer,
                    Reason::Refused => c.remote_refused,
                    Reason::NameNotFound => c.remote_name_not_found,
                    Reason::SignInRefused => c.remote_sign_in_refused,
                    Reason::ShareGone => c.remote_share_gone,
                    Reason::NoNetwork => c.remote_no_network,
                    Reason::Mount(failure) => {
                        use crate::network::MountFailure as F;
                        match failure {
                            F::NoGvfs => c.server_no_gvfs,
                            F::HostKeyChanged => c.server_key_changed,
                            F::NotSupported => c.server_not_supported,
                            F::NoLocalFolder => c.server_no_folder,
                            F::UntrustedGvfs => c.server_untrusted_gvfs,
                            _ => c.server_failed,
                        }
                    }
                };
                (fill(c.remote_failed_title), fill(sentence), true, false)
            }
        };
        let detail = match &link.state {
            LinkState::Failed { detail, .. } if !detail.is_empty() => Some(detail.clone()),
            _ => None,
        };
        let mut buttons = div().flex().flex_wrap().gap(px(8.0)).pt(px(8.0));
        match &link.state {
            LinkState::Connecting { .. } => {
                let weak = this.clone();
                buttons =
                    buttons.child(Button::standard("remote-cancel", c.server_cancel).on_click(
                        move |_window, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.session.cancel_connecting();
                                cx.notify();
                            });
                        },
                    ));
            }
            LinkState::Cancelled | LinkState::Failed { .. } => {
                let label = if link.state == LinkState::Cancelled {
                    c.remote_connect
                } else {
                    c.remote_try_again
                };
                let weak = this.clone();
                buttons = buttons.child(Button::accent("remote-retry", label).on_click(
                    move |_window, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.session.retry_connecting();
                            cx.notify();
                        });
                    },
                ));
                if let LinkState::Failed { reason, .. } = &link.state
                    && reason.offers_network_settings()
                    && settings
                {
                    let weak = this.clone();
                    buttons = buttons.child(
                        Button::standard("remote-network-settings", c.remote_network_settings)
                            .on_click(move |_window, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.session.open_network_settings(&crate::terminal::on_path);
                                    cx.notify();
                                });
                            }),
                    );
                }
            }
            LinkState::NeedsAddress => {
                let weak = this.clone();
                buttons = buttons.child(
                    Button::accent("remote-type-address", c.remote_type_address).on_click(
                        move |window, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.start_typing_address(window, cx);
                                cx.notify();
                            });
                        },
                    ),
                );
            }
        }
        let icon: Glyph = if warning {
            IconName::TriangleAlert.into()
        } else {
            IconName::Globe.into()
        };
        let address = link
            .remote
            .as_ref()
            .map(crate::remote::Remote::address)
            .unwrap_or_else(|| link.location.to_uri());
        Some(
            div()
                .id("remote-page")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .justify_center()
                .p(px(32.0))
                .child(
                    div()
                        .w_full()
                        .max_w(px(480.0))
                        .flex()
                        .flex_col()
                        .gap(px(16.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .child(if busy {
                                    ProgressRing::new("remote-ring", Progress::Indeterminate)
                                        .into_any_element()
                                } else {
                                    div()
                                        .flex()
                                        .text_color(if warning { p.caution } else { p.accent })
                                        .child(icon.element(
                                            px(24.0),
                                            Placement::Labelled,
                                            p,
                                            false,
                                        ))
                                        .into_any_element()
                                })
                                .child(
                                    fluent::label(title, SUBTITLE, p.text_primary)
                                        .min_w_0()
                                        .whitespace_normal(),
                                ),
                        )
                        .child(fluent::label(body, BODY, p.text_primary).whitespace_normal())
                        .children(detail.map(|detail| {
                            fluent::label(detail, CAPTION, p.text_secondary).whitespace_normal()
                        }))
                        .child(buttons)
                        .child(fluent::label(address, CAPTION, p.text_secondary)),
                )
                .into_any_element(),
        )
    }

    /// Any other question: GVfs's words and its own choices.
    fn question_body(
        &self,
        message: &str,
        choices: &[String],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use better_ui::fluent::{self, BODY, button::Button};
        let p = fluent::palette(cx);
        let this = cx.entity().downgrade();
        let buttons = choices.iter().enumerate().map(|(index, choice)| {
            let weak = this.clone();
            let id = SharedString::from(format!("server-choice-{index}"));
            let button = if index == 0 {
                Button::accent(id, choice.clone())
            } else {
                Button::standard(id, choice.clone())
            };
            button.on_click(move |_window, cx| {
                let _ = weak.update(cx, |app, cx| {
                    app.session.answer_server(Answer::Choice(index as u32));
                    cx.notify();
                });
            })
        });
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(fluent::label(message.to_string(), BODY, p.text_primary).whitespace_normal())
            .child(div().flex().flex_wrap().gap(px(8.0)).children(buttons))
            .into_any_element()
    }
}
