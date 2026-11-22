//! The words the supervisor writes: the text login's message (always
//! English, because the Linux console cannot draw Chinese) and what
//! `better-os-login-status` prints (design/login-screen-spec.md section
//! 5.10).

use manager_ipc::boot_watch::{Event, LoginStage, RestartOutcome};

use crate::rules::{Note, Outcome, Phase, Reason, State, Step};

/// The text login's message, with the stage and the time filled in. Both
/// come from closed sets (the stage's English name, a clock time of
/// digits), so agetty finds no escape of its own in it.
pub fn issue(stage: LoginStage, time: &str) -> String {
    format!(
        "The Better OS login screen couldn't be shown ({}, {time}).\n\
         Sign in below with your user name and password to use text mode.\n\
         After signing in, type better-os-login-status to see why.\n\
         To go back to the GNOME login screen, type sudo dpkg-reconfigure gdm3 and choose gdm3.\n\n",
        stage.english()
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    ZhTw,
    English,
}

impl Language {
    /// zh-TW when the locale is Chinese and the terminal can draw it: the
    /// Linux console (`TERM=linux`) cannot.
    pub fn pick(term: Option<&str>, locale: Option<&str>) -> Language {
        let chinese = locale.is_some_and(|l| l.starts_with("zh"));
        if chinese && term.is_some_and(|t| t != "linux") {
            Language::ZhTw
        } else {
            Language::English
        }
    }
}

fn stage_words(language: Language, stage: LoginStage) -> &'static str {
    match language {
        Language::ZhTw => stage.zh_tw(),
        Language::English => stage.english(),
    }
}

/// A journal stage's name in the status: the record's stage names, and the
/// spec's own words for the stages that never end an appearance.
fn step_words(language: Language, step: Step) -> &'static str {
    match (language, step) {
        (Language::ZhTw, Step::DriverLate) => "顯示卡驅動程式較晚載入",
        (Language::English, Step::DriverLate) => "the graphics driver loaded late",
        (Language::ZhTw, Step::Seat) => "座位還不能顯示圖形",
        (Language::English, Step::Seat) => "the seat wasn't ready for graphics",
        (Language::ZhTw, Step::Plymouth) => "開機畫面沒有回應",
        (Language::English, Step::Plymouth) => "the boot splash didn't respond",
        (Language::ZhTw, Step::SessionEnd) => "登入的工作階段結束",
        (Language::English, Step::SessionEnd) => "a signed-in session ended",
        (Language::ZhTw, Step::TextLogin) => "文字模式登入",
        (Language::English, Step::TextLogin) => "the text sign-in",
        (language, step) => stage_words(language, step.stage()),
    }
}

fn reason_words(language: Language, reason: Reason) -> &'static str {
    let (zh, en) = match reason {
        Reason::NoGraphicsDevice => ("找不到顯示卡", "no graphics device"),
        Reason::FirmwareFramebufferOnly => {
            ("只有韌體提供的畫面", "only the firmware's framebuffer")
        }
        Reason::NoScreen => ("沒有接上螢幕", "no screen connected"),
        Reason::ScreenConnected => ("接上了螢幕", "a screen was connected"),
        Reason::RealDriverBound => ("顯示卡驅動程式已載入", "the graphics driver loaded"),
        Reason::SeatNotGraphical => ("座位不能顯示圖形", "the seat can't show graphics"),
        Reason::LogindNoAnswer => ("登入管理服務沒有回應", "logind didn't answer"),
        Reason::PlymouthDidNotReturn => (
            "開機畫面沒有回應，已強制結束",
            "the splash didn't answer and was ended",
        ),
        Reason::NoGreeterSession => (
            "登入畫面的工作階段沒有開啟",
            "the login screen's session didn't open",
        ),
        Reason::GreeterAccountMissing => (
            "登入畫面的帳號不存在",
            "the login screen's account is missing",
        ),
        Reason::NoHeartbeat => ("顯示伺服器沒有啟動", "the display server didn't start"),
        Reason::NotReady => ("登入畫面沒有準備好", "the login screen wasn't ready"),
        Reason::HeartbeatStale => ("登入畫面停止回應", "the login screen stopped responding"),
        Reason::StopDidNotComplete => ("登入畫面無法停止", "the login screen couldn't be stopped"),
        Reason::SessionKilled => (
            "登入畫面的工作階段已強制結束",
            "the login screen's session was ended",
        ),
        Reason::OutOfTime => ("時間用完", "out of time"),
        Reason::SessionsUnknown => (
            "看不出有沒有人登入，所以沒有停止任何東西",
            "who is signed in couldn't be told, so nothing was stopped",
        ),
        Reason::TextLoginNotActive => (
            "文字模式沒有啟動，改用另一個終端機",
            "the text sign-in didn't start, so the other terminal was used",
        ),
        Reason::StateUnreadable => (
            "讀不到登入流程的狀態",
            "the sign-in path's state couldn't be read",
        ),
        Reason::Restarted => (
            "登入監督程式重新啟動過",
            "the login supervisor was restarted",
        ),
        Reason::StartedAgain => (
            "登入管理員重新啟動，重新開始",
            "the display manager was started again, so it began anew",
        ),
    };
    match language {
        Language::ZhTw => zh,
        Language::English => en,
    }
}

