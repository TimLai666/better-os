# Better Input

A Zhuyin input method that needs no switching between Chinese and English,
chooses words from what was written before, and takes dictation into the same
composition, all on the machine. This file says what Better Input is, which of
three ways to build its engine was weighed and what each costs, how voice input
works from the key to the text, how the owner's voice key is heard without a
keylogger, how Better Manager installs and removes it in one action, the
control center page with every setting and state in both languages, and what
is not verified yet. The engine decision is
[ADR 0037](/engineering/decisions/0037-better-input-engine.md) (proposed); the
work is cut into tickets 180 to 191. The look follows `DESIGN.md`, and every
control named here is a `better_ui::fluent` control. The mockups in
`design/better-input/` draw the control center page (`page`), its voice group
while the speech model downloads (`voice-setup`), dictation in a text field
(`dictation`), and typing Chinese and English without switching (`typing`),
each as `-light.html` and `-dark.html`, in zh-TW. The English copy is in the
tables below.

Where this file and a ticket differ, the ticket decides behaviour and this file
decides layout and copy.

## 0. Decisions that are the owner's

Nothing below is built until D1 is answered; each other decision blocks only
the tickets named with it. Each has a recommendation and what each answer
costs.

| # | Decision | Recommendation | What each choice costs |
| --- | --- | --- | --- |
| D1 | **How the engine is built** (section 2, ADR 0037). (A) Fork 0c-ime (靈犀注音) and carry Better OS's changes in it. (B) Package upstream 0c-ime unmodified and add voice input beside it. (C) Write Better Input's engine in Rust on libchewing (新酷音's engine, now Rust), with 0c's MIT neural models for word choice and punctuation. | **C.** | **A:** the best typing on the first day (0c's frozen test: 86% first choice correct, against 64.2% for 新酷音 0.11.0, by 0c's own measurement). Better OS then maintains a Go engine (5 MB of Go) that upstream releases about twice a week, needs an ADR that widens the language policy (Go is allowed for networking and tooling, not an input method), must replace 0c's Python neural worker and Tauri settings program (both forbidden by the language policy), ships AGPL-3.0-only code and CC BY-SA 4.0 data with their notices and source offer (section 2.4), and needs a Go 1.26 toolchain that Ubuntu 24.04's archive does not have (it stops at 1.24). **B:** least work for typing, but voice text cannot enter 0c's composition: an IBus input context has one engine, and 0c's engine takes no text from another process, so "dictate, then correct by typing" is impossible unless upstream accepts such an interface; it also carries B's packaging gaps (Python and ONNX Runtime are not in the archive, upstream updates itself from the home directory). **C:** all Rust, inside the language policy, the composition is Better OS's own so voice fits naturally; the person's 新酷音 words and settings carry over because the dictionary format is libchewing's. The cost is the largest build (tickets 180–183) and typing quality that is unknown until measured: no product combines libchewing with 0c's reranker today, and mixed Chinese–English typing has to be written from scratch. Ticket 181's quality gate keeps 新酷音 the input source until Better Input beats it on a frozen test. |
| D2 | **Which speech model** (section 4.4). (a) MediaTek Breeze-ASR-25: Whisper large-v2 fine-tuned for Taiwanese Mandarin and Mandarin–English code-switching, Apache-2.0, 1.55 billion parameters. (b) OpenAI Whisper large-v3-turbo, MIT, 809 million parameters. (c) (a), plus Breeze-ASR-26 as an optional second language. | **(a).** | **(a):** the model built for how the owner speaks (Taiwanese Mandarin with English words); its card reports a 7.97 WER on CommonVoice16 zh-TW against 9.84 for Whisper large-v2 and 8.95 for large-v3. Its encoder is as heavy as large-v3's, so CPU dictation is slow (section 4.6). **(b):** faster to decode, but not tuned for Taiwanese Mandarin or code-switching, and Whisper often writes Simplified characters. **(c):** Breeze-ASR-26 is *not* a newer Mandarin model: it is Taiwanese Hokkien (台語) speech written as Mandarin characters (its card, 30.13 CER on MediaTek's Taigi benchmark). It doubles the download (another 1.1 GB) for a second language the ticket did not ask for. |
| D3 | **Where the speech model file comes from** (section 4.9). (a) Better OS converts MediaTek's pinned revision to whisper.cpp's format once, with whisper.cpp's own converter, and publishes it as a release asset with its SHA-256. (b) MediaTek's own ONNX export (`MediaTek-Research/Breeze-ASR-25-onnx-250806`), run with ONNX Runtime instead of whisper.cpp. (c) A conversion someone else published on Hugging Face. | **(a).** | **(a):** one runtime (whisper.cpp) with its CPU, Vulkan (the Radeon 890M) and Ryzen AI NPU paths; Better OS hosts about 1.1 GB per model version and redistributes it under Apache-2.0 with its NOTICE and the statement that it was converted. **(b):** no conversion and no hosting, but about 1.8 GB (int8 encoder 766 MB plus decoder 1.0 GB), and Better OS writes Whisper's decoding loop itself around ONNX Runtime. **(c):** nobody vouches for the file; rejected. |
| D4 | **When the models download** (section 4.9). (a) As part of installing Better Input: the install confirmation shows the total, and voice and smart word choice work when it finishes. (b) The first time the voice key is pressed. (c) Only after the person presses a Download button. | **(a).** | **(a):** "one action" holds (AGENTS.md); the install is about 1.4 GB larger (speech model about 1.1 GB, word choice 131 MB, punctuation 131 MB); on a metered connection the download waits and the page says so. **(b):** the first dictation waits minutes for a download nobody warned about. **(c):** a second step the person has to know about, which AGENTS.md says makes the feature not done. |
| D5 | **How Better Input hears the voice key** (section 5). (a) A small system service opens the one "Asus WMI hotkeys" node, reads it without taking it from the desktop, and reports only "the voice key was pressed". (b) A udev rule gives the person at the active seat read access to that node (`uaccess`), and Better Input's user service reads it itself. | **(a).** | **(a):** no process of the person's session opens anything under `/dev/input`, as ADR 0030 keeps it; a system unit, a udev rule, a seccomp filter, a bus policy, and a security review like `touchpad-input-passthrough-security.md` (ticket 188). **(b):** no system service, but every program the person runs can read every ASUS hotkey (brightness, volume, microphone mute) and can grab the node so those keys stop working in the session; a bus policy no longer stands between them and the device. |
| D6 | **The key for hold-to-talk** (section 5.5). Fn+F10 cannot be held: the kernel reports the press and the release together. (a) Hold Right Alt. (b) Hold Right Ctrl. (c) No hold-to-talk, only press-to-start. | **(a), off by default.** | **(a):** Right Alt on the US layout is a second Alt that GNOME does not bind; Better Input starts listening only after it is held 300 ms with no other key, so Alt+Tab with the right hand still works; one key fewer for shortcuts while the setting is on. **(b):** the same, but Right Ctrl is used more often in shortcuts. **(c):** the ticket asked for it as a setting. |
| D7 | **The kernel patch** (section 5.6), which needs a `Signed-off-by` from a real person under the kernel's Developer's Certificate of Origin. (a) The owner tests it on the M5406WA and sends it. (b) It is left in this file for someone else. | **(a).** | **(a):** the owner builds and loads one module on the laptop (a host change only the owner may make) and sends one mail to the platform-driver-x86 list; once it is merged, Fn+F10 arrives as a named key on every Vivobook of this model. **(b):** Fn+F10 stays an unknown key upstream, and the helper in D5 stays the only way to hear it. |
| D8 | **The input-source list after installing** (section 6.2). (a) Better Input replaces 新酷音 in GNOME's list; every other source stays where it was. (b) Better Input is added first and 新酷音 stays in the list. | **(a).** | **(a):** "no switching" is literal: Alt+Shift (ticket 94) has nothing to switch to unless the person keeps a keyboard layout in the list; removing Better Input puts the captured list back exactly. **(b):** a mistaken Alt+Shift lands in 新酷音, which looks like Better Input but behaves differently. |
| D9 | **A fourth update mechanism in AGENTS.md** (section 6.5). An IBus engine is started by `ibus-daemon`, not by systemd, and has no window, so none of AGENTS.md's three restart mechanisms fits it. Proposed: the engine exits when its program is replaced and no composition is open, and Better Input's user service, which the postinst restarts, asks IBus to start the new engine. | **Accept the amendment.** | **Accept:** AGENTS.md's update rule holds for the engine, with a test. **Refuse:** the engine runs the old program until the next login, which AGENTS.md forbids. |

## Sources this design was checked against

Read on the owner's machine on 2026-11-21, without writing anything:

- Zorin OS 18.1 (`/etc/os-release`, `VERSION_CODENAME=noble`), kernel
  `7.0.0-43-generic`, GNOME Shell 46.0, `ASUS Vivobook S 14 M5406WA_M5406WA`
  (`/sys/class/dmi/id/product_name`), `AMD Ryzen AI 9 HX 370 w/ Radeon 890M`, 24
  threads, `avx512f` present (`lscpu`), 30 GiB (`free -g`). The Radeon 890M is
  `1002:150e` and the NPU `1022:17f0` (`lspci -nn`); Mesa's `radeon_icd.json`
  Vulkan driver is installed (`/usr/share/vulkan/icd.d/`).
- Packages (`dpkg-query -W`): `ibus 1.5.29-2`, `ibus-chewing 2.0.0-1build2`,
  `libchewing3 0.6.0-1build1`, `ibus-gtk4 1.5.29-2`, `pipewire 1.0.5-1ubuntu3.3`,
  `pipewire-pulse 1.0.5-1ubuntu3.3`, `wireplumber 0.4.17-1ubuntu4.1`,
  `golang-go 2:1.22~2build1`. No `python3-onnxruntime` is installed and
  `apt-cache search onnxruntime` finds nothing; `apt-cache policy` offers
  `golang-1.24-go 1.24.13-2~24.04.1` at most. `fcitx5` is not installed.
