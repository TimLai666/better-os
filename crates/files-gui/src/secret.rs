//! A passphrase or password on its way from a field to the service that
//! checks it, and nowhere else: it cannot be cloned or printed, its bytes are
//! overwritten when it is dropped, and the field it came from is emptied
//! with its undo history.

use gpui::{Entity, Window};
use gpui_component::input::InputState;

/// Text that overwrites itself on drop. Not `Clone`; `Debug` hides it.
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }

    /// The text, for the one call that needs it.
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        for byte in self.0.iter_mut() {
            // SAFETY: `byte` is a valid, aligned, writable `u8` this vector
            // owns; the volatile write keeps the compiler from leaving it out
            // because the memory is about to be freed.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(…)")
    }
}

/// Takes what was typed into a password field and empties the field and
/// its undo history, so nothing of it can be brought back with Ctrl+Z.
pub fn take(field: &Entity<InputState>, window: &mut Window, cx: &mut gpui::App) -> Secret {
    let secret = Secret::new(field.read(cx).value().as_ref());
    forget(field, window, cx);
    secret
}

/// Empties a password field and its undo history.
pub fn forget(field: &Entity<InputState>, window: &mut Window, cx: &mut gpui::App) {
    // `set_value` clears the undo history as it replaces the text.
    field.update(cx, |state, cx| state.set_value("", window, cx));
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext, VisualTestContext};

    struct Form {
        field: Entity<InputState>,
    }

    impl gpui::Render for Form {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{ParentElement as _, Styled as _};
            gpui::div()
                .size_full()
                .child(gpui_component::input::Input::new(&self.field))
        }
    }

    fn form(cx: &mut TestAppContext) -> (Entity<Form>, VisualTestContext) {
        cx.update(gpui_component::init);
        // gpui-component's fields want its `Root` as the window's root.
        let mut form = None;
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|cx| Form {
                field: cx.new(|cx| InputState::new(window, cx).masked(true)),
            });
            form = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let cx = VisualTestContext::from_window(window.into(), cx);
        (form.expect("the form"), cx)
    }

    /// Types `text` the way a person does, so it lands in the undo history.
    fn type_into(field: &Entity<InputState>, text: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            field.update(cx, |state, cx| {
                state.focus(window, cx);
                state.replace_all(text, window, cx);
            });
        });
    }

    /// Ctrl+Z in the field, as a person would press it.
    fn undo(field: &Entity<InputState>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| field.update(cx, |state, cx| state.focus(window, cx)));
        cx.simulate_keystrokes("ctrl-z");
        cx.run_until_parked();
    }

    #[gpui::test]
    fn a_taken_password_leaves_nothing_to_undo(cx: &mut TestAppContext) {
        let (form, mut cx) = form(cx);
        let field = form.read_with(&cx, |form, _| form.field.clone());
        type_into(&field, "hunter2", &mut cx);
        let secret = cx.update(|window, cx| take(&field, window, cx));
        assert_eq!(secret.expose(), "hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(…)");
        undo(&field, &mut cx);
        assert_eq!(
            field.read_with(&cx, |state, _| state.value().to_string()),
            ""
        );
    }

    #[gpui::test]
    fn a_forgotten_password_leaves_nothing_to_undo(cx: &mut TestAppContext) {
        let (form, mut cx) = form(cx);
        let field = form.read_with(&cx, |form, _| form.field.clone());
        type_into(&field, "hunter2", &mut cx);
        cx.update(|window, cx| forget(&field, window, cx));
        undo(&field, &mut cx);
        assert_eq!(
            field.read_with(&cx, |state, _| state.value().to_string()),
            ""
        );
    }
}
