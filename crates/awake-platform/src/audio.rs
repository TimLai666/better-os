//! Audio playback, from `/proc/asound`.
//!
//! # What this can and cannot tell you
//!
//! Issue #13 lists audio playback as a trigger and says an undetectable provider
//! must report itself unavailable rather than offer an inert control. Audio is
//! the one on that list where the honest answer is "partly", so it is worth
//! being exact about which part.
//!
//! ALSA publishes one `status` file per playback substream. It reads `closed`
//! when nothing has the device open, and `state: RUNNING` when a stream is
//! actually moving samples. Every audio server on a modern desktop — PipeWire,
//! PulseAudio, and applications using ALSA directly — ends up opening an ALSA
//! device, so this reads real playback rather than a guess. It needs no daemon,
//! no new dependency, and no session bus call.
//!
//! Three limits are real and are not papered over:
//!
//! - An audio server holds the device open for a short suspend timeout after a
//!   stream stops, so "playing" can lag the truth by a few seconds downward.
//!   That direction is the safe one for a keep-awake rule.
//! - Bluetooth and network sinks do not appear under `/proc/asound` at all.
//!   Music over Bluetooth headphones is not detected by this provider.
//! - A kernel without `CONFIG_SND_PROC_FS`, or a container with no `/proc/asound`
//!   mounted, has nothing to read. That reports unavailable, naming the path.
//!
//! The second limit is why this provider is not the whole answer to "is audio
//! playing". A session-bus media-player adapter would cover it, and that is
//! Phase 4's richer desktop integration, not this ticket's.

use std::path::PathBuf;

use awake_core::{Observations, ProviderKind};

use crate::provider::{AUDIO_POLL_SECONDS, Cadence, TriggerProvider};
use crate::roots::{ReadError, Roots, list_dir, read_text};

#[derive(Clone, Debug)]
pub struct AudioProvider {
    roots: Roots,
}

impl AudioProvider {
    pub fn new(roots: Roots) -> Self {
        Self { roots }
    }

    fn asound_dir(&self) -> PathBuf {
        self.roots.proc_path("asound")
    }

    /// Whether any ALSA playback substream is running.
    ///
    /// Returns `Ok(None)` when `/proc/asound` exists but holds no playback
    /// substream, which is a machine with no sound card rather than a machine
    /// that is silent.
    pub fn playing(&self) -> Result<Option<bool>, ReadError> {
        let cards = list_dir(&self.asound_dir())?;
        let mut found_a_substream = false;
        let mut playing = false;

        for card in cards {
            let Some(card_name) = card.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !card_name.starts_with("card") {
                continue;
            }
            let Ok(pcms) = list_dir(&card) else { continue };
            for pcm in pcms {
                let Some(pcm_name) = pcm.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                // `pcm0p` is playback, `pcm0c` is capture. A microphone being
                // open is not a reason to keep the screen on for a video.
                if !pcm_name.starts_with("pcm") || !pcm_name.ends_with('p') {
                    continue;
                }
                let Ok(substreams) = list_dir(&pcm) else {
                    continue;
                };
                for substream in substreams {
                    let status = substream.join("status");
                    let Ok(text) = read_text(&status) else {
                        continue;
                    };
                    found_a_substream = true;
                    // `closed` when idle; `state: RUNNING` when moving samples.
                    // `SETUP`, `PREPARED`, and `XRUN` are open but not playing,
                    // so matching on RUNNING rather than on "not closed" is what
                    // keeps a paused video from holding the machine awake.
                    if text.lines().any(|line| {
                        line.trim().strip_prefix("state:").map(str::trim) == Some("RUNNING")
                    }) {
                        playing = true;
                    }
                }
            }
        }

        Ok(found_a_substream.then_some(playing))
    }
}

impl TriggerProvider for AudioProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::AudioPlayback
    }

    fn cadence(&self) -> Cadence {
        Cadence::Poll {
            seconds: AUDIO_POLL_SECONDS,
        }
    }

    fn sample(&mut self, _now_unix_seconds: u64, into: &mut Observations) {
        match self.playing() {
            Err(error) => into.mark_unavailable(ProviderKind::AudioPlayback, error.explanation()),
            Ok(None) => into.mark_unavailable(
                ProviderKind::AudioPlayback,
                "awake.provider.no_alsa_playback_device",
            ),
            Ok(Some(playing)) => {
                into.audio_playing = Some(playing);
                into.mark_available(ProviderKind::AudioPlayback);
            }
        }
    }
}

