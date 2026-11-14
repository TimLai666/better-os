# Ticket 111 — The Updates page says what each part means

**Blocked by:** none · **Status:** open

## Why

On 2026-11-14 the owner found the Better Manager Updates page unclear
(screenshot in Traditional Chinese, 0.2.12 with eight 0.2.13 updates):

- A card shows a big "8" with "等待確認" (Ready to review) and a "確認變更"
  (Review changes) button. It does not say eight *components have updates*,
  and the button actually prepares updating all of them
  (`prepare_update_all`, `crates/manager-gui/src/pages_main.rs` ~663-698).
- The catalog card reads "剛剛下載的清單 · 剛剛更新" ("catalog downloaded just
  now · updated moments ago"): the same fact twice, and "清單" alone does not
  say it is the list of available versions.
- Each component shows "重新啟動需求: 需重新啟動應用程式" (Restart
  requirement: Restart the application), which reads like a form label and,
  since ticket 105, is no longer something the person has to do: Better
  Manager restarts it after the update.
- The "更新" header card and the "檢查更新" button repeat the page title.

## What to build

- Rewrite the Updates page copy in English and Traditional Chinese (Taiwan
  usage, no machine-translation tone) so each card says what it is and what
  its button does, for example "8 個元件有新版本" with "全部更新"; one line
  for when the version list was last checked; restart text that says what
  will happen ("更新後會自動重新開啟") and only when it matters.
- Keep the flow (review before applying) if it adds value; if the review step
  stays, the button names the result ("檢視並全部更新"), not an abstract
  "confirm changes".
- Check the other Better Manager pages for the same problems (labels that
  are field names, counts without a noun, repeated facts) and fix them.
- Screens follow DESIGN.md; no layout regressions at compact width.

## Acceptance

- [ ] Before and after screenshots of the Updates page with pending updates,
      with none, and while checking, in both languages, from a headless
      sandbox with a fixture catalog.
- [ ] The i18n tests cover every changed string in both languages.
- [ ] fmt, clippy on stable and 1.99, `cargo +1.95 check`, workspace tests and
      the license check pass.
