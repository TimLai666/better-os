#!/usr/bin/env bash
# Run as root inside a disposable VM (never on a real machine). Installs the
# Better OS session's pinned COSMIC closure the way the session will
# (better-os-session.md, "Packages and where they come from"; ADR 0038 C2):
# System76's source as deb822, Signed-By a keyring holding only
# 63C4 6DF0 140D 7389 6142 9F4E 204D D8AE C33A 7AFF, a pin that keeps the
# whole origin at -1 and the 14 names at 500, the display-manager question
# preseeded to gdm3, then the 14 and Fcitx 5 with Chewing.
#
#   install-closure.sh KEYRING OUTDIR
#
# KEYRING is the binary keyring with only that key; OUTDIR receives every
# record (package lists, policies, the simulated and real transactions).
set -euo pipefail
KEYRING="$1"; OUT="$2"; mkdir -p "$OUT"
FPR=63C46DF0140D738961429F4E204DD8AEC33A7AFF
CLOSURE=(adw-gtk3 cosmic-bg cosmic-comp cosmic-greeter cosmic-greeter-daemon cosmic-icons
    cosmic-idle cosmic-osd cosmic-randr cosmic-settings cosmic-settings-daemon
    cosmic-theme-bundles pop-sound-theme xdg-desktop-portal-cosmic)
FCITX=(fcitx5 fcitx5-chewing fcitx5-frontend-gtk3 fcitx5-frontend-gtk4 fcitx5-frontend-qt5 fcitx5-config-qt)
export DEBIAN_FRONTEND=noninteractive

# The keyring must hold exactly the one key.
keys="$(gpg --show-keys --with-colons "$KEYRING" | awk -F: '$1=="fpr"{print $10}' | head -1)"
nkeys="$(gpg --show-keys --with-colons "$KEYRING" | grep -c '^pub:')"
[[ "$keys" == "$FPR" && "$nkeys" == 1 ]] || { echo "keyring is not exactly $FPR" >&2; exit 1; }

# Before.
dpkg-query -W -f='${Package}\t${Version}\t${Status}\n' | sort >"$OUT/dpkg-before.tsv"
readlink -f /etc/systemd/system/display-manager.service >"$OUT/display-manager-before.txt"
cat /etc/X11/default-display-manager >>"$OUT/display-manager-before.txt" 2>&1 || true
debconf-show gdm3 >"$OUT/debconf-gdm3-before.txt" 2>&1 || true

install -d -m 0755 /etc/apt/keyrings
install -m 0644 "$KEYRING" /etc/apt/keyrings/better-os-system76.gpg
cat >/etc/apt/sources.list.d/better-os-system76.sources <<'EOF'
Types: deb
URIs: https://apt.pop-os.org/release
Suites: noble
Components: main
Signed-By: /etc/apt/keyrings/better-os-system76.gpg
EOF
# The specific stanza comes first: APT uses the first record that matches a
# package, so the 14 names get 500 and everything else from the origin -1.
cat >/etc/apt/preferences.d/better-os-system76.pref <<EOF
Package: ${CLOSURE[*]}
Pin: origin "apt.pop-os.org"
Pin-Priority: 500

Package: *
Pin: origin "apt.pop-os.org"
Pin-Priority: -1
EOF
# cosmic-greeter asks which display manager to use; GDM stays (C3).
for owner in cosmic-greeter gdm3; do
    echo "$owner shared/default-x-display-manager select gdm3" | debconf-set-selections
done

apt-get update -q >"$OUT/apt-update.txt" 2>&1
apt-cache policy >"$OUT/apt-cache-policy.txt"
apt-cache policy "${CLOSURE[@]}" "${FCITX[@]}" >"$OUT/apt-cache-policy-closure.txt"
apt-get -s -o Debug::NoLocking=1 dist-upgrade >"$OUT/sim-dist-upgrade-before-install.txt"

