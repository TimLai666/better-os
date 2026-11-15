# ADR 0032: Which drives mount automatically is kept in fstab and a udev rule

## Status

Accepted on 2026-11-15 (ticket 127). The owner set the defaults the same day:
removable drives and the system drive mount automatically; other internal
partitions (a Windows partition, say) follow what the system does today until
the person turns them on. ADR 0007's protocol is unchanged; this adds one
interface beside `org.betteros.Hardware1` on the daemon's object.

## Context

Ticket 127 gives every drive Better Files lists a "Mount automatically"
choice, shared by Better Files and Better Manager's System page, and stops
listing a drive that is not mounted. Two kinds of drive need two different
things:

- An **internal partition** is never mounted at login by GNOME. Something
  persistent has to say "mount this at boot", and it has to be written as
  root.
- A **removable drive** is already mounted on arrival by GNOME Shell through
  UDisks2, as the active session may. Turning it *off* needs something that
  stops that, per drive, also as root, since the hint GNOME reads comes from
  udev.

Three places could hold the internal choice:

1. **`/etc/fstab`.** systemd's fstab generator turns each line into a mount
   unit at boot; UDisks2 and GVfs read the same file, so `x-gvfs-show` shows
   the drive under the label GNOME would use, and a Retry is a
   `systemctl start` of the unit the generator made. `findmnt --verify` checks
   the file. The risk is that the file belongs to the administrator: a writer
   that rewrites it can break the machine's boot.
2. **A UDisks2 configuration file** (`/etc/udisks2/<id>.conf`,
   `mount_options.conf`). It sets the options UDisks2 uses *when asked* to
   mount; nothing in it mounts a drive at boot or at login without a client
   asking, so on its own it cannot keep the promise.
3. **A systemd mount unit** in `/etc/systemd/system`. It mounts at boot, but
   UDisks2 and GVfs do not read units: the drive is shown as an unknown mount,
   not under its label, and UDisks2 treats it as mounted by someone else.

## Decision

- **Internal partitions: a block of lines in `/etc/fstab` that Better OS owns.**
  The block is between `# >>> Better OS drive mounting` and
  `# <<< Better OS drive mounting` comment lines. Each line in it is exactly
  `UUID=<uuid> /mnt/better-os/<uuid> <type> nofail,x-gvfs-show,x-systemd.device-timeout=10s[,uid=…,gid=…] 0 0`.
  `nofail` keeps a missing or broken drive from stopping boot. `uid`/`gid` are
  added only for filesystems without owners (FAT, exFAT, NTFS), set to the
  person who asked, so they can write to it. `x-udisks-auth` is not used: it
  only changes whether UDisks2 asks before a client-side mount, and the mount
  here is made by systemd.
- **The writer touches nothing outside its block.** It refuses a file that is
  not UTF-8, holds a NUL, has a line outside the block with more than six
  fields, has a begin without an end (or two blocks), or has anything in the
  block that is not one of its own lines. It refuses to add a drive another
  line already names (by `UUID=`, `PARTUUID=`, `LABEL=` or device path):
  that drive is the administrator's, and the window says so and locks the
  choice. It writes a new file beside `/etc/fstab` with the old file's mode
  and owner, `fsync`s it, checks the old file has not changed since it was
  read, renames it over, and `fsync`s `/etc`. Removing the last entry
  removes the block, so the file goes back to what it was.
- **Removable drives: one rules file Better OS owns**,
  `/etc/udev/rules.d/90-better-os-drives.rules`, holding
  `ENV{ID_FS_UUID}=="<uuid>", ENV{UDISKS_AUTO}="0"` for each drive turned
  off. `UDISKS_AUTO` is UDisks2's documented hint that presentation software
  should not mount the device automatically; GVfs passes it to GNOME Shell's
  automounter. It takes effect at the drive's next arrival; a drive that is
  mounted when it is turned off is unmounted by the window through UDisks2,
  as the person may. Turning one on removes its line, and the window mounts
  it now through UDisks2. No file is left when no line is.
- **One method, `org.betteros.Drives1.SetMountAutomatically(uuid, on)`,**
  behind its own polkit action `org.betteros.manager.drive-mounting`
  (`auth_admin_keep`: asked once, then kept for a few minutes). The UUID must
  be 1 to 64 characters of hex digits and dashes; nothing else crosses. The
  daemon then asks UDisks2 for the device with that UUID itself, and refuses
  one that is not a filesystem a person keeps files on, or is the system's.
  Whether the drive is internal or removable is UDisks2's answer, not the
  caller's. For an internal one turned on it writes the entry,
  `systemctl daemon-reload`s, and starts the mount unit unless the drive is
  already mounted; turned off it stops the unit (a drive in use is refused
  with the reason and the entry stays), removes the entry, reloads, and
  removes the empty mount point.
- **Removing `better-manager-daemon`** (its prerm, on `remove` only) runs
  `better-manager-daemon --remove-drive-entries`: it stops the units of its
  entries, removes the block and the rules file, reloads systemd, and removes
  the empty mount points. What the administrator wrote is untouched.

## Consequences

- The choice has one home, the system's own files, which both windows read
  unprivileged (`/etc/fstab` and the rules file are world-readable). There is
  no preference file to get out of step with what the system does.
- A drive that failed to mount at boot is a failed mount unit; the windows
  read its result from systemd over the system bus and offer Retry, which is
  the same method called again.
- A removable drive turned off while plugged in stays off only from its next
  arrival as far as udev is concerned; until then the window has unmounted it.
- The container end-to-end check writes and removes an entry through the
  daemon and runs `findmnt --verify` on the result.

## Not chosen

- **A UDisks2 configuration file:** it cannot mount anything by itself.
- **A systemd mount unit:** UDisks2 and GVfs would not show the drive as the
  drive it is.
- **Editing fstab lines in place, or rewriting the whole file from a model:**
  either would touch lines the administrator wrote.
- **The window mounting internal drives at login through UDisks2:** it needs
  the window or a service running and an administrator's password at each
  login for a drive the system does not own.
