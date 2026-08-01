# A disposable Ubuntu 24.04 environment for the privileged-service end-to-end
# check. AGENTS.md forbids installing an unreleased build on the host, so this
# is where a real dpkg, a real system bus, and a real polkitd can be used
# without touching anything that matters.
# The base must match the release the packages were built for, or the check
# would be testing a libc pairing nobody ships.
ARG UBUNTU_RELEASE=24.04
FROM ubuntu:${UBUNTU_RELEASE}

ENV DEBIAN_FRONTEND=noninteractive

# dbus and polkitd are what the service is authorized through, so they are the
# point of the exercise rather than incidental dependencies.
#
# The desktop libraries are here because the sandbox this image runs in has no
# network: anything apt would need to fetch at test time has to be present
# already. The privileged service deliberately needs none of them — it installs
# into an image that had none until this line — but better-monitor does, and it
# is the component the test installs and removes.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        apt \
        dbus \
        dpkg \
        policykit-1 \
        libfontconfig1 \
        libwayland-client0 \
        libwayland-cursor0 \
        libwayland-egl1 \
        libxcb1 \
        libxkbcommon0 \
        libxkbcommon-x11-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/better-os
COPY dist/e2e/ /opt/better-os/dist/
COPY packaging/test-daemon-e2e.sh /opt/better-os/packaging/test-daemon-e2e.sh
RUN chmod +x /opt/better-os/packaging/test-daemon-e2e.sh

ENV BETTER_OS_E2E_CONTAINER=1

# Repeated after FROM: an ARG before it is not in scope afterwards.
ARG UBUNTU_RELEASE=24.04
ENV RELEASE_TARGET=ubuntu-${UBUNTU_RELEASE}

CMD ["/bin/sh", "-c", "/opt/better-os/packaging/test-daemon-e2e.sh /opt/better-os/dist \"$RELEASE_TARGET\""]
