# Third-Party License Notices

This inventory is generated from the locked Rust dependency graph with
`cargo metadata --format-version 1 --locked`. It is distributed with each
Better OS Debian package under `/usr/share/doc/<package>/`.

The inventory records the package license metadata and source reference.
It does not replace the license text supplied by each upstream project.

## Review summary

- Root project license: `GPL-3.0-or-later`.
- Resolved Cargo packages: 916.
- Packages with an SPDX license expression: 914.
- Packages with only a license-file field: 0.
- Packages without package-level license metadata: 2.
- `Cargo.lock` SHA-256: `a0b3a90ea8bd13b5d05a41b88226129f12dbae7cce9f3538a0e8dfbcf223f4f2`.

The 2 package(s) without package-level license metadata are
listed separately below. Their pinned upstream checkout contains both
`LICENSE-GPL` and `LICENSE-APACHE`; file-level upstream markings remain the
source of truth for those packages.

## License expression counts

| License expression | Package records |
| --- | ---: |
| `MIT OR Apache-2.0` | 401 |
| `MIT` | 186 |
| `Apache-2.0 OR MIT` | 76 |
| `GPL-3.0-or-later` | 54 |
| `MIT/Apache-2.0` | 35 |
| `Apache-2.0` | 32 |
| `Unicode-3.0` | 18 |
| `Zlib OR Apache-2.0 OR MIT` | 17 |
| `BSD-3-Clause` | 10 |
| `MIT OR Apache-2.0 OR Zlib` | 10 |
| `Apache-2.0/MIT` | 8 |
| `Unlicense OR MIT` | 8 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 7 |
| `ISC` | 5 |
| `Apache-2.0 OR ISC OR MIT` | 4 |
| `BSD-2-Clause` | 4 |
| `CC0-1.0` | 4 |
| `MIT / Apache-2.0` | 4 |
| `MPL-2.0` | 3 |
| `Zlib` | 3 |
| `0BSD` | 2 |
| `BSD-2-Clause OR Apache-2.0 OR MIT` | 2 |
| `BSD-3-Clause OR Apache-2.0` | 2 |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 2 |
| `Unlicense/MIT` | 2 |
| `(Apache-2.0 OR MIT) AND BSD-3-Clause` | 1 |
| `(MIT OR Apache-2.0) AND NCSA` | 1 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 |
| `0BSD OR MIT OR Apache-2.0` | 1 |
| `Apache-2.0 / MIT` | 1 |
| `Apache-2.0 AND ISC` | 1 |
| `Apache-2.0 OR BSL-1.0` | 1 |
| `Apache-2.0 OR GPL-2.0-only` | 1 |
| `Apache-2.0 WITH LLVM-exception` | 1 |
| `BSD-2-Clause OR MIT OR Apache-2.0` | 1 |
| `CC0-1.0 OR Apache-2.0` | 1 |
| `CC0-1.0 OR MIT-0 OR Apache-2.0` | 1 |
| `CDLA-Permissive-2.0` | 1 |
| `MIT OR Zlib OR Apache-2.0` | 1 |
| `bzip2-1.0.6` | 1 |

## Review focus

The following records contain copyleft or additional notice-sensitive
license expressions. Their upstream expressions are preserved verbatim;
Better OS does not relicense or silently select a different expression.

