/*
 * Better Touchpad's GNOME Shell adapter.
 *
 * ── The exception this file exists under ────────────────────────────────────
 *
 * Better OS is a Rust-only project. ADR 0012 asked for one bounded exception to
 * that rule, and the project owner granted it on 2026-08-31. This file is the
 * whole of the exception, and the bounds are:
 *
 *   - it bridges typed events and typed actions, and nothing else;
 *   - it contains no threshold, no cooldown, no cancellation rule, no
 *     configuration, and no gesture decision of any kind. Every one of those
 *     lives in `crates/touchpad-gestures`, tested by replay;
 *   - it performs no action GNOME Shell does not already expose, and it
 *     executes nothing: there is no spawn, no shell string, and no free-text
 *     argument on any method here;
 *   - it decides nothing about which gesture means what. It reports what the
 *     compositor saw and does what it is told.
 *
 * If this file grows a decision, the exception has been abused. The right
 * answer is to move the decision back into Rust, not to widen the exception.
 *
 * ── What it cannot do ───────────────────────────────────────────────────────
 *
 * Clutter's touchpad gesture events carry a contact count and nothing about
 * which contact is which, so a thumb is invisible here. `Capabilities` says so,
 * and the Rust side treats thumb-and-three as its four-contact primitive rather
 * than pretending to a detection this stream cannot make.
 */

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Config from 'resource:///org/gnome/shell/misc/config.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const BUS_NAME = 'org.betteros.TouchpadAdapter1';
const OBJECT_PATH = '/org/betteros/TouchpadAdapter1';
const INTERFACE_FILE = 'org.betteros.TouchpadAdapter1.xml';

/** The wire values for a gesture phase. The Rust side has the same four. */
const PHASE_BEGIN = 0;
const PHASE_UPDATE = 1;
const PHASE_END = 2;
const PHASE_CANCEL = 3;

/** Bumped when the signal or method shapes change in a way a client can see. */
const PROTOCOL_VERSION = 1;

function wirePhase(phase) {
    switch (phase) {
    case Clutter.TouchpadGesturePhase.BEGIN:
        return PHASE_BEGIN;
    case Clutter.TouchpadGesturePhase.UPDATE:
        return PHASE_UPDATE;
    case Clutter.TouchpadGesturePhase.END:
        return PHASE_END;
    case Clutter.TouchpadGesturePhase.CANCEL:
        return PHASE_CANCEL;
    default:
        return null;
    }
}

export default class TouchpadAdapterExtension extends Extension {
    enable() {
        this._suppressed = false;
        this._trackerState = [];

        const xml = this._interfaceXml();
        this._exported = Gio.DBusExportedObject.wrapJSObject(xml, this);
        this._exported.export(Gio.DBus.session, OBJECT_PATH);
        this._nameId = Gio.bus_own_name(
            Gio.BusType.SESSION,
            BUS_NAME,
            Gio.BusNameOwnerFlags.NONE,
            null,
            null,
            null);

        this._capturedId = global.stage.connect(
            'captured-event', (_actor, event) => this._onEvent(event));
    }

    disable() {
        if (this._capturedId) {
            global.stage.disconnect(this._capturedId);
            this._capturedId = null;
        }
        // Whatever was asked for, the desktop gets its own gestures back when
        // this extension goes. A disable, an uninstall, and a shell restart all
        // arrive here.
        this._restoreBuiltInGestures();
        if (this._nameId) {
            Gio.bus_unown_name(this._nameId);
            this._nameId = null;
        }
        if (this._exported) {
            this._exported.unexport();
            this._exported = null;
        }
    }

    /** The contract, read from the file that ships beside this one. */
    _interfaceXml() {
        const file = this.dir.get_child(INTERFACE_FILE);
        const [, contents] = file.load_contents(null);
        return new TextDecoder().decode(contents);
    }

    _onEvent(event) {
        const type = event.type();
        if (type === Clutter.EventType.TOUCHPAD_SWIPE)
            this._emitSwipe(event);
        else if (type === Clutter.EventType.TOUCHPAD_PINCH)
            this._emitPinch(event);
        // Observing only. Suppressing a built-in gesture is done by disabling
        // the shell's own tracker, never by swallowing an event here, so this
        // handler never changes what anything else on the stage receives.
        return Clutter.EVENT_PROPAGATE;
    }

