/*
 * The Better OS GNOME Shell adapter: one extension shared by every Better OS
 * component that needs something only GNOME Shell's own process can see or do.
 *
 * ── The exception this file exists under ────────────────────────────────────
 *
 * Better OS is a Rust-only project. ADR 0012 granted one bounded exception for
 * GJS, and ADR 0018 extends it to this directory with the same bounds:
 *
 *   - it relays typed events out of the shell and performs typed actions the
 *     shell already performs, and nothing else;
 *   - it stores nothing and decides nothing: there is no setting, no
 *     per-application rule, no list of applications, and no choice here. Each
 *     decision is made in Rust, by the service that owns the relay's
 *     counterpart bus name;
 *   - it executes nothing, and no method takes free text;
 *   - each relay is active only while its counterpart owns that bus name, and
 *     answers only that owner. With no counterpart, the relay does nothing and
 *     GNOME's own behaviour stays exactly as GNOME set it up.
 *
 * The one file it reads is the interface XML beside it, once, at enable. The
 * one other thing it reads is where the counterpart's process runs from
 * (/proc/<pid>/exe), to accept the name's owner only when it is the packaged
 * service.
 *
 * If this file grows a decision, the exception has been abused. Move the
 * decision into Rust rather than widening the exception.
 *
 * ── Relays ──────────────────────────────────────────────────────────────────
 *
 * Window attention (ticket 106). Counterpart: org.betteros.WindowAttention1,
 * `better-window-attention`. While it asks, GNOME's WindowAttentionHandler is
 * taken off the display's two attention signals and each request is reported
 * instead; the service answers with one of the window methods below, and is
 * told when a reported window's request ends.
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Config from 'resource:///org/gnome/shell/misc/config.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const BUS_NAME = 'org.betteros.ShellAdapter1';
const ATTENTION_OBJECT_PATH = '/org/betteros/ShellAdapter1/WindowAttention';
const ATTENTION_INTERFACE_FILE = 'org.betteros.ShellAdapter1.WindowAttention.xml';
/** The window attention relay's counterpart: the Rust service that decides. */
const ATTENTION_COUNTERPART = 'org.betteros.WindowAttention1';
const ATTENTION_INTERFACE = 'org.betteros.ShellAdapter1.WindowAttention';
/**
 * The program the counterpart must be. Owning the bus name is not enough: any
 * program in the session could take it first.
 */
const COUNTERPART_EXECUTABLE = '/usr/libexec/better-window-attention';
const ACCESS_DENIED = 'org.freedesktop.DBus.Error.AccessDenied';

/** The wire values for the two display signals. The Rust side has the same. */
const KIND_DEMANDS_ATTENTION = 0;
const KIND_MARKED_URGENT = 1;

/** Why a reported window's request ended. The Rust side has the same. */
const ENDED_FOCUSED = 0;
const ENDED_CLOSED = 1;
const ENDED_WITHDRAWN = 2;

/** Bumped when the signal or method shapes change in a way a client can see. */
const PROTOCOL_VERSION = 1;

export default class ShellAdapterExtension extends Extension {
    enable() {
        this._counterpart = null;
        this._suppressed = false;

        const xml = this._interfaceXml(ATTENTION_INTERFACE_FILE);
        this._exported = Gio.DBusExportedObject.wrapJSObject(xml, this);
        this._exported.export(Gio.DBus.session, ATTENTION_OBJECT_PATH);
        this._nameId = Gio.bus_own_name(
            Gio.BusType.SESSION,
            BUS_NAME,
            Gio.BusNameOwnerFlags.NONE,
            null,
            null,
            null);
        this._watchId = Gio.bus_watch_name(
            Gio.BusType.SESSION,
            ATTENTION_COUNTERPART,
            Gio.BusNameWatcherFlags.NONE,
            (connection, _name, owner) => {
                this._counterpart = null;
                this._acceptIfPackaged(connection, owner);
            },
            () => {
                // The service is gone, whatever the reason: GNOME gets its own
                // handler back at once.
                this._counterpart = null;
                this._restoreBuiltInAttention();
            });
    }