- IBus runs as the user unit `org.freedesktop.IBus.session.GNOME.service`
  (`systemctl --user list-units`) as `/usr/bin/ibus-daemon --panel disable`
  (GNOME Shell is the panel); `ibus-engine-chewing --ibus` runs with 32,612 kB
  resident (`ps -o rss`). Its registry cache is `~/.cache/ibus/bus/registry`;
  there is no `/var/cache/ibus`. `/usr/share/ibus/component/` holds
  `chewing.xml` (engine `chewing`, symbol `&#x9177;` 酷, `hotkeys Super+space`,
  setup `/usr/libexec/ibus-setup-chewing`).
- `org.gnome.desktop.input-sources`: `sources [('ibus', 'chewing')]`,
  `mru-sources [('ibus', 'chewing'), ('xkb', 'us')]`, `per-window false`,
  `xkb-options @as []`. `org.gnome.desktop.wm.keybindings switch-input-source
  ['<Super>space', 'XF86Keyboard']` (ticket 94 moves it to Alt+Shift),
  `minimize ['<Super>h']`; no binding in that schema uses Alt+Shift, and there are
  no custom key bindings.
- `org.freedesktop.IBus.Chewing` (the person's 新酷音 settings):
  `cand-per-page 5`, `sel-keys '1234567890'`, `kb-type 'default'`,
  `chi-eng-mode-toggle 'shift'`, `space-as-selection false`,
  `vertical-lookup-table true`, `easy-symbol-input true`,
  `phrase-choice-from-last true`, `add-phrase-direction true`,
  `auto-shift-cur true`, `default-english-case 'lowercase'`,
  `esc-clean-all-buf false`, `plain-zhuyin false`. The person's learned words
  are `~/.chewing/chewing.sqlite3` (131,072 bytes; only the name and size were
  read).
- `/proc/bus/input/devices`: `N: Name="Asus WMI hotkeys"`,
  `P: Phys=asus-nb-wmi/input0`, `S: Sysfs=/devices/platform/asus-nb-wmi/input/input14`,
  `H: Handlers=rfkill kbd event8`, `B: EV=100013`, `B: MSC=10`. Its `KEY=`
  bitmap has bit 240 (`KEY_UNKNOWN`) set and bit 582 (`KEY_VOICECOMMAND`) clear.
  `/dev/input/event8` is `crw-rw---- root input`, no ACL (`getfacl`);
  `/dev/input/by-path/platform-asus-nb-wmi-event` points to it. `udevadm info`:
  `ID_INPUT=1`, `ID_INPUT_KEY=1`, `ID_PATH=platform-asus-nb-wmi`,
  `KEYBOARD_KEY_6b=f21`, `KEYBOARD_KEY_7c=f20` (hwdb remaps codes already in
  the driver's table), `TAGS=:power-switch:`. The kernel's "Unknown key code
  0xe7" line was not in this boot's journal as the user can read it; the ticket's
  2026-11-13 observation stands for it.

Read from upstream sources on 2026-11-21:

- 0c-ime, `0c-biggo/0c-ime` (GitHub API and the files named): public, not
  archived, created 2026-10-19, last push 2026-11-19, 183 stars, licence
  `AGPL-3.0`; `LICENSE` is byte-identical to the GNU AGPLv3 text
  (<https://www.gnu.org/licenses/agpl-3.0.txt>, compared with `cmp`). Languages
  by bytes: Go 5,048,679, Rust 837,064, Python 816,051, Swift 354,906,
  PowerShell 342,976, JavaScript 226,069, C++ 219,734. Releases: 14, from
  `v0.2.0` (2026-10-20) to `v0.2.13` (2026-11-19), every one marked
  "public-beta" in `CHANGELOG.md`; each ships `0c-ime-<v>.tar.xz` (source and
  compiled dictionary) and, from 0.2.3, `0c-ime-models-<v>.tar.xz`.
  `README.md`: Go 1.26, shells for Fcitx5 (C++), IBus (Go), macOS (Swift),
  Windows (Rust TSF), a Tauri settings program, and "神經推論：Python＋ONNX
  Runtime"; own code AGPL-3.0-only, own icons CC BY-SA 4.0, data and models under
  their own licences. `legal/README.md`: own lexicon and frequencies CC BY-SA 4.0,
  neural models MIT, commercial licence available. `PACKAGING.md`: the models are
  not DFSG-free (no training code or data) and ship separately.
  `platforms/linux-ibus/README.md`: `0c-ibus` (Go) is started by `ibus-daemon`,
  hands keys to `0c-server`, uses `IBUS_ENGINE_PREEDIT_COMMIT` mode, draws
  styles with IBus attributes that only GTK and Qt's IBus modules render, and its
  `register` step restarts IBus and writes `environment.d` files.
  `platforms/linux-system/README.md`: system install under `/usr`, the engine
  "不常駐：外殼在第一個按鍵帶起它", models downloaded by the person into
  `~/.local/share/zuin/models/` (about 262 MB), inference through the system's
  `python3-numpy` and `python3-onnxruntime`, personal data in
  `~/.local/share/zuin` and `~/.local/share/0c/settings.json`.
  `platforms/neural/requirements.txt`: `numpy==2.5.3`, `onnxruntime==1.30.0`.
  `internal/settings/settings.go`: `Default()` (lines 266–271) leaves
  `MixedInput` false; fuzzy defaults to `select` (lines 139, 246); `Toneless`
  turns mixed input off ("與混打二選一", lines 123–124). `CHANGELOG.md`: mixed
  input arrived as "中英混打（試用）… 預設關閉" (0.1.0-dev.47); fuzzy became
  default-on in dev.58; toneless arrived in 0.2.5, default off, standard layout
  only, and disables mixed input.
- 0c's site (<https://0c.biggo.com/technology/>): the two models "約佔 250 MB，
  常駐時需要額外的記憶體與 CPU"; P95 word-choice latency 7 ms to 16 ms on "一般桌機
  CPU", falling back to the statistical order past 100 ms; nothing on idle
  memory. <https://0c.biggo.com/download/>: one Linux download, a 392.5 MiB
  amd64 tar.gz with a one-line installer; requirements "IBus 或 Fcitx5 5.1.7
  以上，Python 3.9 以上"; no .deb, no arm64.
- `Funmula/0c-sequence-d512` (Hugging Face, MIT): a 32.6M-parameter causal
  decoder that scores supplied candidates given up to 64 characters of context
  and the Zhuyin reading, tones optional; `model.onnx` 130,723,137 bytes, SHA-256
  `4a80caf1…9261`; a 4-bit version of 86 MB that grew resident memory by about
  45 MB against 153 MB for float32 in 0c's Android measurement; operating point
  w = 2, τ = 0, to be recalibrated "when using a different engine's score
  scale"; the card's MIT example documents every input tensor.
  `Funmula/0c-punct-d512`: `model.onnx` 130,584,075 bytes, MIT.
- libchewing (`chewing/libchewing`, GitHub API): Rust, LGPL-2.1, last push
  2026-11-03, release `v0.12.0` (2026-04-06); crates.io `chewing` 0.13.1
  stable, 0.14.0-rc.1 (2026-11-07), `LGPL-2.1-or-later`. `NEWS`: 0.9.0 added a
  fuzzy mode with "prefix search, partial zhuyin search, toneless input"; 0.14.0
  removes migration from the legacy sqlite user dictionary. Ubuntu's
  `libchewing3-data` copyright file lists the data as LGPL-2.1-or-later.
  `chewing/ibus-chewing`: C, GPL-2.0, release `v2.1.7` (2025-09-05).
- McBopomofo (`openvanilla/McBopomofo`): Swift, MIT, macOS.
  `openvanilla/fcitx5-mcbopomofo`: C++, MIT, release 3.1.1 (2026-09-02), Fcitx5
  only, built from source on Ubuntu (not in the noble archive per
  `apt-cache policy`). `rime/librime` BSD-3-Clause, `rime/ibus-rime` GPL-3.0,
  `rime/rime-bopomofo` LGPL-3.0 (大千 and 動態能力 layouts, depends on
  `terra-pinyin`); noble has `ibus-rime 1.5.0-5build2` and
  `rime-data-bopomofo 0.0~git20210131`.
- `MediaTek-Research/Breeze-ASR-25` (Hugging Face): Apache-2.0, `zh`/`en`,
  Whisper large-v2 fine-tune "Optimized for Taiwanese Mandarin" and
  "Mandarin-English code-switching"; `config.json` `d_model 1280`, 32 encoder and
  32 decoder layers (Whisper large's 1550M parameters, OpenAI's table);
  `model.safetensors` 3,086,761,032 bytes; WER table in the card.
  `MediaTek-Research/Breeze-ASR-26`: Apache-2.0, `nan`/`zh`, "BreezeASR-Taigi",
  outputs Mandarin characters for Taiwanese Hokkien speech, two safetensors shards
  of 4,992,706,480 and 1,180,663,192 bytes (float32), same architecture.
  `MediaTek-Research/Breeze-ASR-25-onnx-250806`: tagged MIT, int8 encoder
  765,942,323 bytes and int8 decoder 1,008,223,031 bytes, converted for
  sherpa-onnx. Third-party whisper.cpp conversions exist
  (`alan314159/Breeze-ASR-25-whispercpp`: q5_k 1,080,732,108 bytes, q8_0
  1,656,129,708 bytes).
- OpenAI Whisper README (`openai/whisper`): sizes tiny 39M, base 74M, small
  244M, medium 769M, large 1550M, turbo 809M; "Whisper's code and model weights
  are released under the MIT License".
- whisper.cpp (`ggml-org/whisper.cpp`, MIT, release v1.9.5 2026-10-06), README:
  memory tiny ~273 MB … large ~3.9 GB; Vulkan backend (`-DGGML_VULKAN=1`);
  Silero VAD; "AMD Ryzen AI NPU support" on "Linux (Ubuntu 24.04 LTS, Python
  3.12)" through XRT and AMD's FlexML runtime, with prebuilt encoder caches only
  from AMD's Hugging Face collection; PR #3608 ("[AMD] Add VitisAI Plugin…",
  merged 2026-08-24) "Requires proprietary packages from AMD". AMD's caches on
  Hugging Face cover Whisper tiny, base, small, medium, large-v3 and
  large-v3-turbo; the Ryzen AI 1.9 ones are gated (access denied without a
  request).
- Rust bindings and runtimes (crates.io API): `whisper-rs` 0.16.0 Unlicense
  (moved to Codeberg; its GitHub copy is archived), `ort` 2.0.0-rc.15
  `MIT OR Apache-2.0`, `candle-transformers` 0.11.1 `MIT OR Apache-2.0`,
  `ct2rs` 0.11.2 MIT (CTranslate2 itself MIT), `sherpa-rs` 0.6.8 MIT
  (sherpa-onnx Apache-2.0), `pipewire` 0.10.1 MIT, `zbus` (the workspace pins
  5). `ibus` 0.2.0 (2022) is the only IBus crate.
- Published CPU numbers on hardware like the owner's (others' numbers, not
  measured here): whisper.cpp issue #89, comment 3697031893 (2025-12-29), Ryzen
  AI 9 HX 375 with Radeon 890M under WSL: `base.en` encoder about 389–442 ms on
  the CPU, the 11-second `jfk.wav` transcribed in about 816 ms with 16 threads;
  Vulkan through WSL's Dozen layer was faster for the encoder and slower
  overall. `drakosha/whisper-xdna` README: Ryzen 7 PRO 8845HS (8 Zen 4 cores),
  whisper.cpp `large-v3-turbo` q5_0 with 16 threads, a 22.6-second clip: encoder
  6,880 ms at full context and 2,165 ms at `audio_ctx 512`, the large-v3 encoder
  6,940 ms and about 106 CPU-seconds; an open-toolchain XDNA1 (Phoenix) NPU path
  at 3,344 ms. OpenBenchmarking.org and Phoronix refused the request (HTTP 403),
  so their HX 370 results were not read.