/// Builds a fake ALSA playback substream.
#[cfg(any(test, feature = "test-support"))]
pub fn write_substream(proc_dir: &std::path::Path, card: &str, pcm: &str, status: &str) {
    let directory = proc_dir.join("asound").join(card).join(pcm).join("sub0");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("status"), status).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: &str = "state: RUNNING\nowner_pid   : 3312\ntrigger_time: 100.1\n";
    const PREPARED: &str = "state: PREPARED\nowner_pid   : 3312\n";

    fn fixture(substreams: &[(&str, &str, &str)]) -> (tempfile::TempDir, Roots) {
        let directory = tempfile::tempdir().unwrap();
        let proc = directory.path().join("proc");
        std::fs::create_dir_all(proc.join("asound")).unwrap();
        for (card, pcm, status) in substreams {
            write_substream(&proc, card, pcm, status);
        }
        let roots = Roots::at(directory.path());
        (directory, roots)
    }

    fn playing(roots: &Roots) -> Option<bool> {
        let mut observations = Observations::at(1_000);
        AudioProvider::new(roots.clone()).sample(1_000, &mut observations);
        observations.audio_playing
    }

    #[test]
    fn a_running_playback_substream_is_audio_playing() {
        let (_directory, roots) = fixture(&[("card1", "pcm0p", RUNNING)]);
        assert_eq!(playing(&roots), Some(true));
    }

    #[test]
    fn a_closed_device_is_silence() {
        let (_directory, roots) = fixture(&[("card1", "pcm0p", "closed\n")]);
        assert_eq!(playing(&roots), Some(false));
    }

    #[test]
    fn a_paused_stream_that_is_open_but_not_running_is_not_playing() {
        let (_directory, roots) = fixture(&[("card1", "pcm0p", PREPARED)]);
        assert_eq!(
            playing(&roots),
            Some(false),
            "matching on anything but closed would keep the machine awake for a paused video"
        );
    }

    #[test]
    fn a_microphone_being_open_is_not_playback() {
        let (_directory, roots) =
            fixture(&[("card1", "pcm0c", RUNNING), ("card1", "pcm0p", "closed\n")]);
        assert_eq!(playing(&roots), Some(false));
    }

    #[test]
    fn one_running_stream_among_several_cards_is_enough() {
        let (_directory, roots) = fixture(&[
            ("card0", "pcm0p", "closed\n"),
            ("card1", "pcm0p", "closed\n"),
            ("card1", "pcm1p", RUNNING),
        ]);
        assert_eq!(playing(&roots), Some(true));
    }

    #[test]
    fn a_machine_with_no_sound_card_is_unavailable_rather_than_silent() {
        let (_directory, roots) = fixture(&[]);
        let mut observations = Observations::at(1_000);
        AudioProvider::new(roots).sample(1_000, &mut observations);
        assert_eq!(observations.audio_playing, None);
        assert_eq!(
            observations
                .availability_of(ProviderKind::AudioPlayback)
                .explanation(),
            Some("awake.provider.no_alsa_playback_device"),
            "a machine with no card cannot report silence any more than it can report sound"
        );
    }

    #[test]
    fn a_kernel_with_no_proc_asound_names_the_path_it_looked_for() {
        let directory = tempfile::tempdir().unwrap();
        let mut observations = Observations::at(1_000);
        AudioProvider::new(Roots::at(directory.path())).sample(1_000, &mut observations);
        let explanation = observations
            .availability_of(ProviderKind::AudioPlayback)
            .explanation()
            .unwrap()
            .to_string();
        assert!(explanation.contains("asound"), "{explanation}");
    }

    #[test]
    fn the_non_card_entries_under_proc_asound_are_not_walked() {
        let directory = tempfile::tempdir().unwrap();
        let proc = directory.path().join("proc");
        std::fs::create_dir_all(proc.join("asound/oss")).unwrap();
        std::fs::write(proc.join("asound/cards"), b" 0 [Generic]\n").unwrap();
        write_substream(&proc, "card1", "pcm0p", RUNNING);

        assert_eq!(playing(&Roots::at(directory.path())), Some(true));
    }
}