    _emitSwipe(event) {
        const phase = wirePhase(event.get_gesture_phase());
        if (phase === null)
            return;
        const [dx, dy] = event.get_gesture_motion_delta();
        this._exported.emit_signal('SwipeGesture', new GLib.Variant('(uuddt)', [
            phase,
            event.get_touchpad_gesture_finger_count(),
            dx,
            dy,
            event.get_time(),
        ]));
    }

    _emitPinch(event) {
        const phase = wirePhase(event.get_gesture_phase());
        if (phase === null)
            return;
        this._exported.emit_signal('PinchGesture', new GLib.Variant('(uuddt)', [
            phase,
            event.get_touchpad_gesture_finger_count(),
            event.get_gesture_pinch_scale(),
            event.get_gesture_pinch_angle_delta(),
            event.get_time(),
        ]));
    }

    // ── The shell-owned actions ─────────────────────────────────────────────

    ShowOverview() {
        Main.overview.show();
    }

    ShowDesktop() {
        // GNOME 46 has no show-desktop action of its own, so this is what the
        // phrase means: every window on the active workspace minimised. The
        // window list comes from the shell; nothing is executed.
        const workspace = global.workspace_manager.get_active_workspace();
        for (const actor of global.get_window_actors()) {
            const window = actor.meta_window;
            if (window.is_skip_taskbar() || window.get_workspace() !== workspace)
                continue;
            window.minimize();
        }
    }

    SwitchWorkspace(direction) {
        const manager = global.workspace_manager;
        const motion = direction < 0
            ? Meta.MotionDirection.LEFT
            : Meta.MotionDirection.RIGHT;
        const target = manager.get_active_workspace().get_neighbor(motion);
        if (target)
            target.activate(global.get_current_time());
    }

    /**
     * Turns GNOME's own three- and four-finger swipe trackers off, or puts them
     * back exactly as they were. The trackers are objects the shell owns; this
     * flips the `enabled` property each one already has rather than reaching
     * into how they work.
     */
    SuppressBuiltInGestures(suppress) {
        if (suppress)
            this._suppressBuiltInGestures();
        else
            this._restoreBuiltInGestures();
    }

    _swipeTrackers() {
        const trackers = [];
        // Overview and the application grid.
        if (Main.overview?._swipeTracker)
            trackers.push(Main.overview._swipeTracker);
        // Switching workspaces.
        if (Main.wm?._workspaceAnimation?._swipeTracker)
            trackers.push(Main.wm._workspaceAnimation._swipeTracker);
        return trackers;
    }

    _suppressBuiltInGestures() {
        if (this._suppressed)
            return;
        this._trackerState = this._swipeTrackers().map(tracker => {
            const was = tracker.enabled;
            tracker.enabled = false;
            return [tracker, was];
        });
        this._suppressed = true;
    }

    _restoreBuiltInGestures() {
        for (const [tracker, was] of this._trackerState)
            tracker.enabled = was;
        this._trackerState = [];
        this._suppressed = false;
    }

    /**
     * What this event stream can and cannot tell apart. Reported rather than
     * assumed, because a capability nobody states is a capability somebody
     * guesses at.
     */
    Capabilities() {
        return JSON.stringify({
            protocol_version: PROTOCOL_VERSION,
            shell_version: Config.PACKAGE_VERSION ?? 'unknown',
            // Clutter reports how many contacts a gesture has.
            finger_count: true,
            // And nothing at all about which of them is a thumb.
            thumb_detection: false,
            // Swipe and pinch both arrive as begin/update/end/cancel with
            // cumulative deltas, so progress is continuous.
            continuous_progress: true,
            gesture_kinds: ['swipe', 'pinch'],
            actions: ['overview', 'show-desktop', 'switch-workspace'],
            // Stated because the Better OS action catalog has a row for it and
            // GNOME 46 has no facility that answers it: the window picker is
            // the overview itself and cannot be filtered to one application.
            unsupported_actions: ['current-application-windows'],
            // How many of GNOME's own swipe trackers this build can reach. A
            // shell that renamed or removed them reports zero here, which is
            // the difference between "suppression did nothing" and "suppression
            // was never possible" — and the only way a client can tell.
            built_in_trackers: this._swipeTrackers().length,
            built_in_gestures_suppressed: this._suppressed,
        });
    }
}
