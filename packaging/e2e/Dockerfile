# A disposable Ubuntu 24.04 environment for the privileged-service end-to-end
# check. AGENTS.md forbids installing an unreleased build on the host, so this
# is where a real dpkg, a real system bus, and a real polkitd can be used
# without touching anything that matters.
# The base must match the release the packages were built for, or the check
# would be testing a libc pairing nobody ships.
ARG UBUNTU_RELEASE=24.04
FROM ubuntu:${UBUNTU_RELEASE}

ENV DEBIAN_FRONTEND=noninteractive
ARG UBUNTU_RELEASE=24.04

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
# The bootstrap installer, exercised here for the half of it that changes a
# machine. It runs as root in this image, so it takes its no-sudo path; the
# sudo path is the one difference between what runs here and what a user runs.
COPY install.sh /opt/better-os/install.sh
# The test-only polkit authorization and the client that drives the authorized
# path. Neither is in any package: the rule would be a hole in a shipped
# system, and the client is an example built for this check.
COPY packaging/e2e/50-better-os-e2e.rules packaging/e2e/50-better-os-e2e.pkla /opt/better-os/e2e/
COPY dist/e2e-client/e2e_client /opt/better-os/e2e_client

# These fixtures are deliberately built in the disposable container rather
# than copied from dist/. Both ship the same two files, and the new package's
# postinst deletes one of them after dpkg has unpacked and recorded it. That is
# the state the health check exists to catch: dpkg says the package installed
# and lists a file that is not on disk, so a real update can apply successfully
# and then fail its health check.
#
# Neither fixture is named after its own binaries, which is the shape
# better-awake ships and the shape the previous health rule failed on. The old
# package passing its check here is that rule's replacement being exercised
# against real dpkg state rather than only against a fake.
RUN set -eu; \
    fixture_dir=/opt/better-os/e2e-fixtures; \
    architecture="$(dpkg --print-architecture)"; \
    for version in 0.0.9 0.1.0; do \
        root="$fixture_dir/$version"; \
        mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/better-os"; \
        printf 'Package: better-rollback-fixture\nVersion: %s\nArchitecture: %s\nDescription: Better OS rollback fixture\n' \
            "$version" "$architecture" > "$root/DEBIAN/control"; \
        printf '#!/bin/sh\nexit 0\n' > "$root/usr/bin/rollback-fixture-service"; \
        chmod 0755 "$root/usr/bin/rollback-fixture-service"; \
        printf 'fixture %s\n' "$version" > "$root/usr/share/better-os/rollback-fixture.data"; \
    done; \
    printf '#!/bin/sh\nset -e\nrm -f /usr/bin/rollback-fixture-service\nexit 0\n' \
        > "$fixture_dir/0.1.0/DEBIAN/postinst"; \
    chmod 0755 "$fixture_dir/0.1.0/DEBIAN/postinst"; \
    dpkg-deb --build --root-owner-group "$fixture_dir/0.0.9" \
        "$fixture_dir/better-rollback-fixture_0.0.9_ubuntu-${UBUNTU_RELEASE}_${architecture}.deb"; \
    dpkg-deb --build --root-owner-group "$fixture_dir/0.1.0" \
        "$fixture_dir/better-rollback-fixture_0.1.0_ubuntu-${UBUNTU_RELEASE}_${architecture}.deb"; \
    rm -rf "$fixture_dir/0.0.9" "$fixture_dir/0.1.0"

RUN chmod +x /opt/better-os/packaging/test-daemon-e2e.sh /opt/better-os/e2e_client /opt/better-os/install.sh

ENV BETTER_OS_E2E_CONTAINER=1

ENV RELEASE_TARGET=ubuntu-${UBUNTU_RELEASE}

CMD ["/bin/sh", "-c", "/opt/better-os/packaging/test-daemon-e2e.sh /opt/better-os/dist \"$RELEASE_TARGET\""]
