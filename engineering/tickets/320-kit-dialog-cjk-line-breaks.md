# Ticket 320 — A dialog's zh-TW sentence never leaves a lone "。" on a line

**Blocked by:** none

## Why

Found on 2026-11-22 while taking ticket 272's screenshots. In Better
Monitor's zh-TW Force stop confirmation for an application, the body
"會立刻停止它的 3 個處理程序，不給它們儲存的機會，還沒儲存的內容都會遺失。"
wraps so that the closing "。" stands alone on the second line
(`better-os-evidence/272/round2/zh-TW-light-3-application-confirmation-count.png`,
`zh-TW-dark-3-application-confirmation-count.png`). Chinese typesetting never
starts a line with closing punctuation (kinsoku: "。", "，", "、", "」", "？",
"！" and the like stay with the character before them). The dialog's body is
drawn by `better_ui::fluent::dialog::ContentDialog` for every window, so every
window's zh-TW dialog can do it, not only Better Monitor's.

## What to build

- Line breaking in the kit's text (at least `ContentDialog`'s body, and
  wherever else the kit wraps prose) keeps CJK closing punctuation on the
  line before it, as WinUI and GTK do; find where GPUI's wrapping decides
  break opportunities and whether the pinned fork can take the fix (ADR
  0043) or the kit must avoid the break itself.

## Done when

- A test lays out a zh-TW sentence at a width that used to leave "。"
  alone and finds it on the previous line; seen in a sandbox screenshot of
  the same dialog, light and dark.
