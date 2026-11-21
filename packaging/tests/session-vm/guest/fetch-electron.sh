#!/usr/bin/env bash
# Run as root inside a disposable VM. Fetches Electron's own release build
# (HTTPS, checked against the release's SHASUMS256.txt) to /opt/vm201-electron,
# for the Chinese input check in an Electron application (upstream issue 2952).
set -euo pipefail
VER=v33.2.1
ZIP="electron-$VER-linux-x64.zip"
BASE="https://github.com/electron/electron/releases/download/$VER"
mkdir -p /opt/vm201-electron && cd /opt/vm201-electron
curl -sSfLO "$BASE/$ZIP"
curl -sSfLO "$BASE/SHASUMS256.txt"
grep " \*$ZIP\$" SHASUMS256.txt | sed 's/\*//' | sha256sum -c
rm -rf dist && unzip -q "$ZIP" -d dist && rm "$ZIP"
ls dist/electron