# The transaction, simulated, with and without Recommends.
apt-get -s install "${CLOSURE[@]}" "${FCITX[@]}" >"$OUT/sim-install.txt"
apt-get -s --no-install-recommends install "${CLOSURE[@]}" "${FCITX[@]}" >"$OUT/sim-install-no-recommends.txt"
for f in sim-install sim-install-no-recommends; do
    {
        echo "== Inst (new or upgraded) =="; grep '^Inst ' "$OUT/$f.txt" || true
        echo "== Remv =="; grep '^Remv ' "$OUT/$f.txt" || true
        echo "== Inst replacing an installed version (an upgrade or downgrade of a base package) =="
        grep -E '^Inst [^ ]+ \[' "$OUT/$f.txt" || echo none
        echo "== from System76's origin =="; grep '^Inst ' "$OUT/$f.txt" | grep -i 'pop-os' || true
    } >"$OUT/$f.summary.txt"
done

# The real install (with Recommends, as apt does by default).
apt-get install -y -q "${CLOSURE[@]}" "${FCITX[@]}" >"$OUT/install.txt" 2>&1

dpkg-query -W -f='${Package}\t${Version}\t${Status}\n' | sort >"$OUT/dpkg-after-install.tsv"
apt-get -s dist-upgrade >"$OUT/sim-dist-upgrade-after-install.txt"
readlink -f /etc/systemd/system/display-manager.service >"$OUT/display-manager-after.txt"
cat /etc/X11/default-display-manager >>"$OUT/display-manager-after.txt" 2>&1 || true

# Every installed package whose installed version comes from System76's lists.
pop_lists=(/var/lib/apt/lists/apt.pop-os.org_release_dists_noble_main_binary-*_Packages)
awk '/^Package:/{p=$2} /^Version:/{print p"\t"$2}' "${pop_lists[@]}" | sort -u >"$OUT/pop-index.tsv"
awk -F'\t' '$3 ~ /installed$/ {print $1"\t"$2}' "$OUT/dpkg-after-install.tsv" | sort >"$OUT/installed.tsv"
comm -12 "$OUT/pop-index.tsv" "$OUT/installed.tsv" >"$OUT/installed-from-system76.tsv"
# Packages present before whose version changed: any is a base package replaced.
join -t$'\t' <(cut -f1,2 "$OUT/dpkg-before.tsv") <(cut -f1,2 "$OUT/dpkg-after-install.tsv") |
    awk -F'\t' '$2 != $3' >"$OUT/changed-versions.tsv"
comm -13 <(cut -f1 "$OUT/dpkg-before.tsv") <(cut -f1 "$OUT/dpkg-after-install.tsv") >"$OUT/added-packages.txt"
comm -23 <(cut -f1 "$OUT/dpkg-before.tsv") <(cut -f1 "$OUT/dpkg-after-install.tsv") >"$OUT/removed-packages.txt"
{
    echo "added: $(wc -l <"$OUT/added-packages.txt")"
    echo "removed: $(wc -l <"$OUT/removed-packages.txt")"
    echo "pre-existing packages whose version changed: $(wc -l <"$OUT/changed-versions.tsv")"
    echo "installed from System76: $(wc -l <"$OUT/installed-from-system76.tsv")"
    cut -f1 "$OUT/installed-from-system76.tsv" | tr '\n' ' '; echo
    echo "dist-upgrade after install takes from System76:"
    grep '^Inst ' "$OUT/sim-dist-upgrade-after-install.txt" | grep -i pop-os || echo "  nothing"
    echo "dist-upgrade after install, all Inst lines: $(grep -c '^Inst ' "$OUT/sim-dist-upgrade-after-install.txt" || true)"
    echo "display-manager.service before: $(head -1 "$OUT/display-manager-before.txt")"
    echo "display-manager.service after:  $(head -1 "$OUT/display-manager-after.txt")"
} >"$OUT/summary.txt"
cat "$OUT/summary.txt"
