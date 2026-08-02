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
path.write_text(text)
