//! PAM's words mapped to Better OS's copy (design/login-screen-spec.md section
//! 3.9).
//!
//! The screen never decides that a password is wrong; it shows what PAM said.
//! The prompts and messages of `pam_unix` and libpam that it knows get Better
//! OS's copy; any other text is shown as PAM wrote it. The known texts were
//! read from `pam_unix.so` and `libpam.so.0` on the owner's Ubuntu 24.04 base
//! (`strings`, 2026-11-22); greetd's process runs PAM in the C locale, so they
//! arrive in English.

use crate::copy::Language;

/// What a PAM prompt or message is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PamText {
    /// `Password:`, the ordinary login prompt: the field keeps its
    /// placeholder.
    Password,
    /// The password change's first question.
    CurrentPassword,
    NewPassword,
    RetypeNewPassword,
    /// `pam_unix`'s "You are required to change your password immediately".
    PasswordExpired,
    /// `pam_unix`'s "Your account has expired".
    AccountExpired,
    /// Any other text, shown as it is.
    Other(String),
}

fn normalized(text: &str) -> String {
    text.trim()
        .trim_end_matches(':')
        .trim()
        .to_ascii_lowercase()
}

/// Classifies one prompt or message.
pub fn classify(text: &str) -> PamText {
    let key = normalized(text);
    match key.as_str() {
        "password" => PamText::Password,
        "current password" | "(current) unix password" | "current unix password" => {
            PamText::CurrentPassword
        }
        "new password" | "new unix password" | "enter new unix password" => PamText::NewPassword,
        "retype new password" | "retype new unix password" => PamText::RetypeNewPassword,
        _ if key.starts_with("you are required to change your password immediately") => {
            PamText::PasswordExpired
        }
        _ if key.starts_with("your account has expired") => PamText::AccountExpired,
        _ => PamText::Other(text.trim().to_string()),
    }
}

impl PamText {
    /// Better OS's words for it, or PAM's own.
    pub fn text(&self, language: Language) -> String {
        let copy = language.copy();
        match self {
            Self::Password => copy.password.to_string(),
            Self::CurrentPassword => copy.current_password.to_string(),
            Self::NewPassword => copy.new_password.to_string(),
            Self::RetypeNewPassword => copy.retype_new_password.to_string(),
            Self::PasswordExpired => copy.password_expired.to_string(),
            Self::AccountExpired => copy.account_expired.to_string(),
            Self::Other(text) => text.clone(),
        }
    }

    /// Whether answering it is part of changing the password.
    pub fn is_password_change(&self) -> bool {
        matches!(
            self,
            Self::CurrentPassword | Self::NewPassword | Self::RetypeNewPassword
        )
    }
}

/// Why PAM refused, as the screen tells it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// A wrong password, and also an unknown user: the screen never says
    /// which accounts exist.
    WrongPassword,
    /// `acct_mgmt` refused an account that has expired.
    AccountExpired,
    /// Any other refusal (`pam_faillock`, an administrator's lock, a module's
    /// own reason): the title, with PAM's own text below it.
    CannotSignIn { pam_text: String },
}

/// Reads greetd's `auth_error` description. greetd reports PAM's
/// `pam_strerror` text, sometimes with the function before it; both forms of
/// the return code's name are matched.
pub fn classify_refusal(description: &str) -> Refusal {
    let lowered = description.to_ascii_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|needle| lowered.contains(needle));
    if any(&[
        "authentication failure",
        "auth_err",
        "user not known",
        "user_unknown",
    ]) {
        Refusal::WrongPassword
    } else if any(&["account has expired", "acct_expired"]) {
        Refusal::AccountExpired
    } else {
        Refusal::CannotSignIn {
            pam_text: description.trim().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_pam_unix_prompts_get_better_os_copy() {
        assert_eq!(classify("Password: "), PamText::Password);
        assert_eq!(classify("Current password: "), PamText::CurrentPassword);
        assert_eq!(
            classify("(current) UNIX password: "),
            PamText::CurrentPassword
        );
        assert_eq!(classify("New password: "), PamText::NewPassword);
        assert_eq!(
            classify("Retype new password: "),
            PamText::RetypeNewPassword
        );
        assert_eq!(
            classify("You are required to change your password immediately (password expired)."),
            PamText::PasswordExpired
        );
        assert_eq!(
            classify(
                "You are required to change your password immediately (administrator enforced)."
            ),
            PamText::PasswordExpired
        );
        assert_eq!(
            classify("Your account has expired; please contact your system administrator."),
            PamText::AccountExpired
        );
        assert_eq!(classify("New password: ").text(Language::ZhTw), "新密碼");
        assert_eq!(
            classify("Retype new password: ").text(Language::EnUs),
            "Type the new password again"
        );
        assert!(classify("Current password: ").is_password_change());
        assert!(!classify("Password: ").is_password_change());
    }

    #[test]
    fn anything_else_is_passed_through() {
        let warning = "Warning: your password will expire in 3 days.";
        assert_eq!(classify(warning), PamText::Other(warning.into()));
        assert_eq!(classify(warning).text(Language::ZhTw), warning);
        assert_eq!(classify("  PIN: ").text(Language::EnUs), "PIN:");
    }

    #[test]
    fn refusals_are_read_from_greetds_description() {
        assert_eq!(
            classify_refusal("Authentication failure"),
            Refusal::WrongPassword
        );
        assert_eq!(
            classify_refusal("pam_authenticate: AUTH_ERR"),
            Refusal::WrongPassword
        );
        assert_eq!(
            classify_refusal("User not known to the underlying authentication module"),
            Refusal::WrongPassword
        );
        assert_eq!(
            classify_refusal("User account has expired"),
            Refusal::AccountExpired
        );
        assert_eq!(
            classify_refusal("Permission denied"),
            Refusal::CannotSignIn {
                pam_text: "Permission denied".into()
            }
        );
    }
}
