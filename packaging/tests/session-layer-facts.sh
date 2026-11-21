#!/usr/bin/env bash
# Ticket 200's facts: what a GPUI layer-shell surface does on cosmic-comp,
# gathered in the nested test compositor (packaging/tests/cosmic-nested.sh).
# Each case starts its own compositor, runs better-layer-check and the probe,
# prints what it saw, saves screenshots into $OUT, and stops the compositor.
#
#   cargo build -p layer-check
#   packaging/tests/cosmic-nested.sh fetch
#   packaging/tests/session-layer-facts.sh OUT_DIR [CASE...]
#
# Cases: layers anchors zone zone-in-place anchor-change keyboard popup input-region
# scale outputs (all of them when none is named). Every program it starts is
# ended by PID; the compositor by `cosmic-nested.sh stop`.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NESTED="$HERE/cosmic-nested.sh"
BIN="${BETTER_LAYER_CHECK_BIN:-$HERE/../../target/debug}"
CHECK="$BIN/better-layer-check"
export BETTER_WL_PROBE="$BIN/better-wl-probe"
OUT="${1:?usage: session-layer-facts.sh OUT_DIR [CASE...]}"
shift
mkdir -p "$OUT/logs"
[[ -x "$CHECK" && -x "$BETTER_WL_PROBE" ]] || { echo "build crates/layer-check first" >&2; exit 1; }

