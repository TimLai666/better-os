# Ticket 258 — A media seek asked for before the file has opened is kept

**Blocked by:** none

## Why

Found on 2026-11-21: `a_seek_lands_where_asked_even_between_key_frames`
(`crates/files-preview/tests/media.rs`) failed in CI's `rust` job on two
commits of pull request #26, which does not touch `files-preview`, each time
in one of the two runs of the same commit (run 44827879397 attempt 1, job
136605005935; run 44857046739 attempt 1, job 136701576052). Both failures
waited out the test's 60-second deadline: the first found no position at all
(`Option::unwrap()` on `None` at line 296), the second found the player still
at zero (`seek landed at 0ns`, line 297). Neither printed a GStreamer error.
Ticket 93's follow-up commit `521cdcd` had already raised that deadline from
ten seconds to sixty for the same failure on main, on the belief that an
exact seek through a single key frame is slow; decoding twenty 160x120 VP8
frames takes milliseconds, and the failure survived the longer wait.

The cause is in the product, not the runner. `Player::open` sets the
pipeline to paused, which GStreamer finishes asynchronously, and the window
(`crates/files-gui/src/bin/preview.rs`, the seek slider is enabled when
`duration()` is known) and the test both took a known length as "the file is
open". It is not: matroska and WAV demuxers answer the duration query from
the header while the pipeline is still going from ready to paused, before
decodebin has linked the decoder to playsink's sinks. A flushing seek sent in
that window is refused (`seek_simple` returns an error, which `Player::seek`
ignored), and the file stays at zero. The run that found no position at all
is read as the same seek landing while the video chain was half linked, so
the pipeline never finished prerolling and no sink could answer the position
query; that reading is inferred from the symptom, not traced. Whether the
seek survives depends on how far the linking got by the time the 20 ms poll
first saw the length, so the failure came and went with the scheduling of
the runner.

The two CI panics near the second failure, `better-ui-gtk-frame` at
`crates/better-ui/src/system_frame/service.rs:672` and an unnamed thread at
`crates/files-gui/src/restart.rs:287`, are the deliberate panics of passing
tests (`a_renderer_that_panics_gives_up_instead_of_leaving_windows_waiting`
and `a_send_is_counted_for_the_whole_process_until_its_thread_ends`), listed
under "successes" and printed only because CI runs with `--show-output`.

## What to build

- `Player::seek` before the pipeline's own `ASYNC_DONE` keeps the target;
  `Player::poll` sends it when the pipeline posts that message. A state
  change that finished at once (no `ASYNC_DONE` follows) counts as open. An
  error drops a kept seek.
- A regression test that seeks straight after `Player::open` and fails
  without the fix every time.
- The existing seek tests go back to the ordinary ten-second wait, with the
  wrong explanation removed, and say on failure which events the player saw.

## Done when

- The regression test fails on the old `Player` in every run and passes on
  the new one in every run.
- The whole `media` test binary, run in a loop under the conditions that
  reproduced the CI failure locally, has no failure in more runs than the
  measured failure rate needs.

## Notes

- Evidence, on this machine (GStreamer 1.24.2, the version Ubuntu 24.04 and
  CI install):
  - A probe that opened the test's webm in `playbin` and queried the length
    every 20 ms found it known while the pipeline was still `Ready` with
    `Paused` pending in 30 of 30 runs, and in 100 of 100 pinned to one busy
    CPU. A seek sent then landed in all of those runs: the window in which
    it is lost is narrower than the poll.
  - The same probe seeking 0 µs and 200 µs after `set_state(Paused)`:
    `seek_simple` failed and the position stayed at zero in 40 of 40 runs
    each.
  - The unchanged test binary, run whole (the eight tests in parallel, as in
    CI) under `taskset -c 0,1`, failed this test 1 time in 100 with exactly
    the first CI symptom (`None` after 60 s). With `GST_DEBUG=3` and a probe
    loading the same two CPUs it failed 3 times in about 55 runs: once
    `None`, twice `seek landed at 0ns`, with no error message on the bus.
- The fix (`crates/files-preview/src/media.rs`): `Player` records whether
  its pipeline has opened, `seek` keeps a target asked for before that, and
  `poll` sends it on the pipeline's own `ASYNC_DONE`. The window needs no
  change: it already polls every frame and enables its seek bar on a known
  length, which is now safe.
- Proof: `a_seek_asked_before_the_file_has_opened_is_made_once_it_has` on
  the old `Player` failed 20 of 20 runs (`seek landed at 0ns; events []`,
  the second CI symptom), and on the new one passed 200 of 200. The whole
  `media` binary under `taskset -c 0,1` with two busy loops pinned to the
  same CPUs passed 400 of 400 runs; at the 1-in-100 rate measured without
  load the chance of that by luck is under 2 %, and at the 3-in-55 rate
  measured with load it is nil.
- The seek tests are back on the ordinary ten-second wait. `521cdcd`'s
  60-second wait and its explanation are gone, since what it waited for was
  a seek that had been lost.
- Other tests checked for the same shape. `pause_holds_the_position_and_seek_moves_it`
  seeks a WAV after waiting for its length, the same race; the fix covers
  it, and it now prints the events when it fails. Its 300 ms play and pause
  sleeps only need the position to move forward and then hold still. Nothing
  in that has failed in any of these runs. `a_video_shows_its_first_frame_paused_and_new_frames_while_playing`
  seeks only after the end of the file, once the file is open.
  `media_missing_codec.rs` waits on bus events with a deadline. No other
  crate links GStreamer. Thumbnails (`files-preview/src/thumbnails.rs`) and
  the viewer's converter (`viewer.rs`) are timed processes: their tests
  assert that a 300 ms timeout stops a `sleep 30` or `sleep 60` within five
  seconds, which is far from the limit, and there is no media assertion in
  them.
- Hypotheses ruled out: a tolerance smaller than a frame (the clip is ten
  frames a second, the assertion allows 100 ms either side, and the failures
  were at zero or had no position, not near two seconds); an encoder with a
  different key-frame layout (a key-frame seek lands at zero, but so does a
  dropped seek, and the regression test fails the same way with the clip
  unchanged, while the fixed player lands at 2.0 s on it); a decoder without
  accurate seeking (every successful run reports exactly 2.000 s); a slow
  decode (60 s did not help and a passing run takes about 0.35 s). Other
  tests in the same binary matter only as load: they change when the poll
  first sees the length, and nothing in GStreamer's global state is shared
  between the players.
