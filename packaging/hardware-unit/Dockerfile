# A disposable Ubuntu 24.04 with systemd 255 as PID 1 and journald, for the
# hardware helpers' sandbox (ADR 0029, narrowed by ADR 0034): the daemon and
# the restore cannot write /sys, the transient helper writes its one file and
# nothing else, and the LampArray helper's unit starts and fails cleanly with
# no hidraw device, and the USB-C recovery's helper units (ADR 0046) start
# with their own lists. CI runs it on every push. Everything apt needs is
# installed here, so the check itself runs with no network.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends systemd systemd-sysv util-linux dbus \
    && rm -rf /var/lib/apt/lists/*
STOPSIGNAL SIGRTMIN+3
CMD ["/usr/sbin/init"]
