# A disposable Ubuntu 24.04 with systemd 255 as PID 1, for one question
# (ADR 0017, ticket 86): does a ReadWritePaths= entry under /sys win over
# ProtectKernelTunables=yes, so the daemon can write its two attributes while
# the rest of /sys stays read-only? Everything apt needs is installed here, so
# the check itself runs with no network.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends systemd systemd-sysv util-linux \
    && rm -rf /var/lib/apt/lists/*
STOPSIGNAL SIGRTMIN+3
CMD ["/usr/sbin/init"]
