# Copyleft Dependency Review

This is an engineering review of the copyleft and weak-copyleft code that the
Better OS packages carry or load. It is not legal advice. It records what the
locked dependency graph and the published packages contain, what each license
asks of a project that distributes binaries under GPL-3.0-or-later
([ADR 0003](decisions/0003-project-license.md)),
and where the current packages do or do not meet that. Anything that needs a
judgement the project owner has not made is listed under
[Decisions for the owner](#decisions-for-the-owner) and left open here.

Reviewed on 2026-09-27 against:

- `Cargo.lock` with SHA-256
  `cbc060c207a20377c6a1fca7c1e54f3f98f336af0e5b827f934ec0fdd900f755`, the same
  hash `docs/third-party-licenses.md` pins (922 packages).
- The 32 `.deb` files published as
  [`v0.2.8`](https://github.com/TimLai666/better-os/releases/tag/v0.2.8), built
  from merge commit `6e5737e793d6392ae0d677b92f07b6856ce3f9d5`. They were
  unpacked into a scratch directory and read, not installed.

## Summary

- Five third-party Rust packages with a copyleft license, or a copyleft
  alternative, are linked into shipped binaries: `zlog`, `ztracing`, and
  `ztracing_macro` (GPL-3.0-or-later), `option-ext` (MPL-2.0), and `self_cell`
  (Apache-2.0 OR GPL-2.0-only).
- All five reach the binaries only through GPUI, so only the six windows carry
  them: `better-manager`, `better-monitor`, `better-launcher`, `better-files`,
  `better-touchpad`, and `awake-gui`. `better-storage`,
  `better-manager-daemon`, and every command-line tool, service, and tray
  binary carry none of them.
- Four more copyleft records in the lockfile are never built for Linux:
  `r-efi` 5.3.0 and 6.0.0 (UEFI only), `cbindgen` (a macOS build tool), and
  `dwrote` (Windows only).
- One copyleft component does not appear in the Cargo metadata at all. The
  Ubuntu 22.04 packages of the six windows link FreeType 2.13.2, which
  `freetype-sys` compiles from its own bundled copy. FreeType is licensed
  FTL OR GPL-2.0-or-later, and neither the inventory nor any package mentions
  it.
- At run time the binaries load glibc (LGPL-2.1-or-later) and libgcc
  (GPL-3.0-or-later with the GCC Runtime Library Exception), and the Ubuntu
  24.04 arm64 windows load the system FreeType. These are system libraries the
  packages do not redistribute, and they add no obligation.
- Four items need the owner's decision. Two of them are gaps in the current
  packages: no package carries the Apache-2.0 text that `self_cell` requires,
  and nothing records which of FreeType's two licenses the 22.04 packages rely
  on.

## How this was checked

Every package and license expression below comes from these commands, run in
the repository root, and not from memory:

```sh
# The locked graph and each package's declared license.
cargo metadata --format-version 1 --locked --offline

# Which shipped crate reaches a package, with features unified the way
# packaging/build-deb.sh builds them (one `cargo build` over all fifteen crates).
cargo tree -e normal -i <crate>@<version> --locked --offline --target <triple> \
    -p manager-gui -p manager-cli -p manager-daemon -p monitor-gui \
    -p monitor-service -p monitor-cli -p launcher-gui -p files-gui \
    -p touchpad-gui -p touchpad-gesture-service -p awake-service \
    -p awake-tray -p awake-gui -p storage-service -p storage-platform

# Where a package that is not built for Linux comes from.
cargo tree -e all -i <crate>@<version> --locked --offline --workspace --target all
```

`<triple>` was run for both `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu`, and both gave the same answer for every package.
The copyleft filter matched GPL, LGPL, AGPL, MPL, EPL, CDDL, EUPL, OSL, CPL,
CeCILL, SSPL, and RPL in each package's license field, plus packages with no
license field. `NCSA` and `bzip2-1.0.6`, which the inventory's "Review focus"
table also lists, are permissive and are not covered here.

The published packages were read with `dpkg-deb -f` (control fields),
`dpkg-deb -x` into a scratch directory, `readelf -d` (shared libraries each
binary asks the loader for), and `readelf -s` (the symbol table, which also
names every object file that went into the link). Upstream license files were
read from the Cargo registry and git checkouts under `~/.cargo`, and system
library licenses from `/usr/share/doc/<package>/copyright` on the review host.

## What each package ships

`packaging/build-deb.sh` builds eight packages. This table maps each installed
binary to the Cargo crate that produces it, and marks the binaries that link
GPUI, which is where every shipped copyleft package comes from.

| Package | Installed binary | Cargo crate | Links GPUI |
| --- | --- | --- | --- |
| `better-manager` | `/usr/bin/better-manager` | `manager-gui` | yes |
| `better-manager` | `/usr/bin/better-manager-cli` | `manager-cli` | no |
| `better-monitor` | `/usr/bin/better-monitor` | `monitor-gui` | yes |
| `better-monitor` | `/usr/bin/better-monitor-service` | `monitor-service` | no |
| `better-monitor` | `/usr/bin/better-monitor-cli` | `monitor-cli` | no |
| `better-launcher` | `/usr/bin/better-launcher` | `launcher-gui` | yes |
| `better-files` | `/usr/bin/better-files` | `files-gui` | yes |
| `better-touchpad` | `/usr/bin/better-touchpad` | `touchpad-gui` | yes |
| `better-touchpad` | `/usr/bin/better-touchpad-gestured` | `touchpad-gesture-service` | no |
| `better-awake` | `/usr/bin/awake-gui` | `awake-gui` | yes |
| `better-awake` | `/usr/bin/awake-tray` | `awake-tray` | no |
| `better-awake` | `/usr/bin/better-awake-service` | `awake-service` | no |
| `better-storage` | `/usr/bin/better-storage-service` | `storage-service` | no |
| `better-storage` | `/usr/bin/better-storage-doctor` | `storage-platform` | no |
| `better-manager-daemon` | `/usr/libexec/better-manager-daemon` | `manager-daemon` | no |

Every package installs two license documents under `/usr/share/doc/<package>/`
(`packaging/build-deb.sh` lines 201–203 and 311–313):

- `copyright`, which is the repository's `LICENSE`, the full GPL-3.0 text.
- `THIRD-PARTY-LICENSES.md`, which is `docs/third-party-licenses.md`: every
  locked package with its declared license and a link to its source. The copy
  in the v0.2.8 packages is byte-identical to the file at this commit.

No package carries any other license text, and none has a `Homepage` field or
any other pointer to the Better OS source repository.

## Copyleft packages in the locked graph

| Package | Version | License expression | Shipped in | How it is linked | Obligation met |
| --- | --- | --- | --- | --- | --- |
| `zlog` | `0.1.0` | `GPL-3.0-or-later` | the six windows | static, Rust | yes, see Decision 3 |
| `ztracing` | `0.1.0` | `GPL-3.0-or-later` | the six windows | static, Rust | yes, see Decision 3 |
| `ztracing_macro` | `0.1.0` | `GPL-3.0-or-later` | the six windows | procedural macro, runs in the compiler | yes, see Decision 3 |
| `option-ext` | `0.2.0` | `MPL-2.0` | the six windows | static, Rust | yes, see Decision 3 |
| `self_cell` | `1.3.0` | `Apache-2.0 OR GPL-2.0-only` | the six windows | static, Rust | no, see Decision 2 |
| `r-efi` | `5.3.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | none | not built for Linux | nothing to meet |
| `r-efi` | `6.0.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | none | not built for Linux | nothing to meet |
| `cbindgen` | `0.28.0` | `MPL-2.0` | none | macOS build tool only | nothing to meet |
| `dwrote` | `0.11.5` | `MPL-2.0` | none | Windows only | nothing to meet |
| `gpui_shared_string` | `0.1.0` | none declared | the six windows | static, Rust | unclear, see Decision 4 |
| `gpui_util` | `0.1.0` | none declared | the six windows | static, Rust | unclear, see Decision 4 |

"The six windows" means the GPUI binaries in `better-manager`,
`better-monitor`, `better-launcher`, `better-files`, `better-touchpad`, and
`better-awake`, on all four targets. "Static, Rust" means Cargo compiles the
crate into an rlib and links it into the executable. Nothing in the graph builds
a Rust crate as a shared library.

The 52 workspace crates are also `GPL-3.0-or-later`. They are Better OS's own
code, and ADR 0003 covers them.

### `zlog`, `ztracing`, and `ztracing_macro` 0.1.0: GPL-3.0-or-later

These come from the Zed repository at commit `ae394f3`. The chain is
`gpui` → `sum_tree` → `ztracing` → `zlog`, and `ztracing` → `ztracing_macro`.
`sum_tree` uses `zlog` only as a dev-dependency, so `ztracing` is what pulls
it into a release build.

With the `ztracing` cfg flag unset, which is how Better OS builds, `ztracing`
turns its span macros into empty expressions, and `ztracing_macro::instrument`
returns the function it annotates unchanged. The v0.2.8 window binaries contain
no symbol and no object file from `zlog`, `ztracing`, or `option-ext`. That was
checked in every window binary on all four targets. This review still treats
all three as linked, because the licenses apply to what the build combines and
not only to what survives optimization.

- **Obligation.** They carry the same license as Better OS, so the combined
  window binary is conveyed under GPL-3.0-or-later and they add nothing new. As
  for the rest of the binary, GPL-3.0 section 4 requires a copy of the license,
  and section 6 requires the Corresponding Source to be offered with the object
  code. Section 1 defines the Corresponding Source as the source needed to
  build and run the binary, which includes the source of every statically
  linked library.
- **Status.** The license text ships as `/usr/share/doc/<package>/copyright`.
  The packages are offered from the GitHub release, and the same repository
  serves the source at tag `v0.2.8`. GitHub's release API lists a tarball and a
  zipball for that tag, and the release notes name the merge commit. That is an
  offer from the same place under section 6(d). The source of these three
  crates lives on a third party's server, the Zed repository, and
  `THIRD-PARTY-LICENSES.md` names that repository and the pinned commit.
  Section 6(d) allows this, but it leaves the obligation to keep that source
  available with Better OS. See Decision 3.

### `option-ext` 0.2.0: MPL-2.0

The chain is `zed-font-kit` → `dirs` 6.0.0 → `dirs-sys` 0.5.0 → `option-ext`.
MPL-2.0 is copyleft at the file level: it covers the crate's two source files
and nothing it is combined with. Better OS uses those files unmodified.

- **Obligation.** Section 3.2(a) requires that recipients of the executable be
  told how to obtain the MPL-covered source. Section 3.3 allows the executable
  to be distributed as part of a larger work under another license, and permits
  GPL terms for the covered files unless they carry the "Incompatible With
  Secondary Licenses" notice. The crate's `LICENSE.txt` contains that notice
  only as the license's own blank template (Exhibit B), and neither
  `src/lib.rs` nor `src/impl.rs` carries it, so the combination with GPL code
  is allowed.
- **Status.** `THIRD-PARTY-LICENSES.md` lists `option-ext` 0.2.0 as `MPL-2.0`
  with a link to `crates.io/crates/option-ext/0.2.0`, where the source can be
  downloaded. That is a reasonable way to tell recipients where to get the
  source, although the file never says in so many words that it is the source
  offer. It depends on crates.io keeping the crate available. See Decision 3.

### `self_cell` 1.3.0: Apache-2.0 OR GPL-2.0-only

The chain is `gpui_wgpu` → `cosmic-text` 0.19.0 → `self_cell`. The crate's own
object file and two of its symbols are present in every v0.2.8 window binary.

- **Which branch.** Better OS can only use the Apache-2.0 branch. The FSF's
  [license list](https://www.gnu.org/licenses/license-list.html#GPLv2) states
  that GPLv2 on its own is not compatible with GPLv3, and a GPL-2.0-only grant
  gives no route to the later version. The same list states that
  [Apache-2.0](https://www.gnu.org/licenses/license-list.html#apache2) is
  compatible with GPLv3. The project has not recorded this choice anywhere:
  the inventory says Better OS "does not relicense or silently select a
  different expression", and nothing else says which branch it relies on.
- **Obligation under Apache-2.0.** Section 4(a) requires that recipients of
  the work, in source or object form, be given a copy of the Apache License.
  Section 4(d) requires a NOTICE file to be passed on if the work has one.
  `self_cell` 1.3.0 has none: its published crate contains only
  `LICENSE-APACHE`, `LICENSE-GPLv2`, the README, and source.
- **Status.** Not met. No package contains the Apache-2.0 text. See Decision 2.

### `r-efi` 5.3.0 and 6.0.0: MIT OR Apache-2.0 OR LGPL-2.1-or-later

`getrandom` 0.3.4 and 0.4.3 depend on them only under
`cfg(all(target_os = "uefi", getrandom_backend = "efi_rng"))`. Better OS never
builds for UEFI, so neither version is compiled, and `cargo tree` finds neither
on the two Linux targets. If they were ever built, the MIT or Apache-2.0 branch
would avoid the LGPL entirely. Nothing needs to be done.

### `cbindgen` 0.28.0 and `dwrote` 0.11.5: MPL-2.0

`cbindgen` is a build dependency of `gpui` and `gpui_macos` under
`cfg(target_os = "macos")`. It is a code generator that runs during the build,
it is never linked into a binary, and it is not built at all on Linux.
`dwrote` is a dependency of `zed-font-kit` under
`cfg(target_family = "windows")`. Neither reaches a Linux package. Both would
need another look if Better OS ever ships for macOS or Windows.

## Copyleft code that Cargo metadata does not show

A `-sys` crate can compile C source it carries inside itself, and the license
of that C code is not in the crate's license field. Nine packages in the
shipped set have a build script that compiles native code or looks for a system
library through `pkg-config`, and each was checked. One of them carries
copyleft code into a package.

### FreeType 2.13.2, bundled in `freetype-sys` 0.20.1

`freetype-sys` declares `MIT`, and that covers only its Rust bindings. The
crate also contains the FreeType 2.13.2 source in `freetype2/`, whose
`LICENSE.TXT` offers two mutually exclusive licenses: the FreeType License
(`docs/FTL.TXT`) or GPL-2.0 "(any later version can be used also)"
(`docs/GPLv2.TXT`). `LICENSE.TXT` states that the FreeType License is
compatible with GPL-3.0, and so does the FSF's
[license list](https://www.gnu.org/licenses/license-list.html#freetype). The
chain is `gpui_wgpu` → `zed-font-kit` → `freetype-sys`, so only the six windows
are affected.

`freetype-sys`'s `build.rs` (lines 16–24) asks `pkg-config` for a system
FreeType of at least version 24.3.18, which FreeType's `docs/VERSIONS.TXT` maps
to release 2.12.1. When the system copy is older or missing, the build script
compiles the bundled copy with the C++ compiler and links it statically. The
v0.2.8 packages show both outcomes:

| Target | FreeType in the six window binaries | Evidence |
| --- | --- | --- |
| Ubuntu 22.04 amd64 | bundled, static | FreeType object files (`ftbase.c`, `truetype.c`, `sfnt.c`, and others) in the symbol table, no `libfreetype.so.6` in the dynamic section, no `libfreetype6` in `Depends` |
| Ubuntu 22.04 arm64 | bundled, static | same as 22.04 amd64 |
| Ubuntu 24.04 amd64 | not linked | no FreeType object file and no `libfreetype.so.6` |
| Ubuntu 24.04 arm64 | system library, dynamic | `libfreetype.so.6` in the dynamic section and `libfreetype6 (>= 2.2.1)` in `Depends` |

The most likely reason for the split is the build host: Ubuntu 22.04's system
FreeType is older than 2.12.1 and Ubuntu 24.04's is newer. The 22.04 version
number was not checked on a 22.04 machine for this review. The symbol tables
name the FreeType object files but no FreeType function, so it is possible that
the linker removed all FreeType code as unused. This review treats the 22.04
window binaries as containing FreeType, because its object files were part of
the link.

- **Obligation, FreeType License branch.** Section 2 of `FTL.TXT` requires a
  binary distribution to state in its documentation that the software is based
  in part on the work of the FreeType Team. `FTL.TXT` also suggests a one-line
  credit naming the FreeType Project, its web address, and the copyright year
  of the version used, which is 2023 for 2.13.2.
- **Obligation, GPL branch.** Using FreeType under GPL-2.0-or-later, and
  therefore under GPL-3.0 with the rest of the binary, adds nothing beyond the
  GPL obligations the windows already carry. Its source is inside the
  `freetype-sys` crate on crates.io. The BDF and PCF drivers in the same source
  tree carry an MIT-style license (`src/bdf/README`, `src/pcf/README`) that asks
  for their copyright and permission notice to go with every copy, whichever
  branch is chosen.
- **Status.** The FreeType License credit is not in any package, and nothing
  records a choice of the GPL branch. `THIRD-PARTY-LICENSES.md` describes
  `freetype-sys` as `MIT` and does not mention FreeType, because the generator
  reads only Cargo metadata. See Decision 1.

The Ubuntu 24.04 arm64 windows load the system `libfreetype6` at run time.
Better OS does not redistribute that library, so that case adds no obligation.

The other eight were checked the same way. `ring` compiles its own C and
assembly, which is ISC-licensed with some BoringSSL code under Apache-2.0.
`psm` and `stacker` (MIT OR Apache-2.0) assemble their own stack-switching
code, and `wayland-backend` (MIT) compiles two small log shims of its own.
`wayland-sys`, `khronos-egl`, and `yeslogic-fontconfig-sys` only look up a
system library, and `gpui`'s build script does native work only for macOS and
Windows. None of them brings in copyleft code.

Fonts were checked too. The Zed checkout bundles IBM Plex Sans and Lilex under
the SIL Open Font License, but only its tests embed them
(`crates/gpui/src/svg_renderer.rs` and
`crates/gpui_wgpu/src/cosmic_text_system.rs`, both under `#[cfg(test)]`). The
v0.2.8 binaries contain the two family names and none of the font data.

## Libraries loaded at run time

The `Depends` fields come from `dpkg-shlibdeps` plus the names
`packaging/build-deb.sh` adds by hand. They were read from the published v0.2.8
packages. Licenses come from each Debian package's `copyright` file.

| Library or program | Debian package | License | Used by | How | Obligation for Better OS |
| --- | --- | --- | --- | --- | --- |
| glibc (`libc.so.6`, `libm.so.6`) | `libc6` | LGPL-2.1-or-later | all eight packages | shared library | None. It is a system library under GPL-3.0 section 1 and is not shipped. The dynamic link leaves users free to replace it, which is what the LGPL asks of a program that uses it. |
| `libgcc_s.so.1` | `libgcc-s1` | GPL-3.0-or-later with the GCC Runtime Library Exception | all eight packages | shared library | None. The exception covers programs linked against it, and Better OS is GPL-3.0 anyway. |
| `libfreetype.so.6` | `libfreetype6` | FreeType License OR GPL-2.0-or-later | the six windows, Ubuntu 24.04 arm64 only | shared library | None. It is not redistributed. |
| D-Bus bus | `dbus` | AFL-2.1 OR GPL-2.0-or-later, with LGPL-2.1-or-later parts | `better-storage`, `better-manager-daemon` | separate program reached over D-Bus; `zbus` is pure Rust and links no D-Bus library | None. |
| polkit | `policykit-1` or `polkitd` | LGPL-2.0-or-later | `better-manager-daemon` | separate service reached over D-Bus | None. |
| UDisks2 | `udisks2` | GPL-2.0-or-later, LGPL-2.0-or-later | `better-storage` (Depends), `better-files` (Recommends) | separate service reached over D-Bus | None. |
| APT and dpkg | `apt`, `dpkg` | GPL-2.0-or-later, a few APT files GPL-2.0-only | `better-manager-daemon` | separate programs run as child processes (`crates/manager-daemon/src/apt.rs` line 223, `crates/manager-daemon/src/host.rs` line 35) | None. Running a program is not linking with it. |

The windows also use permissive libraries: `libxcb1` and `libxkbcommon0`
(both linked, MIT), `libxkbcommon-x11-0`, `libfontconfig1`,
`libwayland-client0`, `libwayland-egl1`, and `libwayland-cursor0`. They also
open `libEGL.so.1` and `libvulkan.so.1` at run time with no `Depends` entry.
On the review host these come from `libegl1` (MIT library code; its GPL-3+
entries cover build macros and documentation only) and `libvulkan1` (MIT and
Apache-2.0). None of them is copyleft.

## Decisions for the owner

These are left open. Each says what is unresolved, what the options are, and
which one this review would recommend.

1. **FreeType in the Ubuntu 22.04 window packages.** The v0.2.8 packages for
   Ubuntu 22.04 contain FreeType 2.13.2 without saying so, and the project has
   not chosen which of FreeType's two licenses it uses.
   - Use FreeType under GPL-2.0-or-later, which becomes GPL-3.0 inside the
     combined binary. This needs no new text in the packages, only a written
     record of the choice. FreeType's own `LICENSE.TXT` describes this branch
     as the one for programs that already use the GPL.
   - Or use the FreeType License and add the credit line to the packages'
     documentation.
   - In either case, the inventory should mention FreeType and the BDF and PCF
     notices. Stopping the 22.04 build from bundling FreeType is a third
     option, and this review did not look into what it would take.

   Recommended: record the GPL branch, and add FreeType to the shipped notices.

2. **The `self_cell` license choice and the Apache-2.0 text.** Better OS can
   only use `self_cell` under Apache-2.0, but that choice is not written down,
   and no package contains the Apache-2.0 text that section 4(a) requires. The
   same missing text affects every other Apache-2.0 package linked into the
   binaries, GPUI included. See [A related gap](#a-related-gap-outside-copyleft).
   Recommended: record the Apache-2.0 choice for `self_cell`, and ship the
   Apache-2.0 license text in every package.

3. **Where the Corresponding Source for dependencies lives.** The GPL-3.0
   source offer, and the MPL-2.0 source requirement for `option-ext`, rely on
   the GitHub tag for Better OS's own code, and on crates.io and upstream GitHub
   repositories (Zed, font-kit, gpui-component, and four smaller Zed forks) for
   the dependencies. GPL-3.0 section 6(d) allows sources on a third party's
   server, but it keeps Better OS responsible for their staying available.
   - Keep relying on crates.io and the upstream repositories. This costs
     nothing, and the risk is an upstream commit or crate disappearing.
   - Or attach a vendored source archive (for example from `cargo vendor`) to
     each GitHub release.

   Either way, a package could name the source repository, for example through
   a `Homepage` field or one line in the doc directory. At the moment no file
   inside a package says where the Better OS source is. Recommended: attach the
   vendored archive to each release, and add the source pointer.

4. **`gpui_shared_string` and `gpui_util` have no license field.** Both are Zed
   crates pinned at `ae394f3` and are linked into the six windows. Each crate
   directory holds only a `LICENSE-APACHE` link. The Zed crates checked for
   comparison in the same checkout all have a link that matches their declared
   license: `gpui`, `gpui_linux`, `gpui_wgpu`, `sum_tree`, and `collections`
   declare Apache-2.0 and link `LICENSE-APACHE`, and `zlog` declares
   GPL-3.0-or-later and links `LICENSE-GPL`. So both crates are most likely
   Apache-2.0. Either Apache-2.0 or GPL-3.0-or-later would be
   compatible, so the risk is in the record and not in the combination. The
   inventory's note says to "retain the upstream GPL/APACHE notices", which
   does not say which one applies. Recommended: record them as Apache-2.0 on
   the strength of the crate-level license file, and ask Zed upstream to add
   the field.

## A related gap outside copyleft

The packages carry the GPL-3.0 text and an inventory of license names, and
nothing else. The inventory says itself that it "does not replace the license
text supplied by each upstream project". Apache-2.0 section 4(a), the MIT and
BSD licenses, and the X11-style licenses all ask for their license text or
copyright notice to go along with binary copies. So the permissive packages
linked into every binary have the same unmet requirement as `self_cell`. This
review does not cover those packages one by one. Decision 2 is where it would be
settled.

## Limits of this review

- This is an engineering reading of license texts and build artifacts, not a
  legal opinion.
- It covers the v0.2.8 packages and the lockfile above. A lockfile change, a
  new target, or a different build host can change the result, and FreeType
  already differs between build hosts.
- Package licenses are taken from each crate's declared license field, which
  its authors wrote. Individual source files were read only where noted above.
- Whether any FreeType machine code survives the link in the 22.04 binaries
  was not established.
- The license of a Debian package was read from the review host, which runs
  Ubuntu 24.04 packages. The 22.04 packages were not read.