fn outcome_words(language: Language, outcome: Outcome) -> &'static str {
    match (language, outcome) {
        (Language::ZhTw, Outcome::Ok) => "繼續",
        (Language::ZhTw, Outcome::Retried) => "重試",
        (Language::ZhTw, Outcome::FellBack) => "改用文字模式",
        (Language::English, Outcome::Ok) => "continued",
        (Language::English, Outcome::Retried) => "retried",
        (Language::English, Outcome::FellBack) => "fell back",
    }
}

/// `{code}. {stage name}: {result} ({reason})`.
pub fn note_line(language: Language, note: &Note) -> String {
    let separator = match language {
        Language::ZhTw => "：",
        Language::English => ": ",
    };
    let (open, close) = match language {
        Language::ZhTw => ("（", "）"),
        Language::English => (" (", ")"),
    };
    format!(
        "{}. {}{separator}{}{open}{}{close}",
        note.step.code(),
        step_words(language, note.step),
        outcome_words(language, note.outcome),
        reason_words(language, note.reason)
    )
}

fn phase_words(language: Language, phase: &Phase) -> String {
    let zh = matches!(language, Language::ZhTw);
    match phase {
        Phase::Ready { .. } => if zh {
            "登入畫面已出現"
        } else {
            "the login screen is showing"
        }
        .to_string(),
        Phase::Session { .. } => if zh {
            "已有人登入"
        } else {
            "someone is signed in"
        }
        .to_string(),
        Phase::Checking { .. } => if zh {
            "正在等待顯示卡、座位和登入管理服務"
        } else {
            "waiting for the graphics device, the seat and the login manager"
        }
        .to_string(),
        Phase::Attempt { number, .. } => if zh {
            "正在啟動登入畫面（第 {n} 次，共 3 次）"
        } else {
            "starting the login screen (attempt {n} of 3)"
        }
        .replace("{n}", &number.to_string()),
        Phase::Stopping { .. } | Phase::FallingBack { .. } => if zh {
            "正在停止登入畫面"
        } else {
            "stopping the login screen"
        }
        .to_string(),
        Phase::TextLogin { tty, .. } => if zh {
            "已改用文字模式登入（{tty}）"
        } else {
            "using the text sign-in ({tty})"
        }
        .replace("{tty}", tty.name()),
        Phase::Unreadable => reason_words(language, Reason::StateUnreadable).to_string(),
    }
}

fn event_words(language: Language, event: &Event) -> Option<String> {
    let zh = matches!(language, Language::ZhTw);
    match &event.restart {
        RestartOutcome::TextLoginUsed { stage, .. } => Some(
            if zh {
                "上次開機時登入畫面無法顯示（{stage}），已改用文字模式登入。"
            } else {
                "The login screen couldn't be shown last time you started up ({stage}), so a text sign-in was used."
            }
            .replace("{stage}", stage_words(language, *stage)),
        ),
        RestartOutcome::LoginScreenRestarted {
            stage, restarts, ..
        } => Some(
            match (zh, *restarts >= 2) {
                (true, false) => "上次開機時登入畫面重新啟動了一次（{stage}）。",
                (true, true) => "上次開機時登入畫面重新啟動了兩次（{stage}）。",
                (false, false) => "The login screen had to be restarted once last time you started up ({stage}).",
                (false, true) => "The login screen had to be restarted twice last time you started up ({stage}).",
            }
            .replace("{stage}", stage_words(language, *stage)),
        ),
        _ => None,
    }
}

