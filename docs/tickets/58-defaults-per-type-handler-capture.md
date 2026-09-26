# Ticket 58 — Better Defaults can capture and restore a mixed handler group

**Epic:** Better Defaults (Issue #10) · **User Story:** a person whose image
types open in two different viewers can still apply Better Files' handler
group and later get both viewers back · **Branch:** `ticket-57-58` ·
**Blocked by:** none · **Status:** ready

## What it delivers

A handler group's previous value is stored as one owner. When the group's
types currently point at different applications, the value reads as unknown
and apply is refused. After this ticket the snapshot stores the previous owner
per declared type, apply is allowed after the preview lists each type's
current owner, and restore writes each type back to its own previous owner.

## Acceptance criteria

- [ ] The snapshot stores per-type previous values. Snapshots written before
      this change still load and restore as before.
- [ ] The preview in both the CLI and the GUI lists the per-type owners when
      they differ.
- [ ] Restore writes each type to its own captured owner and reports per type
      what the verifying read saw.
- [ ] A type that had no owner is reported as Manual action required on
      restore, as today, until a remove-association operation exists.
