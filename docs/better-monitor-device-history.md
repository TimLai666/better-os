# Better Monitor device history

Better Monitor keeps a bounded in-memory history for every detected drive, network interface, and battery.

- Drives retain read and write throughput plus session maxima.
- Network interfaces retain receive and transmit throughput plus session maxima.
- Batteries retain charge percentage and reported power plus session maxima.
- The history length follows the configured graph point limit.
- Missing driver data remains unavailable and is never converted into a synthetic zero.
- NetworkManager connection names are read through `nmcli` when available; direct D-Bus integration and real-session SSID validation remain follow-up work.

This history belongs to the running GUI process. Persistent background history remains outside the current PR boundary.