PIDS=()
nested() { bash "$NESTED" "$@"; }
# Starts a check program in the nested compositor; its output goes to LOG.
run() {
    local log="$1"; shift
    # Not through the `nested` function: a backgrounded function is a
    # subshell, and its PID would not be the program's.
    bash "$NESTED" exec -- "$CHECK" "$@" > "$OUT/logs/$log" 2>&1 &
    PIDS+=("$!")
    sleep "${SETTLE:-2.5}"
}
end_programs() {
    local pid
    # Before anything ends: nothing installed may have been activated on the
    # sandbox's bus while the programs ran.
    ((${#PIDS[@]})) && nested bus-check
    for pid in "${PIDS[@]}"; do
        # `nested exec` execs the program, so this PID is the program itself.
        kill "$pid" 2>/dev/null
    done
    for pid in "${PIDS[@]}"; do wait "$pid" 2>/dev/null; done
    PIDS=()
}
finish() { end_programs; nested stop >/dev/null 2>&1; }
trap finish EXIT
events() { grep -h -E "^(open|closed|surface|active|click|key|popup|toplevel|spec|region|error)" "$OUT/logs/$1" || true; }
find_color() { nested exec -- "$BETTER_WL_PROBE" find "$1" "$2" 2>&1; }
fact() { printf '\n== %s\n' "$*"; }

case_layers() {
    fact "layers: one surface on each layer, crossing a window"
    nested start >/dev/null
    run layers-window.log --toplevel
    local margin=200 layer color
    for layer in background:c0392b bottom:e67e22 top:2f6fdf overlay:8e44ad; do
        color="${layer#*:}"; layer="${layer%%:*}"
        run "layers-$layer.log" --layer "$layer" --anchor left --width 200 --height 600 \
            --margin "0,0,0,$margin" --color "$color" --label "$layer"
        margin=$((margin + 220))
    done
    nested shot "$OUT/02-layers-around-a-window.png"
    for color in c0392b e67e22 2f6fdf 8e44ad 1a9e5c; do find_color "$OUT/02-layers-around-a-window.png" "$color"; done
    end_programs; nested stop >/dev/null
}

case_anchors() {
    fact "anchors: every combination of the four edges, 200x120 unless filled"
    nested start >/dev/null
    local edges
    for edges in none top bottom left right top,bottom left,right top,left top,right bottom,left \
        bottom,right top,left,right bottom,left,right top,bottom,left top,bottom,right top,bottom,left,right; do
        run "anchor-$edges.log" --layer overlay --anchor "$edges" --zone none --label "$edges"
        nested shot "$OUT/logs/anchor-$edges.png" >/dev/null
        printf '%-20s %s | %s\n' "$edges" "$(events "anchor-$edges.log" | grep surface | tail -1)" \
            "$(find_color "$OUT/logs/anchor-$edges.png" 2f6fdf)"
        end_programs
    done
    nested stop >/dev/null
}

case_zone() {
    fact "exclusive zone: a maximized window leaves the strip free, also after the zone changes"
    nested start >/dev/null
    run zone-strip.log --zone-after 4:64
    run zone-window.log --toplevel --maximized
    nested shot "$OUT/03-zone-32-maximized-window.png"
    find_color "$OUT/03-zone-32-maximized-window.png" 1a9e5c
    sleep 3
    nested shot "$OUT/04-zone-changed-to-64.png"
    find_color "$OUT/04-zone-changed-to-64.png" 2f6fdf
    find_color "$OUT/04-zone-changed-to-64.png" 1a9e5c
    events zone-strip.log; events zone-window.log
    end_programs; nested stop >/dev/null
}

case_zone_in_place() {
    fact "exclusive zone changed in place (Window::set_exclusive_zone): the strip stays 32 high, the zone becomes 96"
    nested start >/dev/null
    run zip-strip.log --zone-after 4:96 --zone-only
    run zip-window.log --toplevel --maximized
    sleep 3
    nested shot "$OUT/04b-zone-changed-in-place-to-96.png"
    find_color "$OUT/04b-zone-changed-in-place-to-96.png" 2f6fdf
    find_color "$OUT/04b-zone-changed-in-place-to-96.png" 1a9e5c
    events zip-strip.log; events zip-window.log
    end_programs; nested stop >/dev/null
}

case_anchor_change() {
    fact "anchor change: the strip moves from the top to the bottom (a new surface)"
    nested start >/dev/null
    run move-strip.log --anchor-after 4:bottom,left,right
    run move-window.log --toplevel --maximized
    sleep 3
    nested shot "$OUT/05-strip-recreated-at-the-bottom.png"
    find_color "$OUT/05-strip-recreated-at-the-bottom.png" 2f6fdf
    find_color "$OUT/05-strip-recreated-at-the-bottom.png" 1a9e5c
    events move-strip.log; events move-window.log
    end_programs; nested stop >/dev/null
}

case_keyboard() {
    local mode
    for mode in none on-demand exclusive; do
        fact "keyboard $mode: a window has focus, keys typed, the strip clicked, keys typed, the window clicked, keys typed"
        nested start >/dev/null
        run "kb-$mode-window.log" --toplevel
        run "kb-$mode-strip.log" --keyboard "$mode" --zone none
        nested keys ab; sleep 0.5
        nested shot "$OUT/logs/kb-$mode-1.png" >/dev/null
        nested click 400 16; sleep 0.8
        nested keys cd; sleep 0.5
        nested shot "$OUT/06-keyboard-$mode-after-clicking-the-strip.png" >/dev/null
        nested click 640 400; sleep 0.8
        nested keys ef; sleep 0.5
        echo "-- window:"; events "kb-$mode-window.log"
        echo "-- strip:"; events "kb-$mode-strip.log"
        end_programs; nested stop >/dev/null
    done
}

case_popup() {
    local mode
    for mode in none on-demand; do
        fact "popup from a strip with keyboard $mode: click opens it, keys go to it, Escape closes it"
        nested start >/dev/null
        run "popup-$mode-window.log" --toplevel
        run "popup-$mode-strip.log" --popup --keyboard "$mode"
        nested click 300 16; sleep 1.5
        nested keys xy; sleep 0.5
        nested shot "$OUT/07-popup-keyboard-$mode.png" >/dev/null
        nested keys '{Escape}'; sleep 0.8
        nested keys z; sleep 0.5
        echo "-- strip:"; events "popup-$mode-strip.log"
        echo "-- window:"; events "popup-$mode-window.log"
        end_programs; nested stop >/dev/null
    done
}

case_input_region() {
    fact "input region: an overlay over a maximized window takes clicks only in its left 300 pixels"
    nested start >/dev/null
    run region-window.log --toplevel --maximized
    run region-overlay.log --layer overlay --anchor top,left,right --height 300 --zone none \
        --input-left 300 --color 8e44ad --label "input only left of the yellow line"
    nested click 100 150; sleep 0.5
    nested click 800 150; sleep 0.5
    nested shot "$OUT/08-input-region-left-300.png" >/dev/null
    echo "-- overlay:"; events region-overlay.log
    echo "-- window:"; events region-window.log
    end_programs; nested stop >/dev/null
}

case_scale() {
    local scale
    for scale in 1 1.5 2; do
        fact "scale $scale: a full-width strip with a popup, a full-height strip, and a maximized window on cosmic-comp's output at $scale"
        nested start --scale "$scale" >/dev/null
        nested exec -- "$BETTER_WL_PROBE" outputs
        # The surfaces first: cosmic-comp does not move a window maximized
        # before a surface's zone appears (ticket 200's notes).
        run "scale-$scale-strip.log" --popup
        run "scale-$scale-side.log" --anchor top,bottom,left --width 48 --color 8e44ad --label side
        run "scale-$scale-window.log" --toplevel --maximized
        nested click 300 16; sleep 1.5
        nested shot "$OUT/09-scale-$scale-strip-popup-window.png"
        find_color "$OUT/09-scale-$scale-strip-popup-window.png" 2f6fdf
        find_color "$OUT/09-scale-$scale-strip-popup-window.png" 8e44ad
        find_color "$OUT/09-scale-$scale-strip-popup-window.png" 1a9e5c
        events "scale-$scale-strip.log"; events "scale-$scale-side.log"; events "scale-$scale-window.log"
        grep -h -m1 'error 0' "$OUT/logs/scale-$scale-strip.log" "$OUT/logs/scale-$scale-side.log" || true
        end_programs; nested stop >/dev/null
    done
}

# Two outputs, a strip on each following them: one added, one removed, one
# added again. On headless sway, because cosmic-comp's nested backends have
# one output. $1 is the scale, the rest are better-layer-check's options.
outputs_at() {
    local scale="$1" tag="$2"; shift 2
    fact "outputs at scale $scale ($tag): two outputs, a third added, the second removed, another added"
    nested start --sway-only --outputs 2 --scale "$scale" >/dev/null
    nested exec -- "$BETTER_WL_PROBE" outputs
    run "outputs-$tag-window.log" --toplevel
    run "outputs-$tag-strip.log" --every-output --popup "$@"
    nested click 300 16; sleep 1.5
    nested keys ab; sleep 0.5
    nested shot "$OUT/10-outputs-$tag-HEADLESS-1.png" HEADLESS-1
    nested shot "$OUT/10-outputs-$tag-HEADLESS-2.png" HEADLESS-2
    echo "-- add"; nested output add; sleep 3
    nested exec -- "$BETTER_WL_PROBE" outputs
    nested shot "$OUT/11-outputs-$tag-HEADLESS-3-added.png" HEADLESS-3
    echo "-- remove HEADLESS-2"; nested output remove HEADLESS-2; sleep 3
    echo "-- add"; nested output add; sleep 3
    nested exec -- "$BETTER_WL_PROBE" outputs
    nested shot "$OUT/12-outputs-$tag-HEADLESS-4-added.png" HEADLESS-4
    echo "-- strip:"; events "outputs-$tag-strip.log"
    grep -h -m3 -i 'protocol error\|error 0' "$OUT/logs/outputs-$tag-strip.log" || true
    echo "-- window:"; events "outputs-$tag-window.log"
    end_programs; nested stop >/dev/null
}

case_outputs() {
    outputs_at 1 scale1-strip
    outputs_at 2 scale2-strip
    outputs_at 2 scale2-fixed --anchor top,left --width 600 --height 40 --zone none
}

cases=("$@")
((${#cases[@]})) || cases=(layers anchors zone zone-in-place anchor-change keyboard popup input-region scale outputs)
for name in "${cases[@]}"; do
    "case_${name//-/_}"
done
