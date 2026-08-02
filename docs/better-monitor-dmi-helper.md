# Better Monitor DMI helper

Better Monitor remains an unprivileged GPUI application. Detailed memory
module information uses the already-packaged root daemon through a separate,
read-only D-Bus surface:

- bus name: `org.betteros.Monitor1`
- object path: `/org/betteros/Monitor1`
- interface: `org.betteros.Monitor1`
- Polkit action: `org.betteros.monitor.read-memory-devices`

The GUI asks only when the user selects **Show memory hardware details**.
Cancellation and denial leave live memory usage available.

The daemon reads `/sys/firmware/dmi/tables/smbios_entry_point` and
`/sys/firmware/dmi/tables/DMI`, decodes SMBIOS Type 17, validates the bounded
`monitor-ipc` document, and returns module size, speed, type, form factor,
locator, manufacturer, part number, and configured voltage where firmware
provides them.

The wire contract deliberately excludes serial numbers, asset tags, raw
firmware bytes, and filesystem paths. It does not invoke `dmidecode` as a
subprocess and the GUI never parses privileged command output.
