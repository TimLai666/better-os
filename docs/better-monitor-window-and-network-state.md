# Better Monitor window and network state

## Window state

Better Monitor stores its logical window width, height, and maximized state in
`monitor.conf`. The saved size is clamped before it is used. On Wayland the
compositor remains responsible for placement, so Better Monitor does not store
or claim control over the global window position.

The state is saved from GPUI's close callback using the platform-reported
`WindowBounds` and `is_maximized()` value, then restored through
`WindowOptions.window_bounds` during the next launch.

## NetworkManager metadata

Network names use the system bus first:

1. Resolve the device through `GetDeviceByIpIface`.
2. For Wi-Fi devices, read `ActiveAccessPoint` and its byte-array `Ssid`.
3. Otherwise read the active connection `Id`.
4. Fall back to `nmcli` only when the typed D-Bus path is unavailable.

Results are cached briefly so a fast monitor refresh does not repeatedly query
NetworkManager or spawn helper processes. Unknown values remain unavailable;
Better Monitor does not invent an SSID.
