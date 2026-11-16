# A disposable Ubuntu 24.04 with systemd 255 as PID 1 and logind, for the
# login screen watch's end-to-end check (ADR 0033, ticket 140). Everything
# apt needs is installed here, so the check itself runs with no network.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends systemd systemd-sysv dbus libpam-systemd \
    && rm -rf /var/lib/apt/lists/*
STOPSIGNAL SIGRTMIN+3
CMD ["/usr/sbin/init"]
