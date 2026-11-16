# Ticket 137 — Better Files finds the computers and shares on the local network

**Blocked by:** none

## Why

The owner asked on 2026-11-16 for Better Files to show what is on the
network, as Windows' Network and GNOME Files' Other Locations do. Today
servers open only when their address is typed into the path bar
(`sftp://`, `smb://`, `ftp://`, `dav(s)://`, `afp://`, `nfs://`, ticket 117),
and the tailnet machines are listed from Tailscale.

## What to build

- A Network place in the sidebar listing computers and services found on the
  local network, read from what the desktop already discovers rather than by
  scanning: GVfs's `network:///` (DNS-SD/mDNS through Avahi, and SMB browsing
  through `wsdd`/NetBIOS where the system has it), deduplicated by host, each
  with what it offers (SMB shares, SFTP, WebDAV, printers left out).
- Opening a computer lists its SMB shares (as `smb://host/` does), each with
  its own capacity once mounted; sign-in goes through the existing in-window
  prompt.
- A "Connect to server" entry (and the path bar) for addresses discovery does
  not find, with recent servers remembered (part of the person's data,
  ticket 85's allowlist).
- Say plainly when discovery has nothing to work with (Avahi not running, no
  SMB browsing service), never an empty list that reads as "no computers".
- Coordinate with ticket 133 (a tailnet machine's SMB shares) so one machine
  is one entry, whether it was found on the LAN or on the tailnet.

## Done when

- Tests against a fake GVfs `network:///` listing; seen against a real SMB
  server on the owner's network with consent.
