from pathlib import Path

path = Path("/tmp/apply_monitor_dmi.py")
text = path.read_text()

old_memory = 'text = replace_once(text, old_render, new_render, "memory page")'
new_memory = '''memory_start = text.find("    fn render_memory(")
memory_end = text.find("\\n    fn render_storage(", memory_start)
if memory_start < 0 or memory_end < 0:
    raise SystemExit("memory page: function boundaries not found")
text = text[:memory_start] + new_render.rstrip("\\n") + text[memory_end:]'''
if text.count(old_memory) != 1:
    raise SystemExit("memory page: migration marker is not unique")
text = text.replace(old_memory, new_memory, 1)

build_start = text.find('build = Path("packaging/build-deb.sh")')
build_end = text.find('\n\nverify = Path("packaging/verify-deb.sh")', build_start)
if build_start < 0 or build_end < 0:
    raise SystemExit("packaging migration boundaries not found")

new_build = r'''build = Path("packaging/build-deb.sh")
text = build.read_text()
if "org.betteros.Monitor1.service" not in text:
    text = replace_once(
        text,
        """    install -m 0644 "$ROOT_DIR/packaging/daemon/org.betteros.Manager1.conf" \\
        "$staging_dir/usr/share/dbus-1/system.d/org.betteros.Manager1.conf"
""",
        """    install -m 0644 "$ROOT_DIR/packaging/daemon/org.betteros.Manager1.conf" \\
        "$staging_dir/usr/share/dbus-1/system.d/org.betteros.Manager1.conf"
    install -m 0644 "$ROOT_DIR/packaging/daemon/org.betteros.Monitor1.service" \\
        "$staging_dir/usr/share/dbus-1/system-services/org.betteros.Monitor1.service"
    install -m 0644 "$ROOT_DIR/packaging/daemon/org.betteros.Monitor1.conf" \\
        "$staging_dir/usr/share/dbus-1/system.d/org.betteros.Monitor1.conf"
""",
        "daemon monitor packaging",
    )
if 'make_package better-monitor monitor-gui \'Better OS monitor desktop application\'' in text and 'better-monitor monitor-gui \'Better OS monitor desktop application\' \\\n    "better-manager-daemon (= $VERSION)"' not in text:
    text = replace_once(
        text,
        "make_package better-monitor monitor-gui 'Better OS monitor desktop application'\n",
        "make_package better-monitor monitor-gui 'Better OS monitor desktop application' \\\n    \"better-manager-daemon (= $VERSION)\"\n",
        "monitor daemon recommends",
    )
build.write_text(text)'''
text = text[:build_start] + new_build + text[build_end:]

verify_start = text.find('verify = Path("packaging/verify-deb.sh")')
verify_end = text.find('\n\nchecklist = Path("docs/better-monitor-resources-v1.10.2-parity.md")', verify_start)
if verify_start < 0 or verify_end < 0:
    raise SystemExit("verifier migration boundaries not found")

new_verify = r'''verify = Path("packaging/verify-deb.sh")
text = verify.read_text()
lines = text.splitlines()
slash = chr(92)
if "org.betteros.Monitor1.service" not in text:
    manager_service = next(
        (index for index, line in enumerate(lines)
         if line.strip().startswith("usr/share/dbus-1/system-services/org.betteros.Manager1.service")),
        None,
    )
    manager_conf = next(
        (index for index, line in enumerate(lines)
         if line.strip().startswith("usr/share/dbus-1/system.d/org.betteros.Manager1.conf")),
        None,
    )
    if manager_service is None or manager_conf is None:
        raise SystemExit("verify monitor dbus files: current required-file markers not found")
    lines.insert(manager_service + 1, f"    usr/share/dbus-1/system-services/org.betteros.Monitor1.service {slash}")
    if manager_conf > manager_service:
        manager_conf += 1
    lines.insert(manager_conf + 1, f"    usr/share/dbus-1/system.d/org.betteros.Monitor1.conf {slash}")
if not any("org.betteros.monitor.read-memory-devices" in line for line in lines):
    apply_line = next(
        (index for index, line in enumerate(lines)
         if "grep -q 'org.betteros.manager.apply-transaction'" in line),
        None,
    )
    if apply_line is None:
        raise SystemExit("verify monitor policy: apply-action check not found")
    apply_end = next(
        (index for index in range(apply_line, len(lines)) if lines[index] == "}"),
        None,
    )
    if apply_end is None:
        raise SystemExit("verify monitor policy: apply-action block is incomplete")
    lines[apply_end + 1:apply_end + 1] = [
        f"grep -q 'org.betteros.monitor.read-memory-devices' {slash}",
        '    "$daemon_extract/usr/share/polkit-1/actions/org.betteros.manager.policy" || {',
        "    printf 'The polkit policy does not declare the monitor DMI action\\n' >&2",
        "    exit 1",
        "}",
    ]
verify.write_text("\\n".join(lines) + "\\n")'''
text = text[:verify_start] + new_verify + text[verify_end:]
path.write_text(text)