- Linux master (`torvalds/linux`, head `d0f871bf`, 2026-11-20):
  `drivers/platform/x86/asus-nb-wmi.c` has no entry for 0xE7 in
  `asus_nb_wmi_keymap` (lines 609–691; the nearest are 0xCA `KEY_F13` and 0xCB
  `KEY_F14`, lines 684–685); `asus-wmi.c` reports every code with
  `autorelease = 1` (line 4682) and logs "Unknown key code" when the keymap has
  none (lines 4745–4747); `drivers/input/sparse-keymap.c`
  `sparse_keymap_report_event` reports an unknown code as `KEY_UNKNOWN` with its
  scan code, autoreleased (lines 273–292, 234–245), and
  `sparse_keymap_setkeycode` refuses a code not in the table with `-EINVAL`
  (lines 133–152), which is why hwdb cannot remap 0xE7.
  `include/uapi/linux/input-event-codes.h`: `KEY_VOICECOMMAND 0x246`
  (line 620). `MAINTAINERS`: "ASUS NOTEBOOKS AND EEEPC ACPI/WMI EXTRAS DRIVERS"
  (lines 4222–4230) and "X86 PLATFORM DRIVERS" (lines 30436–30444), list
  platform-driver-x86@vger.kernel.org. A web search for the code found no patch
  for it.
- IBus 1.5.29 (`ibus/ibus`, tag `1.5.29`): `bus/ibusimpl.c` builds its engine
  table from component files once at start (`bus_ibus_impl_registry_init`,
  lines 2255–2285) and, when a component file changes, only emits
  `RegistryChanged` (lines 2435–2438); an engine registered at run time with
  `RegisterComponent` goes to a separate list (lines 1320–1365) that `Engines`,
  which lists only the table, does not include (lines 1377–1401). GNOME Shell
  46's `js/misc/ibusManager.js` lists engines once per connection
  (`_initEngines`, lines 166–181). So a newly installed engine appears in GNOME
  only after `ibus-daemon` restarts. `src/ibusengine.h`:
  `ibus_engine_update_preedit_text_with_mode` (line 254).
- `text-input-unstable-v3.xml` (wayland-protocols `main`): `preedit_string`
  carries text and a cursor range only; style hints (`preedit_hint`) arrived in
  version 2. Mutter 46 advertises `zwp_text_input_manager_v3`
  (`design/top-bar-spec.md` sources); whether at version 2 was not read, so this
  design assumes a GTK 4 window on GNOME 46 draws every preedit with one
  underline.