/// What `better-os-login-status` prints. `date` turns an event's Unix time
/// into the local calendar time.
pub fn status(
    language: Language,
    state: Option<&State>,
    event: Option<&Event>,
    from_files: bool,
    date: &dyn Fn(u64) -> String,
) -> Vec<String> {
    let zh = matches!(language, Language::ZhTw);
    let mut lines = Vec::new();
    if from_files {
        lines.push(
            if zh {
                "登入監督程式沒有回應，以下直接讀自檔案。"
            } else {
                "The login supervisor didn't answer, so this was read from its files."
            }
            .to_string(),
        );
    }
    match state {
        None => lines.push(
            if zh {
                "這次開機還沒有登入流程的紀錄。"
            } else {
                "Nothing about the sign-in path is recorded this startup."
            }
            .to_string(),
        ),
        Some(state) => {
            lines.push(
                if zh {
                    "Better OS 登入流程，這次開機："
                } else {
                    "Better OS sign-in path, this startup:"
                }
                .to_string(),
            );
            lines.push(
                if zh {
                    "現在：{state}"
                } else {
                    "Now: {state}"
                }
                .replace("{state}", &phase_words(language, &state.phase)),
            );
            if state.notes.is_empty() {
                lines.push(
                    if zh {
                        "每個階段都通過了。"
                    } else {
                        "Every stage passed."
                    }
                    .to_string(),
                );
            } else {
                lines.push(
                    if zh {
                        "沒有通過的階段："
                    } else {
                        "Stages that didn't pass:"
                    }
                    .to_string(),
                );
                for note in &state.notes {
                    lines.push(format!("  {}", note_line(language, note)));
                }
            }
        }
    }
    if let Some(event) = event
        && let Some(words) = event_words(language, event)
    {
        lines.push(
            if zh {
                "最近的紀錄：{date}，{outcome}"
            } else {
                "Last record: {date}, {outcome}"
            }
            .replace("{date}", &date(event.detected_at_unix_seconds))
            .replace("{outcome}", &words),
        );
    }
    lines.push(
        if zh {
            "要改回 GNOME 登入畫面，輸入 sudo dpkg-reconfigure gdm3 並選擇 gdm3。"
        } else {
            "To go back to the GNOME login screen, type sudo dpkg-reconfigure gdm3 and choose gdm3."
        }
        .to_string(),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{STATE_VERSION, TextTty, UnitStart};
    use manager_ipc::boot_watch::{Evidence, LOGIN_SUPERVISOR_UNIT, RECORD_VERSION};

    #[test]
    fn the_message_names_the_stage_in_english_and_has_no_escape_for_agetty() {
        for stage in LoginStage::ALL {
            let text = issue(stage, "08:42");
            assert!(text.starts_with(&format!(
                "The Better OS login screen couldn't be shown ({}, 08:42).\n",
                stage.english()
            )));
            assert!(!text.contains('\\'), "{text}");
            assert!(text.is_ascii(), "the console cannot draw it: {text}");
        }
        assert!(issue(LoginStage::DisplayServer, "08:42").contains("sudo dpkg-reconfigure gdm3"));
    }

    #[test]
    fn chinese_is_printed_only_where_the_terminal_can_draw_it() {
        assert_eq!(
            Language::pick(Some("xterm-256color"), Some("zh_TW.UTF-8")),
            Language::ZhTw
        );
        assert_eq!(
            Language::pick(Some("linux"), Some("zh_TW.UTF-8")),
            Language::English
        );
        assert_eq!(Language::pick(None, Some("zh_TW.UTF-8")), Language::English);
        assert_eq!(
            Language::pick(Some("xterm"), Some("en_US.UTF-8")),
            Language::English
        );
    }

    #[test]
    fn the_status_lists_the_stages_that_did_not_pass_and_the_last_record() {
        let mut state = State::fresh(None, UnitStart::default(), 0);
        assert_eq!(state.version, STATE_VERSION);
        state.notes.push(Note {
            step: Step::Compositor,
            outcome: Outcome::Retried,
            reason: Reason::NoHeartbeat,
        });
        state.phase = Phase::TextLogin {
            step: Step::Compositor,
            tty: TextTty::Tty8,
            started: 70,
            starts: 1,
            confirmed: true,
        };
        let event = Event {
            version: RECORD_VERSION,
            boot_id: "0".repeat(32),
            detected_at_unix_seconds: 7,
            display_manager: LOGIN_SUPERVISOR_UNIT.to_string(),
            waited_seconds: 70,
            evidence: Evidence::default(),
            restart: RestartOutcome::TextLoginUsed {
                stage: LoginStage::DisplayServer,
                tty: "tty8".to_string(),
                after_seconds: 70,
            },
        };
        let date = |_| "2026-11-22 08:42".to_string();
        let zh = status(Language::ZhTw, Some(&state), Some(&event), false, &date);
        assert_eq!(
            zh,
            [
                "Better OS 登入流程，這次開機：",
                "現在：已改用文字模式登入（tty8）",
                "沒有通過的階段：",
                "  6. 顯示伺服器無法啟動：重試（顯示伺服器沒有啟動）",
                "最近的紀錄：2026-11-22 08:42，上次開機時登入畫面無法顯示（顯示伺服器無法啟動），已改用文字模式登入。",
                "要改回 GNOME 登入畫面，輸入 sudo dpkg-reconfigure gdm3 並選擇 gdm3。",
            ]
        );
        let en = status(Language::English, None, None, true, &date);
        assert_eq!(
            en,
            [
                "The login supervisor didn't answer, so this was read from its files.",
                "Nothing about the sign-in path is recorded this startup.",
                "To go back to the GNOME login screen, type sudo dpkg-reconfigure gdm3 and choose gdm3.",
            ]
        );
    }
}