| Package | Version | License expression | Source |
| --- | --- | --- | --- |
| `app-catalog-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `app-catalog-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `app-chooser-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `app-chooser-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-ipc` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-service` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-store` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-tray` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `better-actions` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `better-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `better-ui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `cbindgen` | `0.28.0` | `MPL-2.0` | [crates.io](https://crates.io/crates/cbindgen/0.28.0) |
| `defaults-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `defaults-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `defaults-store` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `dwrote` | `0.11.5` | `MPL-2.0` | [crates.io](https://crates.io/crates/dwrote/0.11.5) |
| `files-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-operations` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-preview` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-search` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `launcher-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `launcher-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `launcher-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `libbz2-rs-sys` | `0.2.5` | `bzip2-1.0.6` | [crates.io](https://crates.io/crates/libbz2-rs-sys/0.2.5) |
| `libfuzzer-sys` | `0.4.13` | `(MIT OR Apache-2.0) AND NCSA` | [crates.io](https://crates.io/crates/libfuzzer-sys/0.4.13) |
| `manager-cli` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-daemon` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-ipc` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-store` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-actions-linux` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-cli` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-collectors-linux` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-export` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-ipc` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-service` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-store` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-views` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `option-ext` | `0.2.0` | `MPL-2.0` | [crates.io](https://crates.io/crates/option-ext/0.2.0) |
| `r-efi` | `5.3.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | [crates.io](https://crates.io/crates/r-efi/5.3.0) |
| `r-efi` | `6.0.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | [crates.io](https://crates.io/crates/r-efi/6.0.0) |
| `self_cell` | `1.3.0` | `Apache-2.0 OR GPL-2.0-only` | [crates.io](https://crates.io/crates/self_cell/1.3.0) |
| `storage-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `storage-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `storage-service` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-gestures` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-session` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `zlog` | `0.1.0` | `GPL-3.0-or-later` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `ztracing` | `0.1.0` | `GPL-3.0-or-later` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `ztracing_macro` | `0.1.0` | `GPL-3.0-or-later` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |

## Package inventory

| Package | Version | License metadata | Source |
| --- | --- | --- | --- |
| `accesskit` | `0.24.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/accesskit/0.24.1) |
| `accesskit_atspi_common` | `0.18.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/accesskit_atspi_common/0.18.1) |
| `accesskit_consumer` | `0.36.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/accesskit_consumer/0.36.0) |
| `accesskit_consumer` | `0.37.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/accesskit_consumer/0.37.0) |
| `accesskit_consumer` | `0.38.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/accesskit_consumer/0.38.0) |
| `accesskit_macos` | `0.26.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/accesskit_macos/0.26.3) |
| `accesskit_unix` | `0.21.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/accesskit_unix/0.21.1) |
| `accesskit_windows` | `0.33.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/accesskit_windows/0.33.1) |
| `addr2line` | `0.25.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/addr2line/0.25.1) |
| `adler2` | `2.0.1` | `0BSD OR MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/adler2/2.0.1) |
| `aes` | `0.8.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/aes/0.8.4) |
| `ahash` | `0.8.12` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ahash/0.8.12) |
| `aho-corasick` | `1.1.4` | `Unlicense OR MIT` | [crates.io](https://crates.io/crates/aho-corasick/1.1.4) |
| `aligned` | `0.4.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/aligned/0.4.3) |
| `aligned-vec` | `0.6.4` | `MIT` | [crates.io](https://crates.io/crates/aligned-vec/0.6.4) |
| `allocator-api2` | `0.2.21` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/allocator-api2/0.2.21) |
| `android_system_properties` | `0.1.5` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/android_system_properties/0.1.5) |
| `annotate-snippets` | `0.12.16` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/annotate-snippets/0.12.16) |
| `anstream` | `1.0.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/anstream/1.0.0) |
| `anstyle` | `1.0.14` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/anstyle/1.0.14) |
| `anstyle-parse` | `1.0.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/anstyle-parse/1.0.0) |
| `anstyle-query` | `1.1.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/anstyle-query/1.1.5) |
| `anstyle-wincon` | `3.0.11` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/anstyle-wincon/3.0.11) |
| `anyhow` | `1.0.104` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/anyhow/1.0.104) |
| `app-catalog-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `app-catalog-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `app-chooser-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `app-chooser-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `ar_archive_writer` | `0.5.2` | `Apache-2.0 WITH LLVM-exception` | [crates.io](https://crates.io/crates/ar_archive_writer/0.5.2) |
| `arbitrary` | `1.4.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/arbitrary/1.4.2) |
| `arc-swap` | `1.9.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/arc-swap/1.9.2) |
| `arg_enum_proc_macro` | `0.3.4` | `MIT` | [crates.io](https://crates.io/crates/arg_enum_proc_macro/0.3.4) |
| `arraydeque` | `0.5.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/arraydeque/0.5.1) |
| `arrayref` | `0.3.9` | `BSD-2-Clause` | [crates.io](https://crates.io/crates/arrayref/0.3.9) |
| `arrayvec` | `0.7.8` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/arrayvec/0.7.8) |
| `as-raw-xcb-connection` | `1.0.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/as-raw-xcb-connection/1.0.1) |
| `as-slice` | `0.2.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/as-slice/0.2.1) |
| `ash` | `0.38.0+1.3.281` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ash/0.38.0+1.3.281) |
| `ashpd` | `0.13.13` | `MIT` | [crates.io](https://crates.io/crates/ashpd/0.13.13) |
| `async-broadcast` | `0.7.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/async-broadcast/0.7.2) |
| `async-channel` | `2.5.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-channel/2.5.0) |
| `async-compression` | `0.4.43` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/async-compression/0.4.43) |
| `async-executor` | `1.14.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-executor/1.14.0) |
| `async-fs` | `2.2.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-fs/2.2.0) |
| `async-io` | `2.6.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-io/2.6.0) |
| `async-lock` | `3.4.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-lock/3.4.2) |
| `async-net` | `2.0.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-net/2.0.0) |
| `async-process` | `2.5.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-process/2.5.0) |
| `async-recursion` | `1.1.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/async-recursion/1.1.1) |
| `async-signal` | `0.2.14` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-signal/0.2.14) |
| `async-task` | `4.7.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/async-task/4.7.1) |
| `async-trait` | `0.1.91` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/async-trait/0.1.91) |
| `atomic` | `0.5.3` | `Apache-2.0/MIT` | [crates.io](https://crates.io/crates/atomic/0.5.3) |
| `atomic-waker` | `1.1.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/atomic-waker/1.1.2) |
| `atspi` | `0.29.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/atspi/0.29.0) |
| `atspi-common` | `0.13.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/atspi-common/0.13.0) |
| `atspi-proxies` | `0.13.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/atspi-proxies/0.13.0) |
| `autocfg` | `1.5.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/autocfg/1.5.1) |
| `av-scenechange` | `0.14.1` | `MIT` | [crates.io](https://crates.io/crates/av-scenechange/0.14.1) |
| `av1-grain` | `0.2.5` | `BSD-2-Clause` | [crates.io](https://crates.io/crates/av1-grain/0.2.5) |
| `avif-serialize` | `0.8.9` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/avif-serialize/0.8.9) |
| `awake-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-ipc` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-service` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-store` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `awake-tray` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `backtrace` | `0.3.76` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/backtrace/0.3.76) |
| `base62` | `2.2.4` | `MIT` | [crates.io](https://crates.io/crates/base62/2.2.4) |
| `base64` | `0.22.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/base64/0.22.1) |
| `better-actions` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `better-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `better-ui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `bindgen` | `0.71.1` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/bindgen/0.71.1) |
| `bit-set` | `0.9.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/bit-set/0.9.1) |
| `bit-vec` | `0.9.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/bit-vec/0.9.1) |
| `bit_field` | `0.10.3` | `Apache-2.0/MIT` | [crates.io](https://crates.io/crates/bit_field/0.10.3) |
| `bitflags` | `1.3.2` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/bitflags/1.3.2) |
| `bitflags` | `2.13.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/bitflags/2.13.1) |
| `bitstream-io` | `4.10.0` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/bitstream-io/4.10.0) |
| `block` | `0.1.6` | `MIT` | [crates.io](https://crates.io/crates/block/0.1.6) |
| `block-buffer` | `0.10.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/block-buffer/0.10.4) |
| `block-buffer` | `0.12.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/block-buffer/0.12.1) |
| `block-padding` | `0.3.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/block-padding/0.3.3) |
| `block2` | `0.5.1` | `MIT` | [crates.io](https://crates.io/crates/block2/0.5.1) |
| `block2` | `0.6.2` | `MIT` | [crates.io](https://crates.io/crates/block2/0.6.2) |
| `blocking` | `1.6.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/blocking/1.6.2) |
| `borsh` | `1.8.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/borsh/1.8.0) |
| `bstr` | `1.13.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/bstr/1.13.0) |
| `built` | `0.8.1` | `MIT` | [crates.io](https://crates.io/crates/built/0.8.1) |
| `bumpalo` | `3.20.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/bumpalo/3.20.3) |
| `bytemuck` | `1.25.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/bytemuck/1.25.2) |
| `bytemuck_derive` | `1.11.0` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/bytemuck_derive/1.11.0) |
| `byteorder` | `1.5.0` | `Unlicense OR MIT` | [crates.io](https://crates.io/crates/byteorder/1.5.0) |
| `byteorder-lite` | `0.1.0` | `Unlicense OR MIT` | [crates.io](https://crates.io/crates/byteorder-lite/0.1.0) |
| `bytes` | `1.12.1` | `MIT` | [crates.io](https://crates.io/crates/bytes/1.12.1) |
| `bzip2` | `0.6.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/bzip2/0.6.1) |
| `calloop` | `0.14.4` | `MIT` | [crates.io](https://crates.io/crates/calloop/0.14.4) |
| `calloop-wayland-source` | `0.4.1` | `MIT` | [crates.io](https://crates.io/crates/calloop-wayland-source/0.4.1) |
| `cbc` | `0.1.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cbc/0.1.2) |
| `cbindgen` | `0.28.0` | `MPL-2.0` | [crates.io](https://crates.io/crates/cbindgen/0.28.0) |
| `cc` | `1.4.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cc/1.4.0) |
| `cexpr` | `0.6.0` | `Apache-2.0/MIT` | [crates.io](https://crates.io/crates/cexpr/0.6.0) |
| `cfg-if` | `1.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cfg-if/1.0.4) |
| `cfg_aliases` | `0.2.2` | `MIT` | [crates.io](https://crates.io/crates/cfg_aliases/0.2.2) |
| `cgl` | `0.3.2` | `MIT / Apache-2.0` | [crates.io](https://crates.io/crates/cgl/0.3.2) |
| `chacha20` | `0.10.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/chacha20/0.10.1) |
| `chrono` | `0.4.45` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/chrono/0.4.45) |
| `cipher` | `0.4.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cipher/0.4.4) |
| `clang-sys` | `1.9.1` | `Apache-2.0` | [crates.io](https://crates.io/crates/clang-sys/1.9.1) |
| `clap` | `4.6.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/clap/4.6.4) |
| `clap_builder` | `4.6.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/clap_builder/4.6.2) |
| `clap_derive` | `4.6.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/clap_derive/4.6.4) |
| `clap_lex` | `1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/clap_lex/1.1.0) |
| `cocoa` | `0.25.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cocoa/0.25.0) |
| `cocoa` | `0.26.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cocoa/0.26.0) |
| `cocoa-foundation` | `0.1.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cocoa-foundation/0.1.2) |
| `cocoa-foundation` | `0.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cocoa-foundation/0.2.0) |
| `codespan-reporting` | `0.13.1` | `Apache-2.0` | [crates.io](https://crates.io/crates/codespan-reporting/0.13.1) |
| `collections` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `color_quant` | `1.1.0` | `MIT` | [crates.io](https://crates.io/crates/color_quant/1.1.0) |
| `colorchoice` | `1.0.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/colorchoice/1.0.5) |
| `compression-codecs` | `0.4.38` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/compression-codecs/0.4.38) |
| `compression-core` | `0.4.32` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/compression-core/0.4.32) |
| `concurrent-queue` | `2.5.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/concurrent-queue/2.5.0) |
| `console_error_panic_hook` | `0.1.7` | `Apache-2.0/MIT` | [crates.io](https://crates.io/crates/console_error_panic_hook/0.1.7) |
| `const-oid` | `0.10.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/const-oid/0.10.2) |
| `const-random` | `0.1.18` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/const-random/0.1.18) |
| `const-random-macro` | `0.1.16` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/const-random-macro/0.1.16) |
| `convert_case` | `0.10.0` | `MIT` | [crates.io](https://crates.io/crates/convert_case/0.10.0) |
| `core-foundation` | `0.10.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-foundation/0.10.1) |
| `core-foundation` | `0.9.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-foundation/0.9.4) |
| `core-foundation-sys` | `0.8.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-foundation-sys/0.8.7) |
| `core-graphics` | `0.23.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-graphics/0.23.2) |
| `core-graphics` | `0.24.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-graphics/0.24.0) |
| `core-graphics-helmer-fork` | `0.24.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-graphics-helmer-fork/0.24.0) |
| `core-graphics-types` | `0.1.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-graphics-types/0.1.3) |
| `core-graphics-types` | `0.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-graphics-types/0.2.0) |
| `core-graphics2` | `0.5.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-graphics2/0.5.2) |
| `core-text` | `21.0.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-text/21.0.0) |
| `core-video` | `0.5.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/core-video/0.5.2) |
| `core_maths` | `0.1.1` | `MIT` | [crates.io](https://crates.io/crates/core_maths/0.1.1) |
| `cosmic-text` | `0.19.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cosmic-text/0.19.0) |
| `cpufeatures` | `0.2.17` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cpufeatures/0.2.17) |
| `cpufeatures` | `0.3.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/cpufeatures/0.3.0) |
| `crc32fast` | `1.5.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/crc32fast/1.5.0) |
| `crossbeam-deque` | `0.8.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/crossbeam-deque/0.8.7) |
| `crossbeam-epoch` | `0.9.20` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/crossbeam-epoch/0.9.20) |
| `crossbeam-queue` | `0.3.13` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/crossbeam-queue/0.3.13) |
| `crossbeam-utils` | `0.8.22` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/crossbeam-utils/0.8.22) |
| `crunchy` | `0.2.4` | `MIT` | [crates.io](https://crates.io/crates/crunchy/0.2.4) |
| `crypto-common` | `0.1.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/crypto-common/0.1.7) |
| `crypto-common` | `0.2.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/crypto-common/0.2.2) |
| `ctor` | `1.0.12` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/ctor/1.0.12) |
| `data-url` | `0.3.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/data-url/0.3.2) |
| `defaults-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `defaults-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `defaults-store` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `deranged` | `0.5.8` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/deranged/0.5.8) |
| `derive_more` | `2.1.1` | `MIT` | [crates.io](https://crates.io/crates/derive_more/2.1.1) |
| `derive_more-impl` | `2.1.1` | `MIT` | [crates.io](https://crates.io/crates/derive_more-impl/2.1.1) |
| `derive_refineable` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `digest` | `0.10.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/digest/0.10.7) |
| `digest` | `0.11.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/digest/0.11.3) |
| `dirs` | `6.0.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/dirs/6.0.0) |
| `dirs-sys` | `0.5.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/dirs-sys/0.5.0) |
| `dispatch` | `0.2.0` | `MIT` | [crates.io](https://crates.io/crates/dispatch/0.2.0) |
| `dispatch2` | `0.3.1` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/dispatch2/0.3.1) |
| `displaydoc` | `0.2.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/displaydoc/0.2.7) |
| `dlib` | `0.5.3` | `MIT` | [crates.io](https://crates.io/crates/dlib/0.5.3) |
| `document-features` | `0.2.12` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/document-features/0.2.12) |
| `downcast-rs` | `1.2.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/downcast-rs/1.2.1) |
| `dunce` | `1.0.5` | `CC0-1.0 OR MIT-0 OR Apache-2.0` | [crates.io](https://crates.io/crates/dunce/1.0.5) |
| `dwrote` | `0.11.5` | `MPL-2.0` | [crates.io](https://crates.io/crates/dwrote/0.11.5) |
| `dyn-clone` | `1.0.20` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/dyn-clone/1.0.20) |
| `either` | `1.17.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/either/1.17.0) |
| `embed-resource` | `3.0.11` | `MIT` | [crates.io](https://crates.io/crates/embed-resource/3.0.11) |
| `encoding_rs` | `0.8.35` | `(Apache-2.0 OR MIT) AND BSD-3-Clause` | [crates.io](https://crates.io/crates/encoding_rs/0.8.35) |
| `encoding_rs_io` | `0.1.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/encoding_rs_io/0.1.7) |
| `endi` | `1.1.1` | `MIT` | [crates.io](https://crates.io/crates/endi/1.1.1) |
| `enum-iterator` | `2.3.0` | `0BSD` | [crates.io](https://crates.io/crates/enum-iterator/2.3.0) |
| `enum-iterator-derive` | `1.5.0` | `0BSD` | [crates.io](https://crates.io/crates/enum-iterator-derive/1.5.0) |
| `enumflags2` | `0.7.12` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/enumflags2/0.7.12) |
| `enumflags2_derive` | `0.7.12` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/enumflags2_derive/0.7.12) |
| `enumn` | `0.1.14` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/enumn/0.1.14) |
| `equator` | `0.4.2` | `MIT` | [crates.io](https://crates.io/crates/equator/0.4.2) |
| `equator-macro` | `0.4.2` | `MIT` | [crates.io](https://crates.io/crates/equator-macro/0.4.2) |
| `equivalent` | `1.0.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/equivalent/1.0.2) |
| `erased-serde` | `0.4.10` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/erased-serde/0.4.10) |
| `errno` | `0.3.14` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/errno/0.3.14) |
| `etagere` | `0.2.15` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/etagere/0.2.15) |
| `euclid` | `0.22.14` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/euclid/0.22.14) |
| `event-listener` | `5.4.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/event-listener/5.4.2) |
| `event-listener-strategy` | `0.5.4` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/event-listener-strategy/0.5.4) |
| `exr` | `1.74.2` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/exr/1.74.2) |
| `fastrand` | `2.5.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/fastrand/2.5.0) |
| `fax` | `0.2.7` | `MIT` | [crates.io](https://crates.io/crates/fax/0.2.7) |
| `fdeflate` | `0.3.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/fdeflate/0.3.7) |
| `filedescriptor` | `0.8.3` | `MIT` | [crates.io](https://crates.io/crates/filedescriptor/0.8.3) |
| `files-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-operations` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-preview` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `files-search` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `filetime` | `0.2.29` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/filetime/0.2.29) |
| `find-msvc-tools` | `0.1.9` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/find-msvc-tools/0.1.9) |
| `fixedbitset` | `0.5.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/fixedbitset/0.5.7) |
| `flate2` | `1.1.9` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/flate2/1.1.9) |
| `float-cmp` | `0.9.0` | `MIT` | [crates.io](https://crates.io/crates/float-cmp/0.9.0) |
| `float-ord` | `0.3.2` | `MIT / Apache-2.0` | [crates.io](https://crates.io/crates/float-ord/0.3.2) |
| `float_next_after` | `1.0.0` | `MIT` | [crates.io](https://crates.io/crates/float_next_after/1.0.0) |
| `fluent-uri` | `0.1.4` | `MIT` | [crates.io](https://crates.io/crates/fluent-uri/0.1.4) |
| `flume` | `0.12.0` | `Apache-2.0/MIT` | [crates.io](https://crates.io/crates/flume/0.12.0) |
| `fnv` | `1.0.7` | `Apache-2.0 / MIT` | [crates.io](https://crates.io/crates/fnv/1.0.7) |
| `foldhash` | `0.1.5` | `Zlib` | [crates.io](https://crates.io/crates/foldhash/0.1.5) |
| `foldhash` | `0.2.0` | `Zlib` | [crates.io](https://crates.io/crates/foldhash/0.2.0) |
| `font-types` | `0.11.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/font-types/0.11.3) |
| `font-types` | `0.12.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/font-types/0.12.2) |
| `fontconfig-parser` | `0.5.8` | `MIT` | [crates.io](https://crates.io/crates/fontconfig-parser/0.5.8) |
| `fontdb` | `0.23.0` | `MIT` | [crates.io](https://crates.io/crates/fontdb/0.23.0) |
| `foreign-types` | `0.5.0` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/foreign-types/0.5.0) |
| `foreign-types-macros` | `0.2.4` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/foreign-types-macros/0.2.4) |
| `foreign-types-shared` | `0.3.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/foreign-types-shared/0.3.1) |
| `form_urlencoded` | `1.2.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/form_urlencoded/1.2.2) |
| `freetype-sys` | `0.20.1` | `MIT` | [crates.io](https://crates.io/crates/freetype-sys/0.20.1) |
| `fsevent-sys` | `4.1.0` | `MIT` | [crates.io](https://crates.io/crates/fsevent-sys/4.1.0) |
| `futf` | `0.1.5` | `MIT / Apache-2.0` | [crates.io](https://crates.io/crates/futf/0.1.5) |
| `futures` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures/0.3.33) |
| `futures-channel` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-channel/0.3.33) |
| `futures-concurrency` | `7.7.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-concurrency/7.7.1) |
| `futures-core` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-core/0.3.33) |
| `futures-executor` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-executor/0.3.33) |
| `futures-io` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-io/0.3.33) |
| `futures-lite` | `2.6.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/futures-lite/2.6.1) |
| `futures-macro` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-macro/0.3.33) |
| `futures-sink` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-sink/0.3.33) |
| `futures-task` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-task/0.3.33) |
| `futures-util` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/futures-util/0.3.33) |
| `generic-array` | `0.14.7` | `MIT` | [crates.io](https://crates.io/crates/generic-array/0.14.7) |
| `gethostname` | `1.1.0` | `Apache-2.0` | [crates.io](https://crates.io/crates/gethostname/1.1.0) |
| `getrandom` | `0.2.17` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/getrandom/0.2.17) |
| `getrandom` | `0.3.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/getrandom/0.3.4) |
| `getrandom` | `0.4.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/getrandom/0.4.3) |
| `gif` | `0.13.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/gif/0.13.3) |
| `gif` | `0.14.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/gif/0.14.2) |
| `gimli` | `0.32.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/gimli/0.32.3) |
| `gl_generator` | `0.14.0` | `Apache-2.0` | [crates.io](https://crates.io/crates/gl_generator/0.14.0) |
| `glob` | `0.3.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/glob/0.3.4) |
| `globset` | `0.4.19` | `Unlicense OR MIT` | [crates.io](https://crates.io/crates/globset/0.4.19) |
| `globwalk` | `0.8.1` | `MIT` | [crates.io](https://crates.io/crates/globwalk/0.8.1) |
| `glow` | `0.17.0` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/glow/0.17.0) |
| `glutin_wgl_sys` | `0.6.1` | `Apache-2.0` | [crates.io](https://crates.io/crates/glutin_wgl_sys/0.6.1) |
| `gpu-allocator` | `0.28.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/gpu-allocator/0.28.0) |
| `gpu-descriptor` | `0.3.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/gpu-descriptor/0.3.2) |
| `gpu-descriptor-types` | `0.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/gpu-descriptor-types/0.2.0) |
| `gpui` | `0.2.2` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui-component` | `0.5.2` | `Apache-2.0` | `git+https://github.com/longbridge/gpui-component?rev=88f102d13654fe25aa2fede076274b6b751a3704#88f102d13654fe25aa2fede076274b6b751a3704` |
| `gpui-component-assets` | `0.5.1` | `Apache-2.0` | `git+https://github.com/longbridge/gpui-component?rev=88f102d13654fe25aa2fede076274b6b751a3704#88f102d13654fe25aa2fede076274b6b751a3704` |
| `gpui-component-macros` | `0.5.1` | `Apache-2.0` | `git+https://github.com/longbridge/gpui-component?rev=88f102d13654fe25aa2fede076274b6b751a3704#88f102d13654fe25aa2fede076274b6b751a3704` |
| `gpui_linux` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui_macos` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui_macros` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui_platform` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui_shared_string` | `0.1.0` | `missing package metadata` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui_util` | `0.1.0` | `missing package metadata` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui_web` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui_wgpu` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `gpui_windows` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `granit-parser` | `0.0.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/granit-parser/0.0.7) |
| `grid` | `1.0.1` | `MIT` | [crates.io](https://crates.io/crates/grid/1.0.1) |
| `h2` | `0.4.15` | `MIT` | [crates.io](https://crates.io/crates/h2/0.4.15) |
| `half` | `2.7.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/half/2.7.1) |
| `harfrust` | `0.5.2` | `MIT` | [crates.io](https://crates.io/crates/harfrust/0.5.2) |
| `hash32` | `0.3.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hash32/0.3.1) |
| `hashbrown` | `0.14.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hashbrown/0.14.5) |
| `hashbrown` | `0.15.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hashbrown/0.15.5) |
| `hashbrown` | `0.16.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hashbrown/0.16.1) |
| `hashbrown` | `0.17.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hashbrown/0.17.1) |
| `heapless` | `0.9.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/heapless/0.9.3) |
| `heck` | `0.4.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/heck/0.4.1) |
| `heck` | `0.5.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/heck/0.5.0) |
| `hermit-abi` | `0.5.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hermit-abi/0.5.2) |
| `hex` | `0.4.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hex/0.4.3) |
| `hexf-parse` | `0.2.1` | `CC0-1.0` | [crates.io](https://crates.io/crates/hexf-parse/0.2.1) |
| `hkdf` | `0.12.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hkdf/0.12.4) |
| `hmac` | `0.12.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hmac/0.12.1) |
| `home` | `0.5.12` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/home/0.5.12) |
| `html5ever` | `0.27.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/html5ever/0.27.0) |
| `http` | `1.5.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/http/1.5.0) |
| `http-body` | `1.1.0` | `MIT` | [crates.io](https://crates.io/crates/http-body/1.1.0) |
| `http-body-util` | `0.1.4` | `MIT` | [crates.io](https://crates.io/crates/http-body-util/0.1.4) |
| `http_client` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `httparse` | `1.10.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/httparse/1.10.1) |
| `hybrid-array` | `0.4.14` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/hybrid-array/0.4.14) |
| `hyper` | `1.11.0` | `MIT` | [crates.io](https://crates.io/crates/hyper/1.11.0) |
| `hyper-rustls` | `0.27.9` | `Apache-2.0 OR ISC OR MIT` | [crates.io](https://crates.io/crates/hyper-rustls/0.27.9) |
| `hyper-util` | `0.1.20` | `MIT` | [crates.io](https://crates.io/crates/hyper-util/0.1.20) |
| `iana-time-zone` | `0.1.65` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/iana-time-zone/0.1.65) |
| `iana-time-zone-haiku` | `0.1.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/iana-time-zone-haiku/0.1.2) |
| `icu_collections` | `2.2.0` | `Unicode-3.0` | [crates.io](https://crates.io/crates/icu_collections/2.2.0) |
| `icu_locale_core` | `2.2.0` | `Unicode-3.0` | [crates.io](https://crates.io/crates/icu_locale_core/2.2.0) |
| `icu_normalizer` | `2.2.0` | `Unicode-3.0` | [crates.io](https://crates.io/crates/icu_normalizer/2.2.0) |
| `icu_normalizer_data` | `2.2.0` | `Unicode-3.0` | [crates.io](https://crates.io/crates/icu_normalizer_data/2.2.0) |
| `icu_properties` | `2.2.0` | `Unicode-3.0` | [crates.io](https://crates.io/crates/icu_properties/2.2.0) |
| `icu_properties_data` | `2.2.0` | `Unicode-3.0` | [crates.io](https://crates.io/crates/icu_properties_data/2.2.0) |
| `icu_provider` | `2.2.0` | `Unicode-3.0` | [crates.io](https://crates.io/crates/icu_provider/2.2.0) |
| `idna` | `1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/idna/1.1.0) |
| `idna_adapter` | `1.2.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/idna_adapter/1.2.2) |
| `ignore` | `0.4.31` | `Unlicense OR MIT` | [crates.io](https://crates.io/crates/ignore/0.4.31) |
| `image` | `0.25.10` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/image/0.25.10) |
| `image-webp` | `0.2.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/image-webp/0.2.4) |
| `imagesize` | `0.13.0` | `MIT` | [crates.io](https://crates.io/crates/imagesize/0.13.0) |
| `imagesize` | `0.14.0` | `MIT` | [crates.io](https://crates.io/crates/imagesize/0.14.0) |
| `imgref` | `1.12.2` | `CC0-1.0 OR Apache-2.0` | [crates.io](https://crates.io/crates/imgref/1.12.2) |
| `indexmap` | `2.14.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/indexmap/2.14.0) |
| `inotify` | `0.10.2` | `ISC` | [crates.io](https://crates.io/crates/inotify/0.10.2) |
| `inotify-sys` | `0.1.8` | `ISC` | [crates.io](https://crates.io/crates/inotify-sys/0.1.8) |
| `inout` | `0.1.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/inout/0.1.4) |
| `instant` | `0.1.13` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/instant/0.1.13) |
| `interpolate_name` | `0.2.4` | `MIT` | [crates.io](https://crates.io/crates/interpolate_name/0.2.4) |
| `inventory` | `0.3.24` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/inventory/0.3.24) |
| `io-surface` | `0.16.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/io-surface/0.16.1) |
| `ipnet` | `2.12.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ipnet/2.12.0) |
| `is-docker` | `0.2.0` | `MIT` | [crates.io](https://crates.io/crates/is-docker/0.2.0) |
| `is-wsl` | `0.4.0` | `MIT` | [crates.io](https://crates.io/crates/is-wsl/0.4.0) |
| `is_terminal_polyfill` | `1.70.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/is_terminal_polyfill/1.70.2) |
| `itertools` | `0.11.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/itertools/0.11.0) |
| `itertools` | `0.13.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/itertools/0.13.0) |
| `itertools` | `0.14.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/itertools/0.14.0) |
| `itoa` | `1.0.18` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/itoa/1.0.18) |
| `jni-sys` | `0.3.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/jni-sys/0.3.1) |
| `jni-sys` | `0.4.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/jni-sys/0.4.1) |
| `jni-sys-macros` | `0.4.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/jni-sys-macros/0.4.1) |
| `jobserver` | `0.1.35` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/jobserver/0.1.35) |
| `js-sys` | `0.3.103` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/js-sys/0.3.103) |
| `khronos-egl` | `6.0.0` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/khronos-egl/6.0.0) |
| `khronos_api` | `3.1.0` | `Apache-2.0` | [crates.io](https://crates.io/crates/khronos_api/3.1.0) |
| `kqueue` | `1.2.0` | `MIT` | [crates.io](https://crates.io/crates/kqueue/1.2.0) |
| `kqueue-sys` | `1.1.2` | `MIT` | [crates.io](https://crates.io/crates/kqueue-sys/1.1.2) |
| `kurbo` | `0.11.3` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/kurbo/0.11.3) |
| `kurbo` | `0.13.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/kurbo/0.13.1) |
| `launcher-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `launcher-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `launcher-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `lazy_static` | `1.5.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/lazy_static/1.5.0) |
| `leak` | `0.1.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/leak/0.1.2) |
| `leaky-cow` | `0.1.1` | `MIT / Apache-2.0` | [crates.io](https://crates.io/crates/leaky-cow/0.1.1) |
| `lebe` | `0.5.3` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/lebe/0.5.3) |
| `libbz2-rs-sys` | `0.2.5` | `bzip2-1.0.6` | [crates.io](https://crates.io/crates/libbz2-rs-sys/0.2.5) |
| `libc` | `0.2.189` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/libc/0.2.189) |
| `libfuzzer-sys` | `0.4.13` | `(MIT OR Apache-2.0) AND NCSA` | [crates.io](https://crates.io/crates/libfuzzer-sys/0.4.13) |
| `libloading` | `0.8.9` | `ISC` | [crates.io](https://crates.io/crates/libloading/0.8.9) |
| `libm` | `0.2.16` | `MIT` | [crates.io](https://crates.io/crates/libm/0.2.16) |
| `libredox` | `0.1.18` | `MIT` | [crates.io](https://crates.io/crates/libredox/0.1.18) |
| `linebender_resource_handle` | `0.1.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/linebender_resource_handle/0.1.1) |
| `link-section` | `0.19.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/link-section/0.19.2) |
| `linktime-proc-macro` | `0.2.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/linktime-proc-macro/0.2.2) |
| `linux-raw-sys` | `0.12.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/linux-raw-sys/0.12.1) |
| `linux-raw-sys` | `0.4.15` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/linux-raw-sys/0.4.15) |
| `litemap` | `0.8.2` | `Unicode-3.0` | [crates.io](https://crates.io/crates/litemap/0.8.2) |
| `litrs` | `1.0.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/litrs/1.0.0) |
| `lock_api` | `0.4.14` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/lock_api/0.4.14) |
| `log` | `0.4.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/log/0.4.33) |
| `loop9` | `0.1.5` | `MIT` | [crates.io](https://crates.io/crates/loop9/0.1.5) |
| `lru-slab` | `0.1.2` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/lru-slab/0.1.2) |
| `lsp-types` | `0.97.0` | `MIT` | [crates.io](https://crates.io/crates/lsp-types/0.97.0) |
| `lyon` | `1.0.19` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/lyon/1.0.19) |
| `lyon_algorithms` | `1.0.20` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/lyon_algorithms/1.0.20) |
| `lyon_geom` | `1.0.19` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/lyon_geom/1.0.19) |
| `lyon_path` | `1.0.19` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/lyon_path/1.0.19) |
| `lyon_tessellation` | `1.0.20` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/lyon_tessellation/1.0.20) |
| `mac` | `0.1.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/mac/0.1.1) |
| `mac-notification-sys` | `0.6.15` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/mac-notification-sys/0.6.15) |
| `mach2` | `0.5.0` | `BSD-2-Clause OR MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/mach2/0.5.0) |
| `malloc_buf` | `0.0.6` | `MIT` | [crates.io](https://crates.io/crates/malloc_buf/0.0.6) |
| `manager-cli` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-daemon` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-ipc` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `manager-store` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `markdown` | `1.0.0` | `MIT` | [crates.io](https://crates.io/crates/markdown/1.0.0) |
| `markup5ever` | `0.12.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/markup5ever/0.12.1) |
| `markup5ever_rcdom` | `0.3.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/markup5ever_rcdom/0.3.0) |
| `maybe-rayon` | `0.1.1` | `MIT` | [crates.io](https://crates.io/crates/maybe-rayon/0.1.1) |
| `md-5` | `0.10.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/md-5/0.10.6) |
| `media` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `memchr` | `2.8.3` | `Unlicense OR MIT` | [crates.io](https://crates.io/crates/memchr/2.8.3) |
| `memmap2` | `0.9.11` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/memmap2/0.9.11) |
| `memoffset` | `0.9.1` | `MIT` | [crates.io](https://crates.io/crates/memoffset/0.9.1) |
| `metal` | `0.33.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/metal/0.33.0) |
| `mime` | `0.3.17` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/mime/0.3.17) |
| `mime_guess` | `2.0.5` | `MIT` | [crates.io](https://crates.io/crates/mime_guess/2.0.5) |
| `minimal-lexical` | `0.2.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/minimal-lexical/0.2.1) |
| `miniz_oxide` | `0.8.9` | `MIT OR Zlib OR Apache-2.0` | [crates.io](https://crates.io/crates/miniz_oxide/0.8.9) |
| `mio` | `1.2.2` | `MIT` | [crates.io](https://crates.io/crates/mio/1.2.2) |
| `monitor-actions-linux` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-cli` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-collectors-linux` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-export` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-ipc` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-service` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-store` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `monitor-views` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `moxcms` | `0.8.1` | `BSD-3-Clause OR Apache-2.0` | [crates.io](https://crates.io/crates/moxcms/0.8.1) |
| `naga` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/naga/29.0.4) |
| `ndk-sys` | `0.6.0+11769913` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ndk-sys/0.6.0+11769913) |
| `new_debug_unreachable` | `1.0.6` | `MIT` | [crates.io](https://crates.io/crates/new_debug_unreachable/1.0.6) |
| `no_std_io2` | `0.9.4` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/no_std_io2/0.9.4) |
| `nohash-hasher` | `0.2.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/nohash-hasher/0.2.0) |
| `nom` | `7.1.3` | `MIT` | [crates.io](https://crates.io/crates/nom/7.1.3) |
| `nom` | `8.0.0` | `MIT` | [crates.io](https://crates.io/crates/nom/8.0.0) |
| `noop_proc_macro` | `0.3.0` | `MIT` | [crates.io](https://crates.io/crates/noop_proc_macro/0.3.0) |
| `normpath` | `1.5.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/normpath/1.5.1) |
| `notify` | `7.0.0` | `CC0-1.0` | [crates.io](https://crates.io/crates/notify/7.0.0) |
| `notify-rust` | `4.18.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/notify-rust/4.18.0) |
| `notify-types` | `1.0.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/notify-types/1.0.1) |
| `ntapi` | `0.4.3` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/ntapi/0.4.3) |
| `nu-ansi-term` | `0.50.3` | `MIT` | [crates.io](https://crates.io/crates/nu-ansi-term/0.50.3) |
| `num` | `0.4.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num/0.4.3) |
| `num-bigint` | `0.4.8` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num-bigint/0.4.8) |
| `num-bigint-dig` | `0.9.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/num-bigint-dig/0.9.1) |
| `num-complex` | `0.4.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num-complex/0.4.6) |
| `num-conv` | `0.2.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num-conv/0.2.2) |
| `num-derive` | `0.4.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num-derive/0.4.2) |
| `num-integer` | `0.1.46` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num-integer/0.1.46) |
| `num-iter` | `0.1.46` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num-iter/0.1.46) |
| `num-rational` | `0.4.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num-rational/0.4.2) |
| `num-traits` | `0.2.19` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num-traits/0.2.19) |
| `num_cpus` | `1.17.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/num_cpus/1.17.0) |
| `objc` | `0.2.7` | `MIT` | [crates.io](https://crates.io/crates/objc/0.2.7) |
| `objc-foundation` | `0.1.1` | `MIT` | [crates.io](https://crates.io/crates/objc-foundation/0.1.1) |
| `objc-sys` | `0.3.5` | `MIT` | [crates.io](https://crates.io/crates/objc-sys/0.3.5) |
| `objc2` | `0.5.2` | `MIT` | [crates.io](https://crates.io/crates/objc2/0.5.2) |
| `objc2` | `0.6.4` | `MIT` | [crates.io](https://crates.io/crates/objc2/0.6.4) |
| `objc2-app-kit` | `0.2.2` | `MIT` | [crates.io](https://crates.io/crates/objc2-app-kit/0.2.2) |
| `objc2-app-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-app-kit/0.3.2) |
| `objc2-cloud-kit` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-cloud-kit/0.3.2) |
| `objc2-core-data` | `0.2.2` | `MIT` | [crates.io](https://crates.io/crates/objc2-core-data/0.2.2) |
| `objc2-core-data` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-core-data/0.3.2) |
| `objc2-core-foundation` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-core-foundation/0.3.2) |
| `objc2-core-graphics` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-core-graphics/0.3.2) |
| `objc2-core-image` | `0.2.2` | `MIT` | [crates.io](https://crates.io/crates/objc2-core-image/0.2.2) |
| `objc2-core-image` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-core-image/0.3.2) |
| `objc2-core-location` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-core-location/0.3.2) |
| `objc2-core-text` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-core-text/0.3.2) |
| `objc2-core-video` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-core-video/0.3.2) |
| `objc2-encode` | `4.1.0` | `MIT` | [crates.io](https://crates.io/crates/objc2-encode/4.1.0) |
| `objc2-foundation` | `0.2.2` | `MIT` | [crates.io](https://crates.io/crates/objc2-foundation/0.2.2) |
| `objc2-foundation` | `0.3.2` | `MIT` | [crates.io](https://crates.io/crates/objc2-foundation/0.3.2) |
| `objc2-io-surface` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-io-surface/0.3.2) |
| `objc2-metal` | `0.2.2` | `MIT` | [crates.io](https://crates.io/crates/objc2-metal/0.2.2) |
| `objc2-metal` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-metal/0.3.2) |
| `objc2-quartz-core` | `0.2.2` | `MIT` | [crates.io](https://crates.io/crates/objc2-quartz-core/0.2.2) |
| `objc2-quartz-core` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-quartz-core/0.3.2) |
| `objc2-user-notifications` | `0.3.2` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/objc2-user-notifications/0.3.2) |
| `objc_exception` | `0.1.2` | `MIT` | [crates.io](https://crates.io/crates/objc_exception/0.1.2) |
| `objc_id` | `0.1.1` | `MIT` | [crates.io](https://crates.io/crates/objc_id/0.1.1) |
| `object` | `0.37.3` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/object/0.37.3) |
| `once_cell` | `1.21.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/once_cell/1.21.4) |
| `once_cell_polyfill` | `1.70.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/once_cell_polyfill/1.70.2) |
| `oo7` | `0.6.0` | `MIT` | [crates.io](https://crates.io/crates/oo7/0.6.0) |
| `open` | `5.4.0` | `MIT` | [crates.io](https://crates.io/crates/open/5.4.0) |
| `openssl-probe` | `0.2.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/openssl-probe/0.2.1) |
| `option-ext` | `0.2.0` | `MPL-2.0` | [crates.io](https://crates.io/crates/option-ext/0.2.0) |
| `ordered-float` | `5.3.0` | `MIT` | [crates.io](https://crates.io/crates/ordered-float/5.3.0) |
| `ordered-stream` | `0.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ordered-stream/0.2.0) |
| `parking` | `2.2.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/parking/2.2.1) |
| `parking_lot` | `0.12.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/parking_lot/0.12.5) |
| `parking_lot_core` | `0.9.12` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/parking_lot_core/0.9.12) |
| `paste` | `1.0.15` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/paste/1.0.15) |
| `pastey` | `0.1.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/pastey/0.1.1) |
| `pathfinder_geometry` | `0.5.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/pathfinder_geometry/0.5.1) |
| `pathfinder_simd` | `0.5.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/pathfinder_simd/0.5.6) |
| `pbkdf2` | `0.12.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/pbkdf2/0.12.2) |
| `percent-encoding` | `2.3.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/percent-encoding/2.3.2) |
| `perf` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `phf` | `0.11.3` | `MIT` | [crates.io](https://crates.io/crates/phf/0.11.3) |
| `phf` | `0.13.1` | `MIT` | [crates.io](https://crates.io/crates/phf/0.13.1) |
| `phf_codegen` | `0.11.3` | `MIT` | [crates.io](https://crates.io/crates/phf_codegen/0.11.3) |
| `phf_generator` | `0.11.3` | `MIT` | [crates.io](https://crates.io/crates/phf_generator/0.11.3) |
| `phf_generator` | `0.13.1` | `MIT` | [crates.io](https://crates.io/crates/phf_generator/0.13.1) |
| `phf_macros` | `0.13.1` | `MIT` | [crates.io](https://crates.io/crates/phf_macros/0.13.1) |
| `phf_shared` | `0.11.3` | `MIT` | [crates.io](https://crates.io/crates/phf_shared/0.11.3) |
| `phf_shared` | `0.13.1` | `MIT` | [crates.io](https://crates.io/crates/phf_shared/0.13.1) |
| `pico-args` | `0.5.0` | `MIT` | [crates.io](https://crates.io/crates/pico-args/0.5.0) |
| `pin-project` | `1.1.13` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/pin-project/1.1.13) |
| `pin-project-internal` | `1.1.13` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/pin-project-internal/1.1.13) |
| `pin-project-lite` | `0.2.17` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/pin-project-lite/0.2.17) |
| `piper` | `0.2.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/piper/0.2.5) |
| `pkg-config` | `0.3.33` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/pkg-config/0.3.33) |
| `png` | `0.17.16` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/png/0.17.16) |
| `png` | `0.18.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/png/0.18.1) |
| `polling` | `3.11.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/polling/3.11.0) |
| `pollster` | `0.2.5` | `Apache-2.0/MIT` | [crates.io](https://crates.io/crates/pollster/0.2.5) |
| `pollster` | `0.4.0` | `Apache-2.0/MIT` | [crates.io](https://crates.io/crates/pollster/0.4.0) |
| `polycool` | `0.4.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/polycool/0.4.0) |
| `portable-atomic` | `1.14.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/portable-atomic/1.14.0) |
| `portable-atomic-util` | `0.2.7` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/portable-atomic-util/0.2.7) |
| `postage` | `0.5.0` | `MIT` | [crates.io](https://crates.io/crates/postage/0.5.0) |
| `potential_utf` | `0.1.5` | `Unicode-3.0` | [crates.io](https://crates.io/crates/potential_utf/0.1.5) |
| `powerfmt` | `0.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/powerfmt/0.2.0) |
| `ppv-lite86` | `0.2.21` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ppv-lite86/0.2.21) |
| `precomputed-hash` | `0.1.1` | `MIT` | [crates.io](https://crates.io/crates/precomputed-hash/0.1.1) |
| `presser` | `0.3.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/presser/0.3.1) |
| `prettyplease` | `0.2.37` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/prettyplease/0.2.37) |
| `proc-macro-crate` | `3.5.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/proc-macro-crate/3.5.0) |
| `proc-macro-error-attr2` | `2.0.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/proc-macro-error-attr2/2.0.0) |
| `proc-macro-error2` | `2.0.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/proc-macro-error2/2.0.1) |
| `proc-macro2` | `1.0.107` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/proc-macro2/1.0.107) |
| `profiling` | `1.0.18` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/profiling/1.0.18) |
| `profiling-procmacros` | `1.0.18` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/profiling-procmacros/1.0.18) |
| `psm` | `0.1.31` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/psm/0.1.31) |
| `pulp` | `0.22.3` | `MIT` | [crates.io](https://crates.io/crates/pulp/0.22.3) |
| `pulp-wasm-simd-flag` | `0.1.1` | `MIT` | [crates.io](https://crates.io/crates/pulp-wasm-simd-flag/0.1.1) |
| `pxfm` | `0.1.30` | `BSD-3-Clause OR Apache-2.0` | [crates.io](https://crates.io/crates/pxfm/0.1.30) |
| `qoi` | `0.4.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/qoi/0.4.1) |
| `quick-error` | `2.0.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/quick-error/2.0.1) |
| `quick-xml` | `0.30.0` | `MIT` | [crates.io](https://crates.io/crates/quick-xml/0.30.0) |
| `quick-xml` | `0.41.0` | `MIT` | [crates.io](https://crates.io/crates/quick-xml/0.41.0) |
| `quinn` | `0.11.11` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/quinn/0.11.11) |
| `quinn-proto` | `0.11.16` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/quinn-proto/0.11.16) |
| `quinn-udp` | `0.5.15` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/quinn-udp/0.5.15) |
| `quote` | `1.0.47` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/quote/1.0.47) |
| `r-efi` | `5.3.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | [crates.io](https://crates.io/crates/r-efi/5.3.0) |
| `r-efi` | `6.0.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | [crates.io](https://crates.io/crates/r-efi/6.0.0) |
| `rand` | `0.10.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand/0.10.2) |
| `rand` | `0.8.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand/0.8.7) |
| `rand` | `0.9.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand/0.9.5) |
| `rand_chacha` | `0.3.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand_chacha/0.3.1) |
| `rand_chacha` | `0.9.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand_chacha/0.9.0) |
| `rand_core` | `0.10.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand_core/0.10.1) |
| `rand_core` | `0.6.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand_core/0.6.4) |
| `rand_core` | `0.9.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand_core/0.9.5) |
| `rand_pcg` | `0.10.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rand_pcg/0.10.2) |
| `range-alloc` | `0.1.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/range-alloc/0.1.5) |
| `rangemap` | `1.7.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/rangemap/1.7.1) |
| `rav1e` | `0.8.1` | `BSD-2-Clause` | [crates.io](https://crates.io/crates/rav1e/0.8.1) |
| `ravif` | `0.13.0` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/ravif/0.13.0) |
| `raw-cpuid` | `11.6.0` | `MIT` | [crates.io](https://crates.io/crates/raw-cpuid/11.6.0) |
| `raw-window-handle` | `0.6.2` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/raw-window-handle/0.6.2) |
| `raw-window-metal` | `1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/raw-window-metal/1.1.0) |
| `rayon` | `1.12.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rayon/1.12.0) |
| `rayon-core` | `1.13.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rayon-core/1.13.0) |
| `read-fonts` | `0.37.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/read-fonts/0.37.0) |
| `read-fonts` | `0.41.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/read-fonts/0.41.0) |
| `reborrow` | `0.5.5` | `MIT` | [crates.io](https://crates.io/crates/reborrow/0.5.5) |
| `redox_syscall` | `0.5.18` | `MIT` | [crates.io](https://crates.io/crates/redox_syscall/0.5.18) |
| `redox_users` | `0.5.2` | `MIT` | [crates.io](https://crates.io/crates/redox_users/0.5.2) |
| `ref-cast` | `1.0.26` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ref-cast/1.0.26) |
| `ref-cast-impl` | `1.0.26` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ref-cast-impl/1.0.26) |
| `refineable` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `regex` | `1.13.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/regex/1.13.1) |
| `regex-automata` | `0.4.16` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/regex-automata/0.4.16) |
| `regex-syntax` | `0.8.11` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/regex-syntax/0.8.11) |
| `renderdoc-sys` | `1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/renderdoc-sys/1.1.0) |
| `resvg` | `0.45.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/resvg/0.45.1) |
| `resvg` | `0.46.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/resvg/0.46.0) |
| `rgb` | `0.8.53` | `MIT` | [crates.io](https://crates.io/crates/rgb/0.8.53) |
| `ring` | `0.17.14` | `Apache-2.0 AND ISC` | [crates.io](https://crates.io/crates/ring/0.17.14) |
| `ropey` | `2.0.0-beta.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ropey/2.0.0-beta.1) |
| `roxmltree` | `0.20.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/roxmltree/0.20.0) |
| `roxmltree` | `0.21.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/roxmltree/0.21.1) |
| `rust-embed` | `8.12.0` | `MIT` | [crates.io](https://crates.io/crates/rust-embed/8.12.0) |
| `rust-embed-impl` | `8.12.0` | `MIT` | [crates.io](https://crates.io/crates/rust-embed-impl/8.12.0) |
| `rust-embed-utils` | `8.12.0` | `MIT` | [crates.io](https://crates.io/crates/rust-embed-utils/8.12.0) |
| `rust-i18n` | `4.2.1` | `MIT` | [crates.io](https://crates.io/crates/rust-i18n/4.2.1) |
| `rust-i18n-macro` | `4.2.1` | `MIT` | [crates.io](https://crates.io/crates/rust-i18n-macro/4.2.1) |
| `rust-i18n-support` | `4.2.1` | `MIT` | [crates.io](https://crates.io/crates/rust-i18n-support/4.2.1) |
| `rustc-demangle` | `0.1.28` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/rustc-demangle/0.1.28) |
| `rustc-hash` | `1.1.0` | `Apache-2.0/MIT` | [crates.io](https://crates.io/crates/rustc-hash/1.1.0) |
| `rustc-hash` | `2.1.3` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/rustc-hash/2.1.3) |
| `rustc_version` | `0.4.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rustc_version/0.4.1) |
| `rustix` | `0.38.44` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/rustix/0.38.44) |
| `rustix` | `1.1.4` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/rustix/1.1.4) |
| `rustls` | `0.23.43` | `Apache-2.0 OR ISC OR MIT` | [crates.io](https://crates.io/crates/rustls/0.23.43) |
| `rustls-native-certs` | `0.8.4` | `Apache-2.0 OR ISC OR MIT` | [crates.io](https://crates.io/crates/rustls-native-certs/0.8.4) |
| `rustls-pemfile` | `2.2.0` | `Apache-2.0 OR ISC OR MIT` | [crates.io](https://crates.io/crates/rustls-pemfile/2.2.0) |
| `rustls-pki-types` | `1.15.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rustls-pki-types/1.15.1) |
| `rustls-webpki` | `0.103.13` | `ISC` | [crates.io](https://crates.io/crates/rustls-webpki/0.103.13) |
| `rustversion` | `1.0.23` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/rustversion/1.0.23) |
| `rustybuzz` | `0.20.1` | `MIT` | [crates.io](https://crates.io/crates/rustybuzz/0.20.1) |
| `ryu` | `1.0.23` | `Apache-2.0 OR BSL-1.0` | [crates.io](https://crates.io/crates/ryu/1.0.23) |
| `same-file` | `1.0.6` | `Unlicense/MIT` | [crates.io](https://crates.io/crates/same-file/1.0.6) |
| `schannel` | `0.1.29` | `MIT` | [crates.io](https://crates.io/crates/schannel/0.1.29) |
| `scheduler` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `schemars` | `1.2.2` | `MIT` | [crates.io](https://crates.io/crates/schemars/1.2.2) |
| `schemars_derive` | `1.2.2` | `MIT` | [crates.io](https://crates.io/crates/schemars_derive/1.2.2) |
| `scoped-tls` | `1.0.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/scoped-tls/1.0.1) |
| `scopeguard` | `1.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/scopeguard/1.2.0) |
| `screencapturekit` | `0.2.8` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/screencapturekit/0.2.8) |
| `screencapturekit-sys` | `0.2.8` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/screencapturekit-sys/0.2.8) |
| `seahash` | `4.1.0` | `MIT` | [crates.io](https://crates.io/crates/seahash/4.1.0) |
| `security-framework` | `3.7.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/security-framework/3.7.0) |
| `security-framework-sys` | `2.17.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/security-framework-sys/2.17.0) |
| `self_cell` | `1.3.0` | `Apache-2.0 OR GPL-2.0-only` | [crates.io](https://crates.io/crates/self_cell/1.3.0) |
| `semver` | `1.0.28` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/semver/1.0.28) |
| `serde` | `1.0.229` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde/1.0.229) |
| `serde-saphyr` | `0.0.29` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde-saphyr/0.0.29) |
| `serde_bytes` | `0.11.19` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_bytes/0.11.19) |
| `serde_core` | `1.0.229` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_core/1.0.229) |
| `serde_derive` | `1.0.229` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_derive/1.0.229) |
| `serde_derive_internals` | `0.30.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_derive_internals/0.30.0) |
| `serde_fmt` | `1.1.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/serde_fmt/1.1.0) |
| `serde_json` | `1.0.151` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_json/1.0.151) |
| `serde_repr` | `0.1.21` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_repr/0.1.21) |
| `serde_spanned` | `0.6.9` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_spanned/0.6.9) |
| `serde_spanned` | `1.1.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_spanned/1.1.1) |
| `serde_urlencoded` | `0.7.1` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/serde_urlencoded/0.7.1) |
| `serde_yaml` | `0.9.34+deprecated` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/serde_yaml/0.9.34+deprecated) |
| `sha1_smol` | `1.0.1` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/sha1_smol/1.0.1) |
| `sha2` | `0.10.9` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/sha2/0.10.9) |
| `sha2` | `0.11.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/sha2/0.11.0) |
| `sharded-slab` | `0.1.7` | `MIT` | [crates.io](https://crates.io/crates/sharded-slab/0.1.7) |
| `shellexpand` | `3.1.2` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/shellexpand/3.1.2) |
| `shlex` | `1.3.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/shlex/1.3.0) |
| `shlex` | `2.0.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/shlex/2.0.1) |
| `signal-hook-registry` | `1.4.8` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/signal-hook-registry/1.4.8) |
| `simd-adler32` | `0.3.10` | `MIT` | [crates.io](https://crates.io/crates/simd-adler32/0.3.10) |
| `simd_helpers` | `0.1.0` | `MIT` | [crates.io](https://crates.io/crates/simd_helpers/0.1.0) |
| `simplecss` | `0.2.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/simplecss/0.2.2) |
| `siphasher` | `1.0.3` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/siphasher/1.0.3) |
| `skrifa` | `0.40.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/skrifa/0.40.0) |
| `skrifa` | `0.44.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/skrifa/0.44.0) |
| `slab` | `0.4.12` | `MIT` | [crates.io](https://crates.io/crates/slab/0.4.12) |
| `slotmap` | `1.1.1` | `Zlib` | [crates.io](https://crates.io/crates/slotmap/1.1.1) |
| `smallvec` | `1.15.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/smallvec/1.15.2) |
| `smol` | `2.0.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/smol/2.0.2) |
| `smol_str` | `0.3.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/smol_str/0.3.6) |
| `socket2` | `0.6.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/socket2/0.6.5) |
| `spin` | `0.10.1` | `MIT` | [crates.io](https://crates.io/crates/spin/0.10.1) |
| `spin` | `0.9.9` | `MIT` | [crates.io](https://crates.io/crates/spin/0.9.9) |
| `spirv` | `0.4.0+sdk-1.4.341.0` | `Apache-2.0` | [crates.io](https://crates.io/crates/spirv/0.4.0+sdk-1.4.341.0) |
| `stable_deref_trait` | `1.2.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/stable_deref_trait/1.2.1) |
| `stacker` | `0.1.24` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/stacker/0.1.24) |
| `stacksafe` | `0.1.4` | `Apache-2.0` | [crates.io](https://crates.io/crates/stacksafe/0.1.4) |
| `stacksafe-macro` | `0.1.4` | `Apache-2.0` | [crates.io](https://crates.io/crates/stacksafe-macro/0.1.4) |
| `static_assertions` | `1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/static_assertions/1.1.0) |
| `storage-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `storage-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `storage-service` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `str_indices` | `0.4.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/str_indices/0.4.4) |
| `strict-num` | `0.1.1` | `MIT` | [crates.io](https://crates.io/crates/strict-num/0.1.1) |
| `string_cache` | `0.8.9` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/string_cache/0.8.9) |
| `string_cache_codegen` | `0.5.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/string_cache_codegen/0.5.4) |
| `strsim` | `0.11.1` | `MIT` | [crates.io](https://crates.io/crates/strsim/0.11.1) |
| `strum` | `0.27.2` | `MIT` | [crates.io](https://crates.io/crates/strum/0.27.2) |
| `strum_macros` | `0.27.2` | `MIT` | [crates.io](https://crates.io/crates/strum_macros/0.27.2) |
| `subtle` | `2.6.1` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/subtle/2.6.1) |
| `sum_tree` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `sval` | `2.21.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/sval/2.21.0) |
| `sval_buffer` | `2.21.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/sval_buffer/2.21.0) |
| `sval_dynamic` | `2.21.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/sval_dynamic/2.21.0) |
| `sval_fmt` | `2.21.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/sval_fmt/2.21.0) |
| `sval_json` | `2.21.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/sval_json/2.21.0) |
| `sval_nested` | `2.21.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/sval_nested/2.21.0) |
| `sval_ref` | `2.21.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/sval_ref/2.21.0) |
| `sval_serde` | `2.21.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/sval_serde/2.21.0) |
| `svg_fmt` | `0.4.5` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/svg_fmt/0.4.5) |
| `svgtypes` | `0.15.3` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/svgtypes/0.15.3) |
| `svgtypes` | `0.16.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/svgtypes/0.16.1) |
| `swash` | `0.2.10` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/swash/0.2.10) |
| `syn` | `2.0.119` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/syn/2.0.119) |
| `syn` | `3.0.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/syn/3.0.3) |
| `sync_wrapper` | `1.0.2` | `Apache-2.0` | [crates.io](https://crates.io/crates/sync_wrapper/1.0.2) |
| `synstructure` | `0.13.2` | `MIT` | [crates.io](https://crates.io/crates/synstructure/0.13.2) |
| `sys-locale` | `0.3.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/sys-locale/0.3.2) |
| `sysinfo` | `0.31.4` | `MIT` | [crates.io](https://crates.io/crates/sysinfo/0.31.4) |
| `system-configuration` | `0.6.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/system-configuration/0.6.1) |
| `system-configuration-sys` | `0.6.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/system-configuration-sys/0.6.0) |
| `taffy` | `0.12.2` | `MIT` | [crates.io](https://crates.io/crates/taffy/0.12.2) |
| `tao-core-video-sys` | `0.2.0` | `MIT` | [crates.io](https://crates.io/crates/tao-core-video-sys/0.2.0) |
| `tauri-winrt-notification` | `0.7.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/tauri-winrt-notification/0.7.3) |
| `tempfile` | `3.27.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/tempfile/3.27.0) |
| `tendril` | `0.4.3` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/tendril/0.4.3) |
| `termcolor` | `1.4.1` | `Unlicense OR MIT` | [crates.io](https://crates.io/crates/termcolor/1.4.1) |
| `thiserror` | `1.0.69` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/thiserror/1.0.69) |
| `thiserror` | `2.0.19` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/thiserror/2.0.19) |
| `thiserror-impl` | `1.0.69` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/thiserror-impl/1.0.69) |
| `thiserror-impl` | `2.0.19` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/thiserror-impl/2.0.19) |
| `thread_local` | `1.1.10` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/thread_local/1.1.10) |
| `tiff` | `0.11.3` | `MIT` | [crates.io](https://crates.io/crates/tiff/0.11.3) |
| `time` | `0.3.54` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/time/0.3.54) |
| `time-core` | `0.1.9` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/time-core/0.1.9) |
| `tiny-keccak` | `2.0.2` | `CC0-1.0` | [crates.io](https://crates.io/crates/tiny-keccak/2.0.2) |
| `tiny-skia` | `0.11.4` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/tiny-skia/0.11.4) |
| `tiny-skia-path` | `0.11.4` | `BSD-3-Clause` | [crates.io](https://crates.io/crates/tiny-skia-path/0.11.4) |
| `tinystr` | `0.8.3` | `Unicode-3.0` | [crates.io](https://crates.io/crates/tinystr/0.8.3) |
| `tinyvec` | `1.12.0` | `Zlib OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/tinyvec/1.12.0) |
| `tinyvec_macros` | `0.1.1` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/tinyvec_macros/0.1.1) |
| `tokio` | `1.53.1` | `MIT` | [crates.io](https://crates.io/crates/tokio/1.53.1) |
| `tokio-macros` | `2.7.2` | `MIT` | [crates.io](https://crates.io/crates/tokio-macros/2.7.2) |
| `tokio-rustls` | `0.26.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/tokio-rustls/0.26.4) |
| `tokio-socks` | `0.5.3` | `MIT` | [crates.io](https://crates.io/crates/tokio-socks/0.5.3) |
| `tokio-util` | `0.7.19` | `MIT` | [crates.io](https://crates.io/crates/tokio-util/0.7.19) |
| `toml` | `0.8.23` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml/0.8.23) |
| `toml` | `1.1.4+spec-1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml/1.1.4+spec-1.1.0) |
| `toml_datetime` | `0.6.11` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml_datetime/0.6.11) |
| `toml_datetime` | `1.1.1+spec-1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml_datetime/1.1.1+spec-1.1.0) |
| `toml_edit` | `0.22.27` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml_edit/0.22.27) |
| `toml_edit` | `0.25.13+spec-1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml_edit/0.25.13+spec-1.1.0) |
| `toml_parser` | `1.1.3+spec-1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml_parser/1.1.3+spec-1.1.0) |
| `toml_write` | `0.1.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml_write/0.1.2) |
| `toml_writer` | `1.1.2+spec-1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/toml_writer/1.1.2+spec-1.1.0) |
| `touchpad-core` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-gestures` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-gui` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-platform` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `touchpad-session` | `0.1.0` | `GPL-3.0-or-later` | workspace |
| `tower` | `0.5.3` | `MIT` | [crates.io](https://crates.io/crates/tower/0.5.3) |
| `tower-layer` | `0.3.3` | `MIT` | [crates.io](https://crates.io/crates/tower-layer/0.3.3) |
| `tower-service` | `0.3.3` | `MIT` | [crates.io](https://crates.io/crates/tower-service/0.3.3) |
| `tracing` | `0.1.44` | `MIT` | [crates.io](https://crates.io/crates/tracing/0.1.44) |
| `tracing-attributes` | `0.1.31` | `MIT` | [crates.io](https://crates.io/crates/tracing-attributes/0.1.31) |
| `tracing-core` | `0.1.36` | `MIT` | [crates.io](https://crates.io/crates/tracing-core/0.1.36) |
| `tracing-log` | `0.2.0` | `MIT` | [crates.io](https://crates.io/crates/tracing-log/0.2.0) |
| `tracing-subscriber` | `0.3.23` | `MIT` | [crates.io](https://crates.io/crates/tracing-subscriber/0.3.23) |
| `triomphe` | `0.1.16` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/triomphe/0.1.16) |
| `try-lock` | `0.2.5` | `MIT` | [crates.io](https://crates.io/crates/try-lock/0.2.5) |
| `ttf-parser` | `0.25.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ttf-parser/0.25.1) |
| `typeid` | `1.0.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/typeid/1.0.3) |
| `typenum` | `1.20.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/typenum/1.20.1) |
| `uds_windows` | `1.2.1` | `MIT` | [crates.io](https://crates.io/crates/uds_windows/1.2.1) |
| `unicase` | `2.9.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/unicase/2.9.0) |
| `unicode-bidi` | `0.3.18` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/unicode-bidi/0.3.18) |
| `unicode-bidi-mirroring` | `0.4.0` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/unicode-bidi-mirroring/0.4.0) |
| `unicode-ccc` | `0.4.0` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/unicode-ccc/0.4.0) |
| `unicode-id` | `0.3.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/unicode-id/0.3.6) |
| `unicode-ident` | `1.0.24` | `(MIT OR Apache-2.0) AND Unicode-3.0` | [crates.io](https://crates.io/crates/unicode-ident/1.0.24) |
| `unicode-linebreak` | `0.1.5` | `Apache-2.0` | [crates.io](https://crates.io/crates/unicode-linebreak/0.1.5) |
| `unicode-properties` | `0.1.4` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/unicode-properties/0.1.4) |
| `unicode-script` | `0.5.8` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/unicode-script/0.5.8) |
| `unicode-segmentation` | `1.13.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/unicode-segmentation/1.13.3) |
| `unicode-vo` | `0.1.0` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/unicode-vo/0.1.0) |
| `unicode-width` | `0.2.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/unicode-width/0.2.2) |
| `unicode-xid` | `0.2.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/unicode-xid/0.2.6) |
| `unsafe-libyaml` | `0.2.11` | `MIT` | [crates.io](https://crates.io/crates/unsafe-libyaml/0.2.11) |
| `untrusted` | `0.9.0` | `ISC` | [crates.io](https://crates.io/crates/untrusted/0.9.0) |
| `ureq` | `3.3.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ureq/3.3.0) |
| `ureq-proto` | `0.6.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/ureq-proto/0.6.0) |
| `url` | `2.5.8` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/url/2.5.8) |
| `usvg` | `0.45.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/usvg/0.45.1) |
| `usvg` | `0.46.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/usvg/0.46.0) |
| `utf-8` | `0.7.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/utf-8/0.7.6) |
| `utf8-zero` | `0.8.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/utf8-zero/0.8.1) |
| `utf8_iter` | `1.0.4` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/utf8_iter/1.0.4) |
| `utf8parse` | `0.2.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/utf8parse/0.2.2) |
| `util_macros` | `0.1.0` | `Apache-2.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `uuid` | `1.24.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/uuid/1.24.0) |
| `v_frame` | `0.3.9` | `BSD-2-Clause` | [crates.io](https://crates.io/crates/v_frame/0.3.9) |
| `valuable` | `0.1.1` | `MIT` | [crates.io](https://crates.io/crates/valuable/0.1.1) |
| `value-bag` | `1.13.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/value-bag/1.13.2) |
| `value-bag-serde1` | `1.13.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/value-bag-serde1/1.13.2) |
| `value-bag-sval2` | `1.13.2` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/value-bag-sval2/1.13.2) |
| `version_check` | `0.9.5` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/version_check/0.9.5) |
| `vswhom` | `0.1.0` | `MIT` | [crates.io](https://crates.io/crates/vswhom/0.1.0) |
| `vswhom-sys` | `0.1.3` | `MIT` | [crates.io](https://crates.io/crates/vswhom-sys/0.1.3) |
| `waker-fn` | `1.2.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/waker-fn/1.2.0) |
| `walkdir` | `2.5.0` | `Unlicense/MIT` | [crates.io](https://crates.io/crates/walkdir/2.5.0) |
| `want` | `0.3.1` | `MIT` | [crates.io](https://crates.io/crates/want/0.3.1) |
| `wasi` | `0.11.1+wasi-snapshot-preview1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/wasi/0.11.1+wasi-snapshot-preview1) |
| `wasip2` | `1.0.4+wasi-0.2.12` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/wasip2/1.0.4+wasi-0.2.12) |
| `wasm-bindgen` | `0.2.126` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wasm-bindgen/0.2.126) |
| `wasm-bindgen-futures` | `0.4.76` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wasm-bindgen-futures/0.4.76) |
| `wasm-bindgen-macro` | `0.2.126` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wasm-bindgen-macro/0.2.126) |
| `wasm-bindgen-macro-support` | `0.2.126` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wasm-bindgen-macro-support/0.2.126) |
| `wasm-bindgen-shared` | `0.2.126` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wasm-bindgen-shared/0.2.126) |
| `wasm-streams` | `0.4.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wasm-streams/0.4.2) |
| `wasm_thread` | `0.3.3` | `Apache-2.0 OR MIT` | `git+https://github.com/zed-industries/wasm_thread?rev=0cf96c7708dfb97ccf3da50347e25edcf75d6937#0cf96c7708dfb97ccf3da50347e25edcf75d6937` |
| `wayland-backend` | `0.3.16` | `MIT` | [crates.io](https://crates.io/crates/wayland-backend/0.3.16) |
| `wayland-client` | `0.31.15` | `MIT` | [crates.io](https://crates.io/crates/wayland-client/0.31.15) |
| `wayland-cursor` | `0.31.14` | `MIT` | [crates.io](https://crates.io/crates/wayland-cursor/0.31.14) |
| `wayland-protocols` | `0.32.13` | `MIT` | [crates.io](https://crates.io/crates/wayland-protocols/0.32.13) |
| `wayland-protocols-plasma` | `0.3.12` | `MIT` | [crates.io](https://crates.io/crates/wayland-protocols-plasma/0.3.12) |
| `wayland-protocols-wlr` | `0.3.12` | `MIT` | [crates.io](https://crates.io/crates/wayland-protocols-wlr/0.3.12) |
| `wayland-scanner` | `0.31.11` | `MIT` | [crates.io](https://crates.io/crates/wayland-scanner/0.31.11) |
| `wayland-sys` | `0.31.11` | `MIT` | [crates.io](https://crates.io/crates/wayland-sys/0.31.11) |
| `web-sys` | `0.3.103` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/web-sys/0.3.103) |
| `web-time` | `1.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/web-time/1.1.0) |
| `webpki-roots` | `1.0.9` | `CDLA-Permissive-2.0` | [crates.io](https://crates.io/crates/webpki-roots/1.0.9) |
| `weezl` | `0.1.12` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/weezl/0.1.12) |
| `wgpu` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wgpu/29.0.4) |
| `wgpu-core` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wgpu-core/29.0.4) |
| `wgpu-core-deps-apple` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wgpu-core-deps-apple/29.0.4) |
| `wgpu-core-deps-emscripten` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wgpu-core-deps-emscripten/29.0.4) |
| `wgpu-core-deps-windows-linux-android` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wgpu-core-deps-windows-linux-android/29.0.4) |
| `wgpu-hal` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wgpu-hal/29.0.4) |
| `wgpu-naga-bridge` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wgpu-naga-bridge/29.0.4) |
| `wgpu-types` | `29.0.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/wgpu-types/29.0.4) |
| `which` | `6.0.3` | `MIT` | [crates.io](https://crates.io/crates/which/6.0.3) |
| `winapi` | `0.3.9` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/winapi/0.3.9) |
| `winapi-i686-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/winapi-i686-pc-windows-gnu/0.4.0) |
| `winapi-util` | `0.1.11` | `Unlicense OR MIT` | [crates.io](https://crates.io/crates/winapi-util/0.1.11) |
| `winapi-x86_64-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/winapi-x86_64-pc-windows-gnu/0.4.0) |
| `windows` | `0.57.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows/0.57.0) |
| `windows` | `0.58.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows/0.58.0) |
| `windows` | `0.61.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows/0.61.3) |
| `windows` | `0.62.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows/0.62.2) |
| `windows-capture` | `1.5.0` | `MIT` | [crates.io](https://crates.io/crates/windows-capture/1.5.0) |
| `windows-collections` | `0.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-collections/0.2.0) |
| `windows-collections` | `0.3.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-collections/0.3.2) |
| `windows-core` | `0.57.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-core/0.57.0) |
| `windows-core` | `0.58.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-core/0.58.0) |
| `windows-core` | `0.61.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-core/0.61.2) |
| `windows-core` | `0.62.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-core/0.62.2) |
| `windows-future` | `0.2.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-future/0.2.1) |
| `windows-future` | `0.3.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-future/0.3.2) |
| `windows-implement` | `0.57.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-implement/0.57.0) |
| `windows-implement` | `0.58.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-implement/0.58.0) |
| `windows-implement` | `0.60.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-implement/0.60.2) |
| `windows-interface` | `0.57.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-interface/0.57.0) |
| `windows-interface` | `0.58.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-interface/0.58.0) |
| `windows-interface` | `0.59.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-interface/0.59.3) |
| `windows-link` | `0.1.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-link/0.1.3) |
| `windows-link` | `0.2.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-link/0.2.1) |
| `windows-numerics` | `0.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-numerics/0.2.0) |
| `windows-numerics` | `0.3.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-numerics/0.3.1) |
| `windows-registry` | `0.4.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-registry/0.4.0) |
| `windows-registry` | `0.5.3` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-registry/0.5.3) |
| `windows-result` | `0.1.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-result/0.1.2) |
| `windows-result` | `0.2.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-result/0.2.0) |
| `windows-result` | `0.3.4` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-result/0.3.4) |
| `windows-result` | `0.4.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-result/0.4.1) |
| `windows-strings` | `0.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-strings/0.1.0) |
| `windows-strings` | `0.3.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-strings/0.3.1) |
| `windows-strings` | `0.4.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-strings/0.4.2) |
| `windows-strings` | `0.5.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-strings/0.5.1) |
| `windows-sys` | `0.52.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-sys/0.52.0) |
| `windows-sys` | `0.59.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-sys/0.59.0) |
| `windows-sys` | `0.61.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-sys/0.61.2) |
| `windows-targets` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-targets/0.52.6) |
| `windows-targets` | `0.53.5` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-targets/0.53.5) |
| `windows-threading` | `0.1.0` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-threading/0.1.0) |
| `windows-threading` | `0.2.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-threading/0.2.1) |
| `windows-version` | `0.1.7` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows-version/0.1.7) |
| `windows_aarch64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_aarch64_gnullvm/0.52.6) |
| `windows_aarch64_gnullvm` | `0.53.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_aarch64_gnullvm/0.53.1) |
| `windows_aarch64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_aarch64_msvc/0.52.6) |
| `windows_aarch64_msvc` | `0.53.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_aarch64_msvc/0.53.1) |
| `windows_i686_gnu` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_i686_gnu/0.52.6) |
| `windows_i686_gnu` | `0.53.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_i686_gnu/0.53.1) |
| `windows_i686_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_i686_gnullvm/0.52.6) |
| `windows_i686_gnullvm` | `0.53.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_i686_gnullvm/0.53.1) |
| `windows_i686_msvc` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_i686_msvc/0.52.6) |
| `windows_i686_msvc` | `0.53.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_i686_msvc/0.53.1) |
| `windows_x86_64_gnu` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_x86_64_gnu/0.52.6) |
| `windows_x86_64_gnu` | `0.53.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_x86_64_gnu/0.53.1) |
| `windows_x86_64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_x86_64_gnullvm/0.52.6) |
| `windows_x86_64_gnullvm` | `0.53.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_x86_64_gnullvm/0.53.1) |
| `windows_x86_64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_x86_64_msvc/0.52.6) |
| `windows_x86_64_msvc` | `0.53.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/windows_x86_64_msvc/0.53.1) |
| `winnow` | `0.7.15` | `MIT` | [crates.io](https://crates.io/crates/winnow/0.7.15) |
| `winnow` | `1.0.4` | `MIT` | [crates.io](https://crates.io/crates/winnow/1.0.4) |
| `winreg` | `0.55.0` | `MIT` | [crates.io](https://crates.io/crates/winreg/0.55.0) |
| `winsafe` | `0.0.19` | `MIT` | [crates.io](https://crates.io/crates/winsafe/0.0.19) |
| `wio` | `0.2.2` | `MIT/Apache-2.0` | [crates.io](https://crates.io/crates/wio/0.2.2) |
| `wit-bindgen` | `0.57.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/wit-bindgen/0.57.1) |
| `workspace-hack` | `0.1.0` | `CC0-1.0` | [crates.io](https://crates.io/crates/workspace-hack/0.1.0) |
| `writeable` | `0.6.3` | `Unicode-3.0` | [crates.io](https://crates.io/crates/writeable/0.6.3) |
| `x11` | `2.21.0` | `MIT` | [crates.io](https://crates.io/crates/x11/2.21.0) |
| `x11-clipboard` | `0.9.3` | `MIT` | [crates.io](https://crates.io/crates/x11-clipboard/0.9.3) |
| `x11rb` | `0.13.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/x11rb/0.13.2) |
| `x11rb-protocol` | `0.13.2` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/x11rb-protocol/0.13.2) |
| `xcb` | `1.7.0` | `MIT` | [crates.io](https://crates.io/crates/xcb/1.7.0) |
| `xcursor` | `0.3.10` | `MIT` | [crates.io](https://crates.io/crates/xcursor/0.3.10) |
| `xim-ctext` | `0.3.0` | `MIT` | `git+https://github.com/zed-industries/xim-rs.git?rev=16f35a2c881b815a2b6cdfd6687988e84f8447d8#16f35a2c881b815a2b6cdfd6687988e84f8447d8` |
| `xim-parser` | `0.2.1` | `MIT` | `git+https://github.com/zed-industries/xim-rs.git?rev=16f35a2c881b815a2b6cdfd6687988e84f8447d8#16f35a2c881b815a2b6cdfd6687988e84f8447d8` |
| `xkbcommon` | `0.8.0` | `MIT` | [crates.io](https://crates.io/crates/xkbcommon/0.8.0) |
| `xkeysym` | `0.2.1` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/xkeysym/0.2.1) |
| `xml-rs` | `0.8.28` | `MIT` | [crates.io](https://crates.io/crates/xml-rs/0.8.28) |
| `xml5ever` | `0.18.1` | `MIT OR Apache-2.0` | [crates.io](https://crates.io/crates/xml5ever/0.18.1) |
| `xmlwriter` | `0.1.0` | `MIT` | [crates.io](https://crates.io/crates/xmlwriter/0.1.0) |
| `y4m` | `0.8.0` | `MIT` | [crates.io](https://crates.io/crates/y4m/0.8.0) |
| `yazi` | `0.2.1` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/yazi/0.2.1) |
| `yeslogic-fontconfig-sys` | `6.0.1` | `MIT` | [crates.io](https://crates.io/crates/yeslogic-fontconfig-sys/6.0.1) |
| `yoke` | `0.8.3` | `Unicode-3.0` | [crates.io](https://crates.io/crates/yoke/0.8.3) |
| `yoke-derive` | `0.8.2` | `Unicode-3.0` | [crates.io](https://crates.io/crates/yoke-derive/0.8.2) |
| `zbus` | `5.18.0` | `MIT` | [crates.io](https://crates.io/crates/zbus/5.18.0) |
| `zbus-lockstep` | `0.5.2` | `MIT` | [crates.io](https://crates.io/crates/zbus-lockstep/0.5.2) |
| `zbus-lockstep-macros` | `0.5.2` | `MIT` | [crates.io](https://crates.io/crates/zbus-lockstep-macros/0.5.2) |
| `zbus_macros` | `5.18.0` | `MIT` | [crates.io](https://crates.io/crates/zbus_macros/5.18.0) |
| `zbus_names` | `4.3.4` | `MIT` | [crates.io](https://crates.io/crates/zbus_names/4.3.4) |
| `zbus_polkit` | `5.0.0` | `MIT` | [crates.io](https://crates.io/crates/zbus_polkit/5.0.0) |
| `zbus_xml` | `5.2.1` | `MIT` | [crates.io](https://crates.io/crates/zbus_xml/5.2.1) |
| `zed-font-kit` | `0.14.1-zed` | `MIT OR Apache-2.0` | `git+https://github.com/zed-industries/font-kit?rev=94b0f28166665e8fd2f53ff6d268a14955c82269#94b0f28166665e8fd2f53ff6d268a14955c82269` |
| `zed-reqwest` | `0.12.15-zed` | `MIT OR Apache-2.0` | `git+https://github.com/zed-industries/reqwest.git?rev=c15662463bda39148ba154100dd44d3fba5873a4#c15662463bda39148ba154100dd44d3fba5873a4` |
| `zed-scap` | `0.0.8-zed` | `MIT` | `git+https://github.com/zed-industries/scap?rev=4afea48c3b002197176fb19cd0f9b180dd36eaac#4afea48c3b002197176fb19cd0f9b180dd36eaac` |
| `zed-sum-tree` | `0.2.0` | `Apache-2.0` | [crates.io](https://crates.io/crates/zed-sum-tree/0.2.0) |
| `zed-xim` | `0.4.0-zed` | `MIT` | `git+https://github.com/zed-industries/xim-rs.git?rev=16f35a2c881b815a2b6cdfd6687988e84f8447d8#16f35a2c881b815a2b6cdfd6687988e84f8447d8` |
| `zeno` | `0.3.3` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/zeno/0.3.3) |
| `zerocopy` | `0.8.55` | `BSD-2-Clause OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/zerocopy/0.8.55) |
| `zerocopy-derive` | `0.8.55` | `BSD-2-Clause OR Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/zerocopy-derive/0.8.55) |
| `zerofrom` | `0.1.8` | `Unicode-3.0` | [crates.io](https://crates.io/crates/zerofrom/0.1.8) |
| `zerofrom-derive` | `0.1.7` | `Unicode-3.0` | [crates.io](https://crates.io/crates/zerofrom-derive/0.1.7) |
| `zeroize` | `1.9.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/zeroize/1.9.0) |
| `zeroize_derive` | `1.5.0` | `Apache-2.0 OR MIT` | [crates.io](https://crates.io/crates/zeroize_derive/1.5.0) |
| `zerotrie` | `0.2.4` | `Unicode-3.0` | [crates.io](https://crates.io/crates/zerotrie/0.2.4) |
| `zerovec` | `0.11.6` | `Unicode-3.0` | [crates.io](https://crates.io/crates/zerovec/0.11.6) |
| `zerovec-derive` | `0.11.3` | `Unicode-3.0` | [crates.io](https://crates.io/crates/zerovec-derive/0.11.3) |
| `zlog` | `0.1.0` | `GPL-3.0-or-later` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `zmij` | `1.0.23` | `MIT` | [crates.io](https://crates.io/crates/zmij/1.0.23) |
| `ztracing` | `0.1.0` | `GPL-3.0-or-later` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `ztracing_macro` | `0.1.0` | `GPL-3.0-or-later` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` |
| `zune-core` | `0.4.12` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/zune-core/0.4.12) |
| `zune-core` | `0.5.1` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/zune-core/0.5.1) |
| `zune-inflate` | `0.2.54` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/zune-inflate/0.2.54) |
| `zune-jpeg` | `0.4.21` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/zune-jpeg/0.4.21) |
| `zune-jpeg` | `0.5.15` | `MIT OR Apache-2.0 OR Zlib` | [crates.io](https://crates.io/crates/zune-jpeg/0.5.15) |
| `zvariant` | `5.13.1` | `MIT` | [crates.io](https://crates.io/crates/zvariant/5.13.1) |
| `zvariant_derive` | `5.13.1` | `MIT` | [crates.io](https://crates.io/crates/zvariant_derive/5.13.1) |
| `zvariant_utils` | `3.5.0` | `MIT` | [crates.io](https://crates.io/crates/zvariant_utils/3.5.0) |

## Packages requiring metadata review

| Package | Version | Source | Review note |
| --- | --- | --- | --- |
| `gpui_shared_string` | `0.1.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` | Pinned upstream Zed workspace package has no package-level license metadata; retain the upstream GPL/APACHE notices and review file-level markings. |
| `gpui_util` | `0.1.0` | `git+https://github.com/zed-industries/zed#ae394f3d474f4996d2cdef6ee97551fdb6748acd` | Pinned upstream Zed workspace package has no package-level license metadata; retain the upstream GPL/APACHE notices and review file-level markings. |