- GPLv3 and AGPLv3 section 13 (<https://www.gnu.org/licenses/gpl-3.0.txt>,
  <https://www.gnu.org/licenses/agpl-3.0.txt>).

## 1. What Better Input is

```
 keyboard ──► GNOME Shell ──► ibus-daemon ──► better-input-engine ◄──► better-input-lm
                (panel,          (IBus)        (Rust, IBus engine,        (word choice and
              candidate                         the composition)          punctuation models)
               popup)                                  ▲
                                                       │ text, unicast
 Fn+F10 ──► Asus WMI hotkeys ──► better-input-voicekey │
            (/dev/input/event8)   (system unit,        │
                                   reports one key) ──►│ better-input-voice
 microphone ──► PipeWire ─────────────────────────────►  (user unit: capture,
                                                         speech model, punctuation)
```

| Process | Runs as | Started by | What it may touch |
| --- | --- | --- | --- |
| `better-input-engine` | the person | `ibus-daemon`, from `/usr/share/ibus/component/better-input.xml` | the IBus private bus, the session bus (only `org.betteros.Input1` peers), `~/.config/better-os/input/`, `~/.local/share/better-os/input/` |
| `better-input-lm` | the person | the engine, on demand, as a child that talks only over its stdin and stdout and opens no socket | the model files, read-only |
| `better-input-voice` | the person | its systemd user unit, enabled at install (AGENTS.md) | PipeWire (one capture stream, only while listening), the session bus, the system bus (the voice key's signal), the models, the network only to download pinned model files |
| `better-input-voicekey` | root for its first millisecond, then its own account | udev, for the one device in section 5.3 | `/dev/input/event<N>` of that device, read-only, and the system bus |

Every decision lives in a GPUI-free Rust crate with tests: `input-core`
(composition, conversion, mixed typing, learning, settings), `input-voice-core`
(the streaming policy, punctuation and spoken-symbol rules, model catalog),
`input-voicekey` (the device judgement and the event filter). The page lives in
`manager-gui` like every control center page.

## 2. How the engine is built (D1)

### 2.1 0c-ime as it is on 2026-11-21

| Question | Answer, with its source above |
| --- | --- |
| Is it still there? | Yes: public, active, last push 2026-11-19, 14 releases in 31 days, all "public-beta". |
| Licences | Own code AGPL-3.0-only; own icons and own lexicon/frequency data CC BY-SA 4.0; third-party lexicon sources each under their own (ledger in `data/sources/`); neural models MIT, published separately on Hugging Face and in a separate tarball because they are not DFSG-free. A commercial licence is offered. |
| Language | Go 1.26 for the engine (`0c-server`) and the IBus shell (`0c-ibus`); C++ for the Fcitx5 shell; Python for the neural workers; Tauri (Rust and JavaScript) for the settings program; Swift and Rust for macOS and Windows. |
| IBus and Fcitx5 | Both; the IBus shell is Go, started by `ibus-daemon`; Fcitx5 needs 5.1.7 or later. Untested by upstream: Ubuntu 24.04's Qt 6.4, GNOME on X11, HiDPI. |
| Mixed Chinese–English | Yes ("中英混打"), but off by default, and turned off by toneless input and by the 許氏 and 倚天 layouts. Hundreds of special cases in the changelog (model numbers, decimals, "我ㄉ天"). |
| Fuzzy Zhuyin | ㄗ/ㄓ, ㄘ/ㄔ, ㄙ/ㄕ, ㄣ/ㄥ, on by default as extra candidates ("select"). |
| Toneless | Yes, off by default, standard layout only, excludes mixed input. |
| Tab completion | Yes, only from the lexicon and personal phrases, only when confident. |
| Personalisation | Local personal dictionary (`~/.local/share/zuin/user.json`), "我的詞彙", learning can be paused. |
| Packaging for Ubuntu 22.04/24.04 | No .deb and no arm64 from upstream; a home-directory installer that updates itself; `make install PREFIX=/usr` for distributions. Building needs Go 1.26 (noble's archive has 1.24) and the neural path needs `python3-onnxruntime` (not in noble's archive). |
| How it runs | `0c-ibus` (started by `ibus-daemon`) starts `0c-server` on the first key; `0c-server` starts Python workers for the models when they are enabled. |
| Memory and CPU | Upstream says the two models take "約 250 MB" resident and extra CPU while loaded, P95 7–16 ms per choice; idle and typing figures for the whole stack are not published. |

### 2.2 The alternatives, against "no switching, smarter choice"

| Engine | Licence | On GNOME 46 with IBus | No switching | Smarter choice |
| --- | --- | --- | --- | --- |
| ibus-chewing + libchewing (today) | GPL-2.0 + LGPL-2.1 | yes, installed | no: Shift toggles 中/英 | statistical (0c measured 64.2% first choice for 0.11.0); fuzzy and toneless modes exist from libchewing 0.9, but noble ships 0.6.0 |
| McBopomofo / fcitx5-mcbopomofo | MIT | no: Fcitx5 only, not in the archive | no | statistical (67.2% in 0c's test) |
| Rime with `rime-bopomofo` | BSD-3 / LGPL-3.0 / GPL-3.0 | yes (`ibus-rime` in noble) | no | statistical, schema-driven |
| 0c-ime | AGPL-3.0-only + CC BY-SA 4.0 + MIT | yes | yes, in beta, off by default | neural reranking (86% in its own test) |

None but 0c meets both halves today. That is why the choice is between taking
0c (A, B) and building the missing halves on the best open engine (C).

### 2.3 The three options

**(A) Fork 0c-ime.** Better OS carries 0c's Go engine and IBus shell, adds the
interface voice needs (text from `better-input-voice` into the composition),
replaces the Python workers with a Rust worker speaking the same stdio protocol
(the language policy forbids Python in a production component), replaces the
Tauri settings program with the control center page writing 0c's
`settings.json` through `0c-update set`, drops the Fcitx5, macOS and Windows
shells, and rebases on upstream's releases. Work: packaging a Go build with a
Go 1.26 toolchain fetched into the build environment (ADR 0021's release
environment changes), a Rust worker, the voice interface in Go, the page.
Ongoing: a rebase onto a fast upstream, in a language no other Better OS
component uses.

**(B) Upstream as is, voice beside it.** Better OS packages an unmodified
release (Go built from the source tarball, no Python: the statistical engine
only, 76.9% in 0c's test, or the neural path with a bundled Python, which the
language policy forbids), and voice input commits its text separately. It
cannot meet "streaming text into the same composition so it can be corrected
by typing": the focused input context has exactly one engine, 0c's, which takes
no text from another process. Voice could only commit finished text through a
second path (a synthetic key stream, which the brief rules out, or a second
engine, which IBus does not run at the same time). B becomes viable only if 0c
accepts an upstream interface for external text, which nobody can promise.

**(C) A Rust engine on an existing language model.** Better OS writes
`better-input-engine` on the `chewing` crate (libchewing's Rust core,
LGPL-2.1-or-later, with Zhuyin layouts, conversion, fuzzy and toneless modes,
the user dictionary), adds 0c's MIT word-choice model as a reranker over
libchewing's N-best (the model card documents the inputs, the fusion rule, and
that w and τ must be recalibrated for another engine), 0c's MIT punctuation
model for voice and optional "punctuate on Enter", and writes mixed
Chinese–English typing itself (section 3.2). Work: tickets 180–183, the largest
of the three. Ongoing: Better OS's own code, versioned with the rest.

### 2.4 Licence consequences

| | A: fork | B: upstream as is | C: Rust engine |
| --- | --- | --- | --- |
| Code Better OS ships | 0c's Go under AGPL-3.0-only, modified, plus Better OS's GPL-3.0-or-later | 0c's Go under AGPL-3.0-only, unmodified | Better OS's GPL-3.0-or-later; libchewing LGPL-2.1-or-later, which section 3 of LGPL-2.1 lets a GPL program carry; whisper.cpp MIT; ONNX Runtime MIT |
| How the licences sit together | GPLv3 §13 permits combining a GPL-3.0 work with an AGPL-3.0 work and conveying the result; each part keeps its licence, and AGPLv3 §13's network clause applies to the combination. Better OS's GPL-3.0-or-later code is used under version 3 for this. If the two stay separate programs talking over pipes and D-Bus, they are an aggregate, and §13 is not even needed. | the same as A, and nothing is combined | no AGPL code at all |
| AGPLv3 §13 (network users) | a *modified* version must offer its source to "all users interacting with it remotely through a computer network". A local input method has no remote users; the clause bites only if a part of the fork ever serves someone over a network (for example 0c's local vocabulary API bound beyond loopback), so the fork must never do that without a source offer | does not apply: unmodified | does not apply |
| Source offer for the binaries (GPLv3/AGPLv3 §6) | every `.deb` carrying the fork needs its Corresponding Source "from the same place" (§6d): the fork's tagged source next to the release assets, with the build scripts | the same, for 0c's unmodified source tarball | Better OS's existing source offer covers it |
| CC BY-SA 4.0 data | 0c's compiled lexicon ships in the package: attribution to Funmula Co., Limited, the licence URI, a note of changes; any dictionary Better OS changes is itself CC BY-SA 4.0 | the same, unchanged | none; libchewing's data is LGPL-2.1-or-later (Debian's copyright file) |
| MIT models | downloaded, never in the `.deb`; the MIT notice is kept beside the files and listed in the third-party notices | the same | the same |
| Third-party notices | `generate-third-party-notices.sh` covers `Cargo.lock` only; a Go module inventory (`go.sum`) is a new generator | the same | `Cargo.lock` covers it, plus the model and data notices |
| Language rule | Go is allowed "only when networking, remote management, agents, or standalone service tooling has a concrete advantage" (`engineering/specs/language-policy.md`); an input method is none of these, so the ADR must widen the policy; Python and Tauri parts must be replaced | the same for Go; the Python path cannot ship | inside the rule; whisper.cpp and ONNX Runtime are C/C++ libraries behind Rust bindings, which the policy accepts as dependencies |

### 2.5 Recommendation

C, recorded in ADR 0037. It is the only option that meets every requirement of
the ticket (voice in the same composition) without a third party's
cooperation, stays inside the language policy, and keeps the person's 新酷音
words. Its weakness is quality risk, which ticket 181 turns into a gate:
Better Input does not replace 新酷音 as the input source until it beats
libchewing's first-choice accuracy on a frozen test set, with 0c's statistical
and neural results on the same set reported beside it as the bar to reach.

## 3. Typing

Everything in this section is the recommended engine (C). It follows the
person's existing 新酷音 settings on first run (section 3.6).

### 3.1 The composition

- Keys go to the engine through IBus; the engine keeps one composition per input
  context and draws it as IBus preedit in `IBUS_ENGINE_PREEDIT_COMMIT` mode, so a
  focus change commits what is shown and nothing is lost. A composition that is
  only an unfinished syllable (ㄕ with no tone) is cleared, not committed, on a
  focus change, as 0c learned (#322).
- The composition is a list of segments, each with its source: typed Zhuyin
  (with its reading and the chosen candidate), typed English (its letters), and
  dictated text (its words and whether they are still tentative). Editing keys
  (←, →, Backspace, Delete, Home, End, ↓ to choose) work on every segment the
  same way, which is what lets dictated text be corrected by typing.
- Enter commits the composition; Esc clears it (or, with
  `esc-clean-all-buf false` as on the machine, only the current syllable).
- Candidates: GNOME Shell draws the candidate popup (it is the IBus panel), one
  page at a time, vertical (from `vertical-lookup-table true`), 5 per page (from
  `cand-per-page 5`), keys 1–0 (from `sel-keys`). The auxiliary text above the
  candidates carries hints (Tab suggestion, the listening state).
- Styles: IBus attributes (underline, colour) reach only applications that use
  IBus's GTK or Qt module; GTK 4 on GNOME 46 Wayland draws every preedit with one
  underline (sources). So no state is shown by style alone: tentative dictated
  text and Tab suggestions are shown in the auxiliary text, never only as a
  colour in the preedit.
- Password and PIN fields (IBus content purpose `PASSWORD` or `PIN`, or the
  private hint): GNOME already disables input methods in them; the engine also
  sends nothing to the models, learns nothing, and refuses dictation.
- Context for word choice: the last 64 characters the engine committed into this
  context, confirmed with `SetSurroundingText` when the application reports it
  and the text matches; otherwise only what this engine committed since focus.

### 3.2 Chinese and English without switching

On by default (the ticket; owner, 2026-11-13). The engine decides, as keys
arrive, whether a run of keys is Zhuyin or English:

1. A run that does not parse as Zhuyin syllables in the person's layout is
   English (for example `python` on the standard layout).
2. A run that parses both ways is scored both ways: the Chinese reading by
   libchewing and the reranker, the English word by an English lexicon (a
   permissively licensed word list; ticket 182 names it and checks its licence)
   and the person's own English words. The higher-scoring reading is shown; the
   other is the first alternative under ↓.
3. Capital letters (Shift+letter) are English always (`DNS`, `iPhone`).
4. Digits after English stay digits (`win10`, `3.4`, `04:10`); 0c's changelog
   lists the cases that need this, and ticket 182's tests carry them.
5. A space between Chinese and English is added when committing if "Add a
   space between Chinese and English" is on (off by default, as 0c).
6. In mixed mode the space bar always types a space; choosing a candidate is ↓
   or the selection keys, and "Space selects" is shown disabled with the reason.
7. A single Shift tap still toggles a pure English mode (the person's
   `chi-eng-mode-toggle 'shift'`), for long English text. Shift with any other
   key, Shift held over 300 ms, and Shift while Alt is held (ticket 94's
   Alt+Shift switches input sources) are not a tap.

Toneless input and mixed input exclude each other, as in 0c (with tones
optional, almost every letter run is also a Zhuyin run): turning one on turns
the other off, and the page says so.

### 3.3 Fuzzy Zhuyin and toneless

Fuzzy pairs ㄗ/ㄓ, ㄘ/ㄔ, ㄙ/ㄕ, ㄣ/ㄥ add the near readings' candidates after the
exact ones, each marked 近音 in the candidate comment; on by default, each pair
its own setting. Toneless ("不打聲調") uses libchewing's fuzzy conversion and the
reranker's unknown-tone input (`?` in the model card); off by default.

### 3.4 Tab completion

When the composition ends where a phrase in the lexicon or the person's words
continues, and the continuation is the clear best, the auxiliary text shows it
(`Tab ⇥ 天氣很好`); Tab accepts it into the composition, any other key ignores
it. Nothing is generated: only lexicon and personal phrases, as 0c does. On by
default.

### 3.5 Learning and "My words"

- What the person chooses is learned into their user dictionary (libchewing's
  user phrase store) when "Learn from what I choose" is on (default on). Nothing
  is learned in password fields or while the setting is off.
- "My words" lists the person's own phrases with their readings; add one (word
  and its Zhuyin, which the page fills from the lexicon and the person can
  correct), remove one, export as a text file, import such a file. "Clear what
  Better Input learned" removes learned entries and keeps the person's own words.
- Shift+← / Shift+→ in the composition marks a span, Enter adds it as a word;
  Shift+Delete on a highlighted candidate forgets a learned one (0c's keys, which
  are free in 新酷音 too).

### 3.6 Carried over from 新酷音

On first run the engine reads, never writes, the person's 新酷音 state:

- settings from `org.freedesktop.IBus.Chewing` (layout, selection keys,
  candidates per page, vertical list, Space as selection, the Shift toggle, easy
  symbols, cursor direction for phrases);
- learned phrases from `~/.chewing/chewing.sqlite3` (libchewing 0.6's sqlite
  format, read with libchewing 0.13's reader, since 0.14 drops it), copied into
  Better Input's own store.

新酷音's files and settings are left exactly as they were, so removing Better
Input finds them untouched.

### 3.7 Quality gate

Ticket 181 builds a frozen test set (Taiwanese text with dictionary-synthesised
readings, as 0c's method, from a licence-clean corpus), and reports first-choice
accuracy for libchewing alone, Better Input statistical, Better Input with the
reranker, and 0c 0.2.13 (statistical and neural, run unmodified in a container as
a measuring instrument only). Better Input becomes the input source at install
only when it beats libchewing alone; until then the component installs but the
Defaults entry that switches the source is withheld and the page says why.

## 4. Voice input

### 4.1 What the person does

| Mode | How it starts | How it ends |
| --- | --- | --- |
| Press to start (default) | press the voice key (Fn+F10 on the M5406WA) or the dictation shortcut | press it again; or Enter, Esc, or any typing key; or silence for the chosen time |
| Hold to talk (setting, D6) | hold Right Alt for 300 ms with no other key, in a text field | release it |

While listening, recognised words stream into the composition at the cursor:
words that will not change enter the preedit; the tail that may still change is
shown in the auxiliary text above the cursor (section 3.1 says why not as a
style). When listening ends, the final pass replaces the whole dictated segment,
and the text stays in the composition: arrows, Backspace and typing correct it,
↓ on a word offers the recogniser's alternatives and the homophones libchewing
knows for its reading, and Enter commits. Esc while listening cancels the
dictated segment; Esc after it clears it like any composition.

Typing while listening stops listening first, then the key acts on the
composition, so the person can always take over. Dictation starts only in a
focused text field with an active Better Input engine; elsewhere the voice key
raises a notification (section 7.6) and records nothing.

### 4.2 The path of the audio and the text

1. The key: `better-input-voicekey` emits `Pressed` on the system bus
   (section 5), or the engine sees the hold-to-talk key, or GNOME runs the
   dictation shortcut (`better-input-voice --toggle`, a Better Defaults global
   shortcut).
2. `better-input-voice` asks the focused engine whether dictation may start
   (`org.betteros.Input1.Engine.BeginDictation`, answered with a serial, or
   refused for a password field or no focus).
3. It opens one PipeWire capture stream (16 kHz mono, `media.role` and
   `application.name` "Better Input" so the desktop shows who records), only now,
   and closes it when listening ends. On Ubuntu 22.04, where PulseAudio is the
   sound server, the same interface uses PulseAudio's simple API.
4. Audio goes through Silero VAD and the recogniser (section 4.5) in memory; it
   is never written to disk.
5. Each hypothesis goes to that engine alone (`Partial(serial, stable,
   tentative)`, then `Final(serial, text, alternatives)`), checked by the
   engine's bus name and `/proc/<pid>/exe` as ADR 0018 checks its counterparts.
   Transcripts are personal data: never broadcast, never logged, kept only until
   `Final` is delivered.
6. The engine puts the text into the composition (section 4.1).

This path is why the engine choice matters (D1): text reaches the application
only as the focused engine's own preedit and commit. Option C owns that engine;
option A gets it by changing 0c's engine; option B cannot have it.

### 4.3 Punctuation, Chinese and English

- The recogniser writes English words as English (Breeze-ASR-25 is tuned for
  code-switching: its card's example keeps "open mind", "explore", "commitment").
- Punctuation: Breeze's own output has spaces where a pause was and little
  punctuation (its card's example). On `Final`, 0c's MIT punctuation model
  inserts ，。？！、； between Han characters without changing a character; a pause
  space between two Han characters is removed when punctuation is inserted
  there; English keeps its spaces. Setting "Add punctuation" (default on).
- Spoken symbols (setting, default on): 逗號 ，, 句號 。, 問號 ？, 驚嘆號 ！, 冒號 ：,
  分號 ；, 頓號 、, 換行 (a new line, committed with the composition), 刪掉 (removes
  the last dictated phrase), recognised only as a whole phrase between pauses.
- Output is Traditional Chinese. If the recogniser emits a Simplified character
  that has a single Traditional form, it is converted (a table from OpenCC's
  data, Apache-2.0); a character with several Traditional forms is left and its
  forms offered under ↓.

### 4.4 The model (D2)

| Model | Parameters | Licence | Languages | Size as whisper.cpp q5_k | Note |
| --- | --- | --- | --- | --- | --- |
| **Breeze-ASR-25** (recommended) | 1.55 B (Whisper large-v2) | Apache-2.0 | Taiwanese Mandarin, English, code-switching | about 1.08 GB (a third-party conversion; ours will be measured) | CommonVoice16 zh-TW WER 7.97 (card) |
| Breeze-ASR-26 | 1.55 B (Whisper large-v2) | Apache-2.0 | Taiwanese Hokkien in, Mandarin characters out | about 1.08 GB | not a newer Mandarin model; the ticket's "Breeze-ASR-26 … Taiwanese Mandarin" is corrected here |
| Whisper large-v3-turbo | 809 M | MIT | multilingual | not read | large-v3 encoder, 4-layer decoder |
| Whisper medium | 769 M | MIT | multilingual | not read | |
| Whisper small | 244 M | MIT | multilingual | not read | in ticket 98's measurement list |

The research note's "Breeze-ASR-25, about 2B parameters" does not match its
`config.json`, which is Whisper large-v2's shape (1.55 B).

### 4.5 The runtime

| Runtime | Licence | Rust form | CPU | GPU (890M) | NPU (Strix) | Fits the language rule |
| --- | --- | --- | --- | --- | --- | --- |
| **whisper.cpp** (recommended) | MIT | `whisper-rs` (Unlicense), builds whisper.cpp from source | AVX-512, quantised models, VAD built in | Vulkan backend | VitisAI plugin on Ubuntu 24.04, proprietary FlexML runtime, caches only for OpenAI's models | yes: a C/C++ library behind a Rust binding |
| ONNX Runtime | MIT | `ort` (`MIT OR Apache-2.0`), library shipped in the package (not in the archive) | int8 | not for this GPU on Linux without ROCm | VitisAI execution provider (Ryzen AI, EULA) | yes, same reason; decoding loop is Better OS's |
| candle | `MIT OR Apache-2.0` | native Rust | yes, slower than whisper.cpp on CPU in general (not measured here) | no Vulkan | no | yes, pure Rust |
| CTranslate2 (faster-whisper's engine) | MIT | `ct2rs` (MIT) | int8, fast | no AMD GPU | no | yes |

whisper.cpp is recommended because it alone has a CPU path, a path to this GPU
(Vulkan, no ROCm), and the only Strix NPU path on Linux today, all behind one
API, and it reads the converted Breeze file directly. The word-choice and
punctuation models stay on ONNX Runtime through `ort` (they are ONNX, and that
runtime also has the NPU's VitisAI provider).

### 4.6 Latency and memory: what others measured, and what it implies

None of these numbers was measured on the owner's machine; they are others'
numbers on similar chips, and ticket 186 measures the real ones before any
default is chosen.

- Ryzen AI 9 HX 375 (same CPU family and GPU as the HX 370), whisper.cpp, WSL:
  `base.en` encoder about 0.4 s on the CPU; an 11-second clip transcribed in
  about 0.8 s.
- Ryzen 7 PRO 8845HS (8 Zen 4 cores): whisper.cpp `large-v3-turbo` q5_0 with
  16 threads takes 6.9 s for the encoder over a full 30-second window and
  2.2 s with `audio_ctx 512` (about 10 seconds of audio), using about 106
  CPU-seconds for the full window.
- Breeze-ASR-25 has the same encoder size as large-v3 (32 layers, width 1280),
  so on the HX 370's CPU a final pass over a few seconds of speech should take
  seconds, not tenths of a second, and keep every core busy while it runs. This
  is an inference from the two numbers above, not a measurement.
- Memory: whisper.cpp's table gives about 3.9 GB for a large model at f16; a
  q5 model's file is about 1.1 GB, so a few hundred MB above that while loaded
  is expected (to be measured).

What the design does with that:

- Partial results are produced from the audio so far with `audio_ctx` sized to
  it (the encoder's cost falls with the context, as the 8845HS numbers show),
  re-run about every 1.5 s, and a word becomes stable when two consecutive runs
  agree on it (the "local agreement" policy of Macháček et al., 2023,
  arXiv 2307.14743).
- The final pass runs once, over the whole utterance (split at VAD pauses, at
  most 30 s per window).
- The model loads on the first dictation after login and unloads after 5 minutes
  without dictation, so idle memory is the service's own few MB.
- Threads: all physical cores while running; the service runs at
  `Nice=5` so typing and the desktop stay responsive.
- If the measured final pass for 5 seconds of speech exceeds 3 s on the CPU,
  ticket 186 does not ship Breeze on the CPU by default: it uses the GPU path
  (ticket 191) where that measures faster, or offers Whisper large-v3-turbo with
  the cost stated. The page always shows where the model runs.

### 4.7 The NPU (ticket 98)

Ticket 98's findings stand, with one correction found here: whisper.cpp's NPU
path is no longer Windows-only. Its README lists Linux (Ubuntu 24.04, Python
3.12) since the VitisAI plugin merged on 2026-08-24, and it still needs AMD's
proprietary XRT plugin and FlexML runtime and a prebuilt encoder cache. Those
caches exist only for OpenAI's Whisper models; Breeze-ASR-25 would need a cache
compiled with AMD's tools under AMD's EULA. On this machine nothing can be
measured yet: `/dev/accel0` is not the person's, the firmware is 1.0.0.63
where the 7.0 driver asks for a newer one, and no runtime is installed. So:

- the NPU appears on the page as "需要 NPU 執行環境" until ticket 98 makes the
  device and runtime available;
- ticket 191 then measures Breeze (if a cache can be built) and Whisper
  large-v3-turbo on the NPU against the CPU and GPU, and moves voice there only
  where the numbers say so;
- the word-choice model (32.6 M parameters, 7–16 ms on a CPU) stays on the CPU;
  ticket 98's small-transformer measurement decides whether moving it is ever
  worth the runtime.

### 4.8 Privacy

Nothing typed or spoken leaves the machine. The only network requests are the
model downloads in section 4.9, to fixed URLs with fixed checksums, carrying no
personal data. Audio exists only in memory while listening. The capture stream
exists only while listening, and the composition shows that it is listening.

### 4.9 Models: download, storage, removal (D3, D4)

| Model | Used by | Source | Size | SHA-256 pinned in |
| --- | --- | --- | --- | --- |
| `breeze-asr-25-q5_k.bin` | voice | Better OS release asset converted from MediaTek's pinned revision (D3 a) | about 1.1 GB | `input-voice-core`'s model catalog |
| `0c-sequence-d512/model.onnx` and `vocab.json` | word choice | Hugging Face `Funmula/0c-sequence-d512`, pinned revision | 131 MB | `input-core` |
| `0c-punct-d512/model.onnx` and `vocab.json` | punctuation | Hugging Face `Funmula/0c-punct-d512`, pinned revision | 131 MB | `input-core` |
| `silero-vad` | voice | whisper.cpp's published VAD model (MIT), pinned | under 1 MB | `input-voice-core` |

- **When** (D4 a): Better Manager's install confirmation lists the package and
  the models with the total. After the package installs, `better-input-voice`
  starts (it is a user unit) and downloads what the enabled features need, one
  file at a time, resumable, verified against its SHA-256 before it is moved into
  place. Progress shows in Better Manager's Activity and on the page. Turning a
  feature off deletes its model; turning it on downloads it. If NetworkManager
  reports the connection as metered, the download waits and the page offers
  "Download now".
- **Where**: `~/.local/share/better-os/input/models/<name>/<sha256>/`, each with
  its `LICENSE` and, for Breeze, the Apache-2.0 `NOTICE` and the statement that
  Better OS converted it. A file whose hash does not match is deleted, never
  used.
- **Removal**: models are component content, not the person's data, so removing
  Better Input always deletes them, whether or not "Also delete my settings and
  history" is ticked (AGENTS.md: nothing it installed outlives it). This needs a
  second list beside `manager_core::personal_data` for downloaded component
  content (ticket 185).
- **The manifest's `paths`** names:
  `~/.local/share/better-os/input/models` (always deleted),
  `~/.local/share/better-os/input/user` (the person's words and learning: deleted
  only when the box is ticked), `~/.config/better-os/input` (settings: same).

## 5. The voice key

### 5.1 What the key sends today

On the M5406WA, Fn+F10 reaches the kernel as ASUS WMI event 0xE7.
`asus-nb-wmi`'s keymap has no entry for it on Linux master (2026-11-20), so
`sparse_keymap_report_event` reports `EV_MSC MSC_SCAN 0xe7`, then
`EV_KEY KEY_UNKNOWN 1` and `KEY_UNKNOWN 0` at once, on the "Asus WMI hotkeys"
device, and `asus-wmi` logs "Unknown key code 0xe7". hwdb cannot help:
`sparse_keymap_setkeycode` refuses a scan code that is not already in the table.
Upstream reports every code autoreleased, mapped or not, so the key never says
when it is let go.

### 5.2 Why this is not a keylogger

The device is not the keyboard: the laptop's letters come from
`platform-i8042-serio-0-event-kbd` (`/dev/input/by-path`), a different node. The
hotkeys device carries only ASUS function keys (brightness, volume, microphone
mute, the touchpad toggle 0x6B, and the like). Even so, the helper below never
forwards anything but one fact, and nothing in the person's session opens any
input node (D5).

### 5.3 The helper (D5 a)

- **udev starts it for one device only**: `91-better-input-voicekey.rules`
  matches `SUBSYSTEM=="input"`, `KERNEL=="event*"`, `ATTRS{name}=="Asus WMI
  hotkeys"`, `ENV{ID_PATH}=="platform-asus-nb-wmi"`, and starts
  `better-input-voicekey@%k.service`.
- **The unit** (as ADR 0030's pass-through): `DevicePolicy=closed`,
  `DeviceAllow=/dev/input/%I r`, nothing else; `PrivateNetwork=yes`,
  `RestrictAddressFamilies=AF_UNIX`; it opens the node with `O_NOFOLLOW`, checks
  it is an input character device whose sysfs name and phys are the ones above,
  checks the DMI product against a closed table (today one row:
  `ASUS Vivobook S 14 M5406WA`, scan code 0xE7), then drops to its own account
  `_better-input-voicekey` with no capabilities and installs a seccomp filter
  that allows only `read`, `poll`, and its bus traffic.
- **It never grabs** (`EVIOCGRAB` is refused by its own filter): the desktop
  keeps receiving every hotkey.
- **It reports one thing**: when it reads `MSC_SCAN 0xe7` followed by
  `KEY_UNKNOWN 1` in the same report, or `KEY_VOICECOMMAND 1` (after the
  upstream patch), it emits `org.betteros.VoiceKey1.Pressed` on the system bus,
  with no argument. Every other event is read and dropped in the same loop,
  never stored, never logged.
- **Who may hear it**: the system bus policy lets only members of the active
  local seat's session receive it in practice (the voice service checks
  `org.freedesktop.login1` that its session is active and unlocked before acting
  on a press); the signal carries nothing but the fact.
- **A second press** within 300 ms of the first is ignored (debounce).
- A security review, `engineering/specs/voice-key-security.md`, holds every
  claim above to a test or a check, as the touchpad review does (ticket 188), and
  an ADR records the boundary (ticket 188 writes it).

### 5.4 Without the key

Every machine gets the dictation shortcut, a GNOME custom key binding applied and
restored through Better Defaults (`global-shortcut`, ticket 187).
`<Super>h` is GNOME's minimize on the machine, so the default is
`<Super><Alt>h`, which nothing in `org.gnome.desktop.wm.keybindings` uses (the
media-keys and shell schemas are checked by ticket 187 before applying). On a
machine without the voice key, the page shows the shortcut instead of the key.

### 5.5 Press to start, press to stop; hold to talk

Fn+F10 is press-to-start and press-to-stop. Because the kernel reports its
press and release together, holding it cannot be told from pressing it; the
"Hold to talk" setting therefore uses Right Alt (D6), which the engine sees with
its press and release while a text field is focused. The page says why Fn+F10
cannot be held.

### 5.6 The upstream patch (D7)

To be tested on the M5406WA by the owner, then sent with `git send-email` to
the X86 platform drivers maintainers and the ASUS WMI maintainers listed in
`MAINTAINERS`, copying platform-driver-x86@vger.kernel.org, against the pdx86
`for-next` branch:

```diff
From: <owner's name and address>
Subject: [PATCH] platform/x86: asus-nb-wmi: map 0xE7 to KEY_VOICECOMMAND

The voice key (Fn+F10) on the ASUS Vivobook S 14 M5406WA sends WMI event
0xE7, which the keymap does not know, so asus-wmi logs
"Unknown key code 0xe7" and reports KEY_UNKNOWN. Map it to
KEY_VOICECOMMAND so desktops can bind it.

Signed-off-by: <owner's name and address>
---
 drivers/platform/x86/asus-nb-wmi.c | 1 +
 1 file changed, 1 insertion(+)

diff --git a/drivers/platform/x86/asus-nb-wmi.c b/drivers/platform/x86/asus-nb-wmi.c
--- a/drivers/platform/x86/asus-nb-wmi.c
+++ b/drivers/platform/x86/asus-nb-wmi.c
@@ -683,6 +683,7 @@ static const struct key_entry asus_nb_wmi_keymap[] = {
 	{ KE_KEY, 0xC5, { KEY_KBDILLUMDOWN } },
 	{ KE_KEY, 0xCA, { KEY_F13 } }, /* Noise cancelling on Expertbook B9 */
 	{ KE_KEY, 0xCB, { KEY_F14 } }, /* Fn+noise-cancel */
+	{ KE_KEY, 0xE7, { KEY_VOICECOMMAND } }, /* Voice key (Fn+F10) */
 	{ KE_IGNORE, 0xC6, },  /* Ambient Light Sensor notification */
 	{ KE_IGNORE, 0xCF, },	/* AC mode */
 	{ KE_KEY, 0xFA, { KEY_PROG2 } },           /* Lid flip action */
```

The hunk was produced with `diff -u` against that file; GNU diff's `-p`
truncates the function name, which `git format-patch` writes in full.

Line numbers are those of Linux master `d0f871bf` (2026-11-20); the patch is
regenerated against `for-next` when it is sent. After it lands, the key arrives
as `KEY_VOICECOMMAND` (582). The helper accepts both forms, so nothing changes
for Better Input; whether GNOME can bind a key code above 255 is not relied on.

## 6. Install, update, turn off, remove

### 6.1 One action each

| Action in Better Manager | What happens, in order, with nothing else to do |
| --- | --- |
| Install | the daemon installs `better-input` (engine, worker, voice service, voice-key helper and its udev rule, component XML); manager-core (as the person) enables and starts `better-input-voice.service`; restarts the session's IBus once (section 6.3); applies the Better Defaults entries (section 6.2) once the quality gate allows; the voice service downloads the models (section 4.9). The person was in Better Manager when they clicked, so no composition is open in any other window when IBus restarts. |
| Turn off | restores every Better Defaults entry (the captured input sources, the shortcut), stops and disables the voice unit; the engine stays installed but is no longer a source. |
| Turn on | the reverse, with the same capture rules (a value changed by hand since is not overwritten without asking). |
| Update | as AGENTS.md's rules plus section 6.5. |
| Remove | turn off first, then the daemon removes the package; IBus is restarted once so the engine leaves GNOME's list; models are deleted; the person's words and settings are deleted unless "Also delete my settings and history" is unticked. 新酷音 is the source again because its captured list is put back, and its own files were never touched. |

### 6.2 Better Defaults entries

| Entry id | Kind | Keys | Desired | Restore |
| --- | --- | --- | --- | --- |
| `input-source` | `input-method` | `org.gnome.desktop.input-sources sources`, `mru-sources` | `sources`: the captured list with `('ibus', 'chewing')` replaced by `('ibus', 'better-input')` in place, or Better Input prepended when 新酷音 is absent (D8 a); `mru-sources`: Better Input first | `captured-value`, each key to its own captured value |
| `dictation-shortcut` | `global-shortcut` | a GNOME custom binding `better-input-dictate` | `<Super><Alt>h`, command `/usr/bin/better-input-voice --toggle` | `captured-value` (removed) |

The `input-method` adapter does not exist in production yet (ADR 0009 lists it
as manual action required), and `sources` is an `a(ss)` value, which
`DefaultsValue` cannot hold (`desktop_entry`, `text`, `text_list`, `boolean`).
Ticket 184 adds an `input_sources` value type, its dconf encoding, and the
adapter, with the missing-engine rule: a restore whose captured engine is no
longer installed reports it and writes the list anyway, because GNOME skips a
source it cannot find.

Mixed typing on by default is Better Input's own setting, not a desktop setting,
so it needs no Defaults entry.

### 6.3 IBus must restart once, and why that is still one action

`ibus-daemon` reads component files only when it starts, and GNOME Shell lists
engines only when it connects (sources). A newly installed engine therefore does
not exist for GNOME until `ibus-daemon` restarts; a login would do it, but that
is the second step AGENTS.md forbids. Registering the engine at run time does not
help, because GNOME's list does not include run-time engines.

So manager-core restarts it, as the person, right after the package lands:
`systemctl --user try-restart org.freedesktop.IBus.session.GNOME.service` where
that unit exists (Ubuntu 24.04 and Zorin 18), otherwise `ibus restart`. GNOME
Shell reconnects by itself and lists the engines again. This is a new controlled
operation in `manager-platform` (the IBus unit is not Better OS's, so it is not
`UserUnitController`'s), tested against a fake `systemctl` with the
`BETTER_OS_TEST_SYSTEMCTL_GUARD`. No environment file is written: GNOME on
Wayland reaches IBus through the compositor, and the `GTK_IM_MODULE` change 0c
makes would need a login.

### 6.4 The manifest

`components/manifests/better-input.yaml`: `component_type: replacement`,
`replaces: [ibus-chewing]`, `icon: generic` (the icon set is closed), `restart:
none`, targets 22.04 and 24.04, amd64 and arm64 (the NPU parts only where ticket
98 says), lifecycle as every component, `default_integrations` as section 6.2,
health checks `input.engine_registered` (GNOME lists the engine),
`input.engine_running`, `input.voice_service`, `input.models`, `input.voicekey`
(only where the device exists), permissions (`ibus-engine`, `session-bus-name`
`org.betteros.Input1`, `system-bus-name` `org.betteros.VoiceKey1`,
`input-device-read` for the one node, `microphone`, `network-download` for the
pinned model URLs, `systemd-user-unit`), and `paths` as section 4.9 plus the
installed files. `personal_data::DATA_DIRECTORIES` gains
`.local/share/better-os/input/user` and `.config/better-os/input` for
`better-input`.

### 6.5 Running the new program after an update (D9)

`better-input-voice` is a user unit: the package's preinst and postinst from
`packaging/maintainer/user-units.sh` restart it. The engine is started by
`ibus-daemon`, so it cannot be: it watches its own executable, and when the file
is replaced it waits until no composition is open in any context, then exits.
The voice service, restarted by the postinst, sees the engine's bus name go and
calls IBus's `SetGlobalEngine("better-input")` if Better Input was the current
engine, which makes `ibus-daemon` start the new program. The worker is a child
of the engine and goes with it. Whether `ibus-daemon` would also restart the
engine by itself on the next focus is not known and is ticket 183's first test.

## 7. The control center page

A new navigation item below 預設應用程式 / Default apps: icon `Keyboard`,
label 輸入法 / Input. `better-manager --page input` opens it. (Ticket 229
proposes it as a sub-page of 時間與語言 / Time & language instead, with the
same `--page input`: decision M3 of `design/system-settings-spec.md`.) The mockups are
`design/better-input/page-*.html` and `voice-setup-*.html`.

### 7.1 Status (top of the page)

| State | How it is drawn | zh-TW | English |
| --- | --- | --- | --- |
| Not installed | the shared "not installed" card (control-center spec section 2) | 需要安裝 Better Input 才能使用這一頁。 / 安裝 | Better Input needs to be installed to use this page. / Install |
| Installed, current source | `SettingsCard`, success icon | Better Input 是目前的輸入法 · 中文和英文直接打，不用切換。 | Better Input is your input method · Type Chinese and English without switching. |
| Installed, another source is current | `SettingsCard`, accent `Button` | 目前使用的是 {source} · 改用 Better Input 後，中英文不用切換。 / 改用 Better Input | You're using {source} · With Better Input you don't switch between Chinese and English. / Use Better Input |
| Quality gate not met (section 3.7) | `InfoBar` Informational | 這一版的選字還沒有比新酷音準，所以先不取代它。你可以先在這裡試用。 | This version doesn't choose words better than Chewing yet, so it isn't replacing it. You can try it here. |
| IBus not running | `InfoBar` Warning, cards disabled, `Button` | 輸入法框架沒有在執行，現在無法打字。 / 重新啟動輸入法 | The input method framework isn't running, so typing won't work right now. / Restart input method |
| Engine missing from GNOME's list (needs the one restart) | `InfoBar` Warning, `Button` | Better Input 已安裝，但輸入法框架還沒載入它。 / 重新載入 | Better Input is installed, but the input method framework hasn't loaded it yet. / Reload |

### 7.2 Group 打字 / Typing

| Card | Control | zh-TW (title · description) | English (title · description) | Default |
| --- | --- | --- | --- | --- |
| Mixed | `ToggleSwitch` | 中英混打 · 直接打英文單字，Better Input 會分辨哪些是中文、哪些是英文。 | Type English without switching · Type English words directly; Better Input tells Chinese from English. | on |
| Mixed, while toneless is on | disabled switch, Caption | 不打聲調開著時無法使用。 | Not available while tones are optional. | — |
| Chinese–English key | `ComboBox` | 中英切換鍵 · 單按一下切換成全英文。 · 左右 Shift / 左 Shift / 右 Shift / 不使用 | Chinese–English key · Tap once to switch to English only. · Either Shift / Left Shift / Right Shift / None | from 新酷音 (`shift`: either Shift) |
| Space between | `ToggleSwitch` | 中英文之間加空格 · 送出時在中文和英文字中間補一個半形空格。 | Add a space between Chinese and English · Adds a space where Chinese meets English when you commit. | off |
| Toneless | `ToggleSwitch` | 不打聲調 · 聲調可打可不打。開啟後中英混打會關閉。 | Tones optional · Type with or without tones. Turning this on turns off typing English without switching. | off |
| Similar sounds | `SettingsExpander` with four `CheckBox` rows | 模糊音 · 把近音的字也列進候選。 · ㄗ 和 ㄓ / ㄘ 和 ㄔ / ㄙ 和 ㄕ / ㄣ 和 ㄥ | Similar sounds · Also list words with similar sounds. · ㄗ and ㄓ / ㄘ and ㄔ / ㄙ and ㄕ / ㄣ and ㄥ | all on |
| Smart choice | `ToggleSwitch` | 依前文選字 · 用這台電腦上的語言模型，看你前面打的字來選字。 | Choose words from context · Uses a language model on this PC to pick words from what you've written. | on |
| Smart choice, model state | Caption under it | 模型下載中… {percent}% / 模型無法下載，暫時用一般選字。 / 重試 | Downloading the model… {percent}% / The model couldn't be downloaded, so standard word choice is used for now. / Try again | — |
| Tab | `ToggleSwitch` | Tab 補完 · 有把握時提示後面的字詞，按 Tab 接受。 | Tab completion · Suggests what comes next when it's sure; press Tab to accept. | on |
| Punctuate on Enter | `ToggleSwitch` | 送出時補標點 · 按 Enter 送出時，在中文句子裡補上，。？！，不改你打的字。 | Add punctuation on Enter · When you press Enter, adds ，。？！ to Chinese sentences without changing your words. | off |
| Layout | `ComboBox` | 鍵盤排列 · 標準 / 倚天 / 倚天 26 鍵 / 許氏 / IBM / 精業 / 漢語拼音 / 大千 26 鍵 | Keyboard layout · Standard / Eten / Eten 26 / Hsu / IBM / Gin-Yieh / Hanyu Pinyin / Dachen 26 | from 新酷音 |
| Selection keys | `ComboBox` | 選字鍵 · 1234567890 / asdfghjkl; / 1234qweras | Selection keys · 1234567890 / asdfghjkl; / 1234qweras | from 新酷音 |
| Per page | `ComboBox` | 每頁候選字數 · 5 / 7 / 9 / 10 | Candidates per page · 5 / 7 / 9 / 10 | from 新酷音 (5) |
| Space selects | `ToggleSwitch` | 空白鍵選字 · 按空白鍵打開候選字。 | Space opens candidates · Press Space to see candidates. | from 新酷音 (off) |
| Space selects, while mixed is on | disabled, Caption | 中英混打開著時，空白鍵一律是空格，選字請按 ↓。 | While typing English without switching, Space always types a space; press ↓ to choose. | — |

The layouts are libchewing's; ticket 180 confirms the list and its zh-TW names
against the crate.

### 7.3 Group 個人化 / Personal

| Card | Control | zh-TW | English | Default |
| --- | --- | --- | --- | --- |
| Learn | `ToggleSwitch` | 記住我選的字詞 · 下次同樣的情況優先出現。只存在這台電腦上。 | Learn from what I choose · Your choices come first next time. Kept on this PC only. | on |
| My words | card with chevron → sub-page | 我的詞彙 · {n} 個詞 | My words · {n} words | — |
| Imported | Caption under My words, once | 已從新酷音匯入 {n} 個詞。 | Imported {n} words from Chewing. | — |
| Clear learned | `Button` with a `ContentDialog` | 清除學到的字詞… · 這會刪掉 Better Input 學到的選字習慣，你自己加的詞會留著。 / 清除 / 取消 | Clear what Better Input learned… · This deletes the choices Better Input learned. Words you added stay. / Clear / Cancel | — |

Sub-page 輸入法 › 我的詞彙 / Input › My words: a list (word, reading, added or
learned), a search box (尋找詞彙 / Find a word), 新增詞彙 / Add word (a dialog with
詞 / Word and 注音 / Zhuyin, the reading filled from the lexicon, 新增 / Add),
移除 / Remove on each row, 匯出… / Export…, 匯入… / Import…, and the empty state
還沒有自己加的詞。打字時按 Shift+← 選起來，再按 Enter 就能加入。 / No words of
your own yet. While typing, select with Shift+← and press Enter to add one.

### 7.4 Group 語音輸入 / Voice input

| Card | Control | zh-TW | English | Default |
| --- | --- | --- | --- | --- |
| Voice | `ToggleSwitch` | 語音輸入 · 按 {key} 開始說話，再按一下停止。說的話會出現在組字區，可以直接用鍵盤修改。全部在這台電腦上處理。 | Voice typing · Press {key} to start talking and again to stop. What you say appears in the composition, where you can fix it by typing. Everything stays on this PC. | on |
| Model | `SettingsCard`, value | 語音模型 · Breeze ASR 25（台灣華語，聽得懂中英夾雜）· {size} | Speech model · Breeze ASR 25 (Taiwanese Mandarin, understands mixed English) · {size} | — |
| Model downloading | `ProgressBar` in the card | 正在下載… {done} / {total} | Downloading… {done} of {total} | — |
| Model waiting (metered) | Caption, `Button` | 目前的網路有流量限制，連上一般網路後會自動下載。 / 現在下載 | You're on a metered connection. It'll download on a regular network. / Download now | — |
| Model failed | `InfoBar` Error, `Button` | 語音模型沒有下載成功。{reason} / 重試 | The speech model didn't download. {reason} / Try again | — |
| Model corrupt | `InfoBar` Error, `Button` | 下載的檔案不完整，已經刪除。 / 重新下載 | The downloaded file was incomplete and has been deleted. / Download again | — |
| Not enough space | `InfoBar` Warning | 磁碟空間不夠，語音模型需要 {size}。 | There isn't enough disk space; the speech model needs {size}. | — |
| Voice key | `SettingsCard`, `Button` | 語音鍵 · Fn+F10（這台電腦的語音鍵） / 測試 | Voice key · Fn+F10 (this PC's voice key) / Test | — |
| Voice key test | the card's description while testing | 請按一下語音鍵… / 收到了。 / 10 秒內沒有收到，語音鍵的背景服務可能沒有在執行。 | Press the voice key… / Got it. / Nothing arrived in 10 seconds; the voice key service may not be running. | — |
| No voice key | the card's description | 這台電腦沒有語音鍵，請用下面的快速鍵。 | This PC has no voice key; use the shortcut below. | — |
| Shortcut | `SettingsCard`, shortcut picker | 快速鍵 · 在任何文字欄位開始或停止語音輸入。 · Super+Alt+H | Shortcut · Starts or stops voice typing in any text field. · Super+Alt+H | `<Super><Alt>h` |
| Hold to talk | `ToggleSwitch` | 按住說話 · 按住右 Alt 說話，放開就停止。 | Hold to talk · Hold Right Alt to talk and release it to stop. | off |
| Hold to talk, why not Fn+F10 | Caption | Fn+F10 放開的時候鍵盤不會通知系統，所以按住說話要用右 Alt。 | The keyboard doesn't report when Fn+F10 is released, so hold to talk uses Right Alt. | — |
| Punctuation | `ToggleSwitch` | 自動加標點 · 依語氣補上，。？！ | Add punctuation automatically · Adds ，。？！ from how you speak. | on |
| Spoken symbols | `ToggleSwitch` | 說出標點符號 · 說「逗號」「句號」「問號」「換行」就會打出符號。 | Spoken punctuation · Say 逗號, 句號, 問號, or 換行 to type the symbol. | on |
| Stop after silence | `ComboBox` | 安靜多久後停止 · 5 秒 / 10 秒 / 30 秒 / 不自動停止 | Stop after silence · 5 seconds / 10 seconds / 30 seconds / Don't stop automatically | 10 秒 |
| Microphone | `ComboBox` | 麥克風 · 系統預設（{name}）/ {each source} | Microphone · System default ({name}) / {each source} | system default |
| No microphone | the card disabled, description | 找不到麥克風。 | No microphone found. | — |
| Runs on | `SettingsCard`, value or `ComboBox` | 執行位置 · 自動（{where}）/ 處理器 / 顯示晶片 / NPU | Runs on · Automatic ({where}) / Processor / Graphics / NPU | automatic |
| NPU unavailable | the NPU entry disabled, Caption | NPU 需要另外的執行環境，目前還不能用。 | The NPU needs an extra runtime and can't be used yet. | — |
| Voice service not running | `InfoBar` Warning, `Button` | 語音輸入的背景服務沒有在執行。 / 啟動服務 | The voice typing service isn't running. / Start service | — |
| Voice off | the cards below the switch are hidden, Caption | 關閉後會刪除語音模型（{size}），之後再開啟會重新下載。 | Turning this off deletes the speech model ({size}); turning it back on downloads it again. | — |

`{key}` is 語音鍵 Fn+F10 / the voice key (Fn+F10) where the helper reports the
device, otherwise the shortcut. `{where}` is 處理器 / processor, 顯示晶片 /
graphics, or NPU, whichever ticket 186 or 191 measured best.

### 7.5 Group 關於 / About

| Card | zh-TW | English |
| --- | --- | --- |
| About (expander) | 關於 Better Input · 版本 {version} · 新酷音引擎 libchewing（LGPL-2.1）· 選字與標點模型 0c D512（MIT，Funmula）· 語音模型 Breeze ASR 25（Apache-2.0，MediaTek Research）· whisper.cpp（MIT）· 第三方授權 | About Better Input · Version {version} · libchewing engine (LGPL-2.1) · 0c D512 word-choice and punctuation models (MIT, Funmula) · Breeze ASR 25 speech model (Apache-2.0, MediaTek Research) · whisper.cpp (MIT) · Third-party licenses |
| Privacy line | 打字和說話都只在這台電腦上處理，不會上傳。只有下載模型時會連網。 | Typing and speech are processed on this PC only and never uploaded. Only model downloads use the network. |

### 7.6 Outside the page

| Where | When | zh-TW | English |
| --- | --- | --- | --- |
| Auxiliary text (GNOME's candidate popup) | listening | 🎙 正在聽… {tentative} · 再按 {key} 停止 · Esc 取消 | 🎙 Listening… {tentative} · Press {key} to stop · Esc to cancel |
| Auxiliary text | final pass running | 🎙 正在整理… | 🎙 Finishing… |
| Auxiliary text | Tab suggestion | Tab ⇥ {text} | Tab ⇥ {text} |
| Engine property symbol (GNOME's indicator, ticket 157's bar) | Chinese / English only / listening | 中 / 英 / 聽 | 中 / 英 / 聽 (the symbol is not translated) |
| Notification | voice key with no text field focused | 先點一下要輸入文字的地方，再按語音鍵。 | Click where you want to type, then press the voice key. |
| Notification | voice key in a password field | 密碼欄位不能用語音輸入。 | Voice typing isn't available in password fields. |
| Notification | model not downloaded yet | 語音模型還在下載（{percent}%），好了就能用。 | The speech model is still downloading ({percent}%). Voice typing will work when it's done. |
| Notification | microphone failed | 無法使用麥克風：{reason} | Couldn't use the microphone: {reason} |

GNOME Shell caps an engine symbol at under three characters (0c's IBus README),
which 中, 英 and 聽 meet.

## 8. Copy that is new to the kit

| Key | zh-TW | English |
| --- | --- | --- |
| nav_input | 輸入法 | Input |
| listening | 正在聽… | Listening… |
| finishing | 正在整理… | Finishing… |
| download_now | 現在下載 | Download now |
| test | 測試 | Test |

## 9. Not verified, and owned by a ticket

| What | Owner |
| --- | --- |
| Whether `ibus-daemon` restarts an engine that exited on the next focus by itself, or needs `SetGlobalEngine` | 183, first step, in a headless GNOME Shell |
| That `systemctl --user try-restart org.freedesktop.IBus.session.GNOME.service` makes GNOME Shell 46 list a new engine without a login, and what Ubuntu 22.04 (IBus 1.5.26, GNOME 42) needs instead | 185, in a disposable VM per release |
| Whether Mutter 46's text-input-v3 is version 2 (styled preedit) | 183 |
| Better Input's first-choice accuracy against libchewing and 0c | 181 (the quality gate) |
| Breeze-ASR-25's real latency, CPU time, and memory on the HX 370's CPU and on the 890M through Vulkan, at the thread counts and `audio_ctx` sizes of section 4.6 (a CPU benchmark runs in a container with no host change) | 186, 191 |
| That the press of Fn+F10 appears on `event8` as `MSC_SCAN 0xe7` then `KEY_UNKNOWN 1`/`0` in one report (read from the kernel source, not from the device) | 188, with the owner pressing the key while a test build of the helper's reader runs in a container given that one node read-only, or by the owner with `evtest` |
| That Fn+F10 sends nothing on release (no second WMI code) | 188, same check |
| The libchewing layouts' names and that `chewing` 0.13 reads `~/.chewing/chewing.sqlite3` | 180 |
| The English word list and its licence | 182 |
| Whether an NPU cache can be built for Breeze-ASR-25, and the NPU's numbers | 191, after ticket 98 |
| Dictation, typing, install, and removal on the owner's desktop | 185 and 190, after release |
