# Ticket 111 — The Updates page says what each part means

**Blocked by:** none · **Status:** merged, not released

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

## What was built

- **One summary card.** The "更新" card that repeated the page title, the
  catalog card with its second refresh button, and the "8 / 等待確認 / 確認變更"
  card are one card: "8 個元件有新版本" ("8 components have new versions"),
  the line 「7 分鐘前檢查過新版本」 under it, and the buttons 「再檢查一次」
  and 「檢視並全部更新」 ("Review and update all"). The review step stays: the
  plan shows what each update downloads and restores before anything runs,
  and the button now names that. With nothing to update the card says
  「所有元件都是最新版本」 with one 「檢查更新」 button, and the separate
  empty state that said it again is gone. While checking: 「正在檢查有沒有新版本…」
  and a disabled 「正在檢查…」. A warning about the version list (offline,
  partly refused) and a failed check go on the same card, said once.
- **The count at launch.** The refresh at launch is a check too, so the card
  shows its count rather than the page title before a click
  (`UpdateCheck::NotRun` is gone). Only a check the person asked for is
  reported as failed; after a failed launch refresh the count stands with the
  list's warning under it.
- **Page subtitle.** The Updates page had the Components subtitle ("Install,
  update, disable…"); it now says 「這台電腦上 Better OS 元件的新版本。」
- **Version list line.** "剛剛下載的清單 · 剛剛更新" is one line,
  「剛剛檢查過新版本」 / "Checked for new versions moments ago", with the
  singular in English ("a minute ago"). Where the list came from is said only
  when it matters, by the existing warnings, which now say 「版本清單」 and
  「下載不到最新的清單」 instead of "已發佈的清單". The Apps page's copy of
  the line follows, and its 「更新清單」 button is 「檢查更新」.
- **Restart text.** On a card it is a note, shown only when an update is
  waiting and something happens: 「更新後會自動重新開啟」 (ticket 105 reopens
  it), 「更新後需要登出再登入」, 「更新後需要重新開機」; nothing for no
  restart or an undeclared one. In detail and review tables the field
  「重新啟動需求」 is 「完成後」 / "Afterwards" with values such as
  「會自動重新開啟」, which also reads right beside Default apps' 「立即生效」.
- **Other pages.** Home's attention card and the Apps page's Updates link
  said 「有 8 個更新」; both say 「8 個元件有新版本」. The catalog warning on
  Home says 「版本清單可能不是最新的」 with 「檢查更新」. The restore page with
  nothing to restore said "A component did not start correctly" over the
  Updates page's "found no newer compatible versions"; it now says
  「還原元件」 and 「目前沒有元件檢查失敗，不需要還原。」
- **Tests.** `the_updates_page_says_what_each_part_means` pins every changed
  string in both languages; the count, the line's age (both languages,
  singular), the launch count, and the restart note have their own tests;
  the attention and Apps-link tests assert the new counts.
- **Screenshots** (`.shots/`, not committed; demo mode with the published
  0.2.13 catalog saved as the fixture cache, eight components at 0.2.12):
  `before111-updates-{pending,none,checking}-{en,zh}.png` and
  `after111-…` for the same states; `after111-updates-pending-compact-*`
  at the minimum window width; `before111-{home,apps}-pending-zh.png`,
  `after111-{home,apps}-pending-zh.png`, and
  `after111-component-detail-zh.png`. "Checking" points the catalog URL at a
  non-routable address so the fetch stays in flight.
- **After review.**
  - The restart wording depends on the operation. `manager-core` attaches the
    manifest's requirement to every operation, but only an update reopens a
    running app by itself (ticket 105). An update says 「開著的話會自動重新開啟」;
    installing, enabling, or restoring says 「下次開啟時生效」; disabling or
    removing an app that only needed restarting shows no row. Logging out and
    restarting the computer are shown for every operation. The card's note
    reads 「更新後，開著的話會自動重新開啟」, and the detail page shows the
    row only while an update is waiting. The review's summary row uses the
    update wording only when every step is an update.
  - An offline first launch (or a failed refresh) with nothing to update says
    「目前的版本清單裡沒有新版本」, not 「所有元件都是最新版本」, which is kept for a
    list fetched in full or in part.
  - English says "1 component entry failed its check" in the singular.
  - The Updates and Apps pages redraw once a minute, so 「剛剛檢查過新版本」
    becomes 「5 分鐘前…」 while the window sits open.

