//! Entry 1: blank text from a damaged fontconfig cache.
//!
//! Seen on the owner's machine on 2026-11-11 (ticket 76): every label in an
//! Electron application was blank, and every font query was answered with
//! one file and an unknown style. `fc-cache -f` rebuilt the person's cache
//! and the text came back.
//!
//! Detection asks fontconfig for several families and styles and calls it
//! the fault only when every answer is the same file and none has a style.
//! One file with a known style is a machine with one font, not a damaged
//! cache. The fix rebuilds the person's own cache; it removes nothing they
//! made, so there is nothing to undo.

use crate::command::{Command, FontQuery};
use crate::entry::{
    Answer, Answers, CannotCheck, Change, ChangeLine, CheckOutcome, Description, Detection, Entry,
    EntryId, Finding, FindingDetail, Plan, PlanError, Read, ReadError, Subject, UndoKind, UndoPlan,
};
use crate::record::{AppliedRecord, Recorded};

pub struct FontCache;

/// One `fc-match` answer: the file and its first style name.
pub fn parse_answer(output: &str) -> Option<(String, String)> {
    let line = output.lines().next()?;
    let (file, style) = line.split_once('\t').unwrap_or((line, ""));
    let file = file.trim();
    (!file.is_empty()).then(|| (file.to_string(), style.trim().to_string()))
}

/// A style fontconfig could not name.
pub fn style_unknown(style: &str) -> bool {
    let style = style.trim();
    style.is_empty() || style.eq_ignore_ascii_case("unknown")
}

fn reads() -> Vec<Read> {
    FontQuery::ALL
        .iter()
        .map(|query| Read::Command(Command::MatchFont(*query)))
        .collect()
}

/// The one file every query was answered with, when the cache is damaged;
/// `None` when it answers like a healthy one.
fn damaged(answers: &Answers) -> Result<Option<String>, CannotCheck> {
    let mut seen = Vec::new();
    for query in FontQuery::ALL {
        let answer = answers.get(&Read::Command(Command::MatchFont(query)));
        let (file, style) = match answer {
            Some(Answer::Output(Ok(text))) => parse_answer(text).ok_or_else(|| {
                CannotCheck::FontQueryFailed(format!("{}: no answer", query.pattern()))
            })?,
            Some(Answer::Output(Err(ReadError::NotInstalled(program)))) => {
                return Err(CannotCheck::ProgramMissing(program));
            }
            Some(Answer::Output(Err(error))) => {
                return Err(CannotCheck::FontQueryFailed(format!(
                    "{}: {error}",
                    query.pattern()
                )));
            }
            _ => return Err(crate::engine::not_answered()),
        };
        seen.push((file, style));
    }
    let first = &seen[0].0;
    let fault = seen
        .iter()
        .all(|(file, style)| file == first && style_unknown(style));
    Ok(fault.then(|| first.clone()))
}

impl Entry for FontCache {
    fn id(&self) -> EntryId {
        EntryId::FontconfigCache
    }

    fn undo_kind(&self) -> UndoKind {
        UndoKind::NotNeeded
    }

    fn detection_reads(&self) -> Vec<Read> {
        reads()
    }

    fn detect(&self, answers: &Answers) -> Detection {
        match damaged(answers) {
            Err(reason) => Detection::CannotCheck(reason),
            Ok(None) => Detection::Checked(Vec::new()),
            Ok(Some(file)) => Detection::Checked(vec![Finding {
                subject: Subject::Machine,
                detail: FindingDetail::FontCache { file },
            }]),
        }
    }

    fn describe(&self, _: &Subject) -> Description {
        Description {
            change: vec![ChangeLine::Runs(Command::RebuildFontCache)],
            undo: UndoKind::NotNeeded,
            limits: Vec::new(),
        }
    }

    fn plan_reads(&self, _: &Finding) -> Vec<Read> {
        Vec::new()
    }

    fn plan(&self, finding: &Finding, _: &Answers) -> Result<Plan, PlanError> {
        if !matches!(finding.detail, FindingDetail::FontCache { .. }) {
            return Err(PlanError::NotThisEntry);
        }
        Ok(Plan {
            changes: vec![Change::Run(Command::RebuildFontCache)],
            record: Recorded::RebuiltFontCache,
        })
    }

