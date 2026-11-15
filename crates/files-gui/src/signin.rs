//! What GVfs asks while it opens a server (ticket 117), in the content area
//! where the folder will be: the sign-in form, a host key it does not know,
//! or any other question, in the Fluent look.

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
        let (icon, title, body): (IconName, String, AnyElement) = match &ask.prompt {
            Prompt::SignIn {
                message,
                fields,
                again,
                ..
            } => (
                IconName::User,
                c.server_sign_in_to.replace("{server}", &server),
                self.sign_in_form(message, *fields, *again, cx),
            ),
            Prompt::Question { message, choices } => {
                match HostKeyQuestion::read(&ask.address, message, choices) {
                    Some(HostKeyQuestion::Unknown { fingerprint }) => (
                        IconName::Info,
                        c.server_key_unknown_title.replace("{server}", &server),
                        self.host_key_body(
                            c.server_key_unknown_body.replace("{server}", &server),
                            Some(fingerprint),
                            false,
                            cx,
                        ),
                    ),
                    Some(HostKeyQuestion::Mismatch) => (
                        IconName::TriangleAlert,
                        c.server_key_mismatch_title.replace("{server}", &server),
                        self.host_key_body(
                            c.server_key_mismatch_body.replace("{server}", &server),
                            None,
                            true,
                            cx,
                        ),
                    ),
                    None => (
                        IconName::Info,
                        c.server_question_title.replace("{server}", &server),
                        self.question_body(message, choices, cx),
                    ),
                }
            }
        };
        let warning = matches!(icon, IconName::TriangleAlert);
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
                                    gpui_component::Icon::new(icon)
                                        .size(px(24.0))
                                        .text_color(if warning { p.caution } else { p.accent }),
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
                                app.session.answer_server(Answer::Choice(0));
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