    disable() {
        if (this._watchId) {
            Gio.bus_unwatch_name(this._watchId);
            this._watchId = null;
        }
        // A disable, an uninstall, and a shell restart all arrive here, and
        // all of them leave GNOME's own handler connected.
        this._restoreBuiltInAttention();
        this._counterpart = null;
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
    _interfaceXml(name) {
        const file = this.dir.get_child(name);
        const [, contents] = file.load_contents(null);
        return new TextDecoder().decode(contents);
    }

    /**
     * Accepts the name's owner as the counterpart only when the bus says its
     * process runs the packaged service. Anything else that took the name is
     * never answered. A unique name is never reused, so an answer that
     * arrives after the owner left names nobody who can call.
     */
    _acceptIfPackaged(connection, owner) {
        connection.call(
            'org.freedesktop.DBus',
            '/org/freedesktop/DBus',
            'org.freedesktop.DBus',
            'GetConnectionCredentials',
            new GLib.Variant('(s)', [owner]),
            new GLib.VariantType('(a{sv})'),
            Gio.DBusCallFlags.NONE,
            -1,
            null,
            (source, result) => {
                try {
                    const [credentials] = source.call_finish(result).deepUnpack();
                    const pid = credentials.ProcessID.deepUnpack();
                    if (GLib.file_read_link(`/proc/${pid}/exe`) === COUNTERPART_EXECUTABLE)
                        this._counterpart = owner;
                } catch {
                    // No credentials, or no such process: not the service.
                }
            });
    }

    /** Sends a signal to the counterpart alone; nobody else hears it. */
    _tellCounterpart(member, parameters) {
        if (this._counterpart === null)
            return;
        Gio.DBus.session.emit_signal(
            this._counterpart, ATTENTION_OBJECT_PATH, ATTENTION_INTERFACE, member, parameters);
    }

    /**
     * Whether a call came from the counterpart's current owner. Anyone else is
     * refused, so no other program on the session bus can drive the relay.
     */
    _fromCounterpart(invocation) {
        if (this._counterpart !== null && invocation.get_sender() === this._counterpart)
            return true;
        invocation.return_dbus_error(ACCESS_DENIED, `only ${ATTENTION_COUNTERPART} may call this`);
        return false;
    }

    // ── Window attention ────────────────────────────────────────────────────

    SuppressBuiltInAttentionAsync([suppress], invocation) {
        if (!this._fromCounterpart(invocation))
            return;
        if (!suppress)
            this._restoreBuiltInAttention();
        else if (!this._suppressBuiltInAttention())
            return invocation.return_dbus_error(ACCESS_DENIED, 'GNOME Shell has no window attention handler where this build expects it');
        invocation.return_value(null);
    }

    /**
     * Takes GNOME's handler off the two display signals and reports each
     * request instead. The handler is the shell's own object; it is
     * disconnected by its own owner tag and nothing inside it is changed.
     */
    _suppressBuiltInAttention() {
        const handler = Main.windowAttentionHandler;
        if (this._suppressed)
            return true;
        if (typeof handler?._onWindowDemandsAttention !== 'function')
            return false;
        global.display.disconnectObject(handler);
        global.display.connectObject(
            'window-demands-attention',
            (_display, window) => this._reportAttention(window, KIND_DEMANDS_ATTENTION),
            'window-marked-urgent',
            (_display, window) => this._reportAttention(window, KIND_MARKED_URGENT),
            this);
        this._suppressed = true;
        return true;
    }

    /** Puts GNOME's handler back exactly as its constructor connects it. */
    _restoreBuiltInAttention() {
        if (!this._suppressed)
            return;
        global.display.disconnectObject(this);
        for (const actor of global.get_window_actors())
            actor.meta_window.disconnectObject(this);
        const handler = Main.windowAttentionHandler;
        global.display.connectObject(
            'window-demands-attention', handler._onWindowDemandsAttention.bind(handler),
            'window-marked-urgent', handler._onWindowDemandsAttention.bind(handler),
            handler);
        this._suppressed = false;
    }

    /**
     * Reports what the shell saw: the window, its application, and its state.
     * The window's own focus, close, and withdrawal are reported once each,
     * as `WindowAttentionEnded`, so the service can take back what it marked.
     */
    _reportAttention(window, kind) {
        if (!window)
            return;
        const app = Shell.WindowTracker.get_default().get_window_app(window);
        window.disconnectObject(this);
        window.connectObject(
            'focus', () => this._reportEnded(window, ENDED_FOCUSED),
            'unmanaged', () => this._reportEnded(window, ENDED_CLOSED),
            'notify::demands-attention', () => this._reportIfWithdrawn(window),
            'notify::urgent', () => this._reportIfWithdrawn(window),
            this);
        this._tellCounterpart('WindowAttention', new GLib.Variant('(tssubb)', [
            window.get_id(),
            app?.get_id() ?? '',
            app?.get_name() ?? '',
            kind,
            window.has_focus(),
            window.is_skip_taskbar(),
        ]));
    }

    _reportIfWithdrawn(window) {
        if (!window.demands_attention && !window.urgent)
            this._reportEnded(window, ENDED_WITHDRAWN);
    }

    _reportEnded(window, reason) {
        window.disconnectObject(this);
        this._tellCounterpart('WindowAttentionEnded', new GLib.Variant('(tu)', [
            window.get_id(),
            reason,
        ]));
    }

    /** The window the shell knows by this id, or null. */
    _windowById(id) {
        for (const actor of global.get_window_actors()) {
            if (actor.meta_window.get_id() === id)
                return actor.meta_window;
        }
        return null;
    }

    ShowAttentionBannerAsync([id], invocation) {
        if (!this._fromCounterpart(invocation))
            return;
        const window = this._windowById(id);
        if (window)
            Main.windowAttentionHandler._onWindowDemandsAttention(global.display, window);
        invocation.return_value(null);
    }

    ActivateWindowAsync([id], invocation) {
        if (!this._fromCounterpart(invocation))
            return;
        const window = this._windowById(id);
        if (window)
            Main.activateWindow(window);
        invocation.return_value(null);
    }

    ClearAttentionAsync([id], invocation) {
        if (!this._fromCounterpart(invocation))
            return;
        const window = this._windowById(id);
        if (window)
            window.unset_demands_attention();
        invocation.return_value(null);
    }

    /** What the relay can do and whether it is on. Reads and changes nothing. */
    Capabilities() {
        return JSON.stringify({
            protocol_version: PROTOCOL_VERSION,
            shell_version: Config.PACKAGE_VERSION ?? 'unknown',
            relays: ['window-attention'],
            window_attention: {
                counterpart_present: this._counterpart !== null,
                built_in_suppressed: this._suppressed,
                // Whether GNOME's handler is where this build expects it. A
                // shell that moved it reports false rather than a takeover
                // that did nothing.
                built_in_handler_found:
                    typeof Main.windowAttentionHandler?._onWindowDemandsAttention === 'function',
            },
        });
    }
}