    fn check_reads(&self, _: &AppliedRecord) -> Vec<Read> {
        reads()
    }

    fn check(&self, _: &AppliedRecord, answers: &Answers) -> CheckOutcome {
        match damaged(answers) {
            Ok(None) => CheckOutcome::Passed,
            Ok(Some(_)) => CheckOutcome::StillPresent,
            Err(reason) => CheckOutcome::CannotCheck(reason),
        }
    }

    fn undo_reads(&self, _: &AppliedRecord) -> Vec<Read> {
        Vec::new()
    }

    /// A rebuilt cache is not something to put back: the damaged one is
    /// gone and nothing the person made was in it.
    fn plan_undo(&self, _: &AppliedRecord, _: &Answers, _: bool) -> Result<UndoPlan, PlanError> {
        Ok(UndoPlan {
            changes: Vec::new(),
            remaining: None,
            skipped: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{self, fake::FakeHost};
    use crate::record::Records;
    use crate::state::EntryState;

    /// What the damaged cache answered on the owner's machine: one file and
    /// no style for everything.
    fn damaged_answers() -> Answers {
        let mut answers = Answers::new();
        for query in FontQuery::ALL {
            answers.insert(
                Read::Command(Command::MatchFont(query)),
                Answer::Output(Ok(
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf\t\n".into()
                )),
            );
        }
        answers
    }

    /// What a healthy cache answers, read on the owner's machine on
    /// 2026-11-21 from a fresh scratch cache.
    fn healthy_answers() -> Answers {
        let files = [
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "Regular",
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
                "Regular",
            ),
            (
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                "Book",
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
                "Bold",
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
                "Regular",
            ),
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "Regular",
            ),
        ];
        let mut answers = Answers::new();
        for (query, (file, style)) in FontQuery::ALL.iter().zip(files) {
            answers.insert(
                Read::Command(Command::MatchFont(*query)),
                Answer::Output(Ok(format!("{file}\t{style}\n"))),
            );
        }
        answers
    }

    #[test]
    fn answers_are_read_as_a_file_and_a_style() {
        assert_eq!(
            parse_answer("/a.ttf\tBold\n"),
            Some(("/a.ttf".into(), "Bold".into()))
        );
        assert_eq!(
            parse_answer("/a.ttf\t\n"),
            Some(("/a.ttf".into(), String::new()))
        );
        assert_eq!(
            parse_answer("/a.ttf"),
            Some(("/a.ttf".into(), String::new()))
        );
        assert_eq!(parse_answer(""), None);
        assert_eq!(parse_answer("\tBold"), None);
        assert!(style_unknown(""));
        assert!(style_unknown("Unknown"));
        assert!(!style_unknown("Regular"));
    }

    #[test]
    fn one_file_with_no_style_for_every_query_is_the_fault() {
        let Detection::Checked(found) = FontCache.detect(&damaged_answers()) else {
            panic!("it could check");
        };
        assert_eq!(
            found,
            [Finding {
                subject: Subject::Machine,
                detail: FindingDetail::FontCache {
                    file: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()
                },
            }]
        );
    }

    #[test]
    fn different_answers_are_a_healthy_cache() {
        assert_eq!(
            FontCache.detect(&healthy_answers()),
            Detection::Checked(Vec::new())
        );
    }

    #[test]
    fn a_machine_with_one_font_is_not_a_damaged_cache() {
        let mut answers = Answers::new();
        for query in FontQuery::ALL {
            answers.insert(
                Read::Command(Command::MatchFont(query)),
                Answer::Output(Ok("/usr/share/fonts/DejaVuSans.ttf\tBook\n".into())),
            );
        }
        assert_eq!(FontCache.detect(&answers), Detection::Checked(Vec::new()));
    }

    #[test]
    fn a_missing_fontconfig_or_a_failed_query_is_cannot_check() {
        let mut answers = healthy_answers();
        answers.insert(
            Read::Command(Command::MatchFont(FontQuery::Serif)),
            Answer::Output(Err(ReadError::NotInstalled(crate::command::FC_MATCH))),
        );
        assert_eq!(
            FontCache.detect(&answers),
            Detection::CannotCheck(CannotCheck::ProgramMissing("/usr/bin/fc-match"))
        );
        answers.insert(
            Read::Command(Command::MatchFont(FontQuery::Serif)),
            Answer::Output(Err(ReadError::TimedOut)),
        );
        assert!(matches!(
            FontCache.detect(&answers),
            Detection::CannotCheck(CannotCheck::FontQueryFailed(_))
        ));
        assert_eq!(
            FontCache.detect(&Answers::new()),
            Detection::CannotCheck(CannotCheck::NotAnswered)
        );
    }

    #[test]
    fn the_fix_rebuilds_the_cache_and_the_check_sees_different_answers() {
        let mut host = FakeHost {
            answers: damaged_answers(),
            after_change: Some(Box::new(|change, answers| {
                assert_eq!(change, &Change::Run(Command::RebuildFontCache));
                *answers = healthy_answers();
            })),
            ..FakeHost::default()
        };
        let report = engine::check(&FontCache, &mut host, &Records::default(), &[]);
        assert_eq!(report.instances[0].state, EntryState::Found);
        let finding = report.instances[0].finding.clone().unwrap();
        let outcome = engine::fix(&FontCache, &finding, &mut host, 100).unwrap();
        assert_eq!(host.changes, [Change::Run(Command::RebuildFontCache)]);
        assert_eq!(outcome.check, CheckOutcome::Passed);
        assert_eq!(outcome.record.change, Recorded::RebuiltFontCache);
        assert_eq!(outcome.record.applied_at, 100);
    }

    #[test]
    fn a_rebuild_that_does_not_help_is_applied_but_still_present() {
        let mut host = FakeHost {
            answers: damaged_answers(),
            ..FakeHost::default()
        };
        let finding = Finding {
            subject: Subject::Machine,
            detail: FindingDetail::FontCache { file: "/x".into() },
        };
        let outcome = engine::fix(&FontCache, &finding, &mut host, 1).unwrap();
        assert_eq!(outcome.check, CheckOutcome::StillPresent);
    }

    #[test]
    fn a_failed_rebuild_records_nothing() {
        let mut host = FakeHost {
            answers: damaged_answers(),
            fail_changes: Some(crate::entry::ChangeError::NotInstalled(
                crate::command::FC_CACHE,
            )),
            ..FakeHost::default()
        };
        let finding = Finding {
            subject: Subject::Machine,
            detail: FindingDetail::FontCache { file: "/x".into() },
        };
        assert_eq!(
            engine::fix(&FontCache, &finding, &mut host, 1),
            Err(engine::FixError::Change {
                error: crate::entry::ChangeError::NotInstalled("/usr/bin/fc-cache"),
                applied: None,
            })
        );
    }

    #[test]
    fn there_is_nothing_to_undo_and_it_says_so() {
        assert_eq!(FontCache.undo_kind(), UndoKind::NotNeeded);
        let record = AppliedRecord {
            entry: EntryId::FontconfigCache,
            subject: Subject::Machine,
            applied_at: 1,
            change: Recorded::RebuiltFontCache,
        };
        let mut host = FakeHost::default();
        let outcome = engine::undo(&FontCache, &record, &mut host, false).unwrap();
        assert!(host.changes.is_empty());
        assert_eq!(outcome.remaining, None);
        assert_eq!(
            FontCache.describe(&Subject::Machine).change,
            [ChangeLine::Runs(Command::RebuildFontCache)]
        );
    }

    #[test]
    fn it_refuses_another_entry_s_finding() {
        let finding = Finding {
            subject: Subject::Machine,
            detail: FindingDetail::ElectronCursor {
                name: Default::default(),
                scales: vec![2.0],
                electron: None,
                offer: crate::entry::Offer::Fix,
            },
        };
        assert_eq!(
            FontCache.plan(&finding, &Answers::new()),
            Err(PlanError::NotThisEntry)
        );
    }
}
