# Better Monitor visibility throttling

Better Monitor keeps collecting at the configured 250 ms to 3 s interval even
when its window is not active. Collection updates the bounded histories,
incident evidence, device maxima, and in-memory process/application models.

Presentation work is separate. While the window is active, tables and charts
refresh at the configured collection interval. While inactive or minimized,
Better Monitor coalesces table/chart refresh and window notifications to at
most once every five seconds. This avoids spending rendering work on a window
the user is not looking at without creating gaps in the collected evidence.

The next scheduled sample restores full-rate presentation after the window
becomes active. The policy is covered by unit tests and does not change the
manual chart-pause behavior.
