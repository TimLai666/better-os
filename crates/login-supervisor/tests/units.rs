//! The login path's units (design/login-screen-spec.md section 5.10, "The
//! units, and what starts what"), read from `packaging/greeter/`: nothing a
//! retry or the fall back stops or starts can stop the supervisor, the
//! sandbox is the security review's, and the backstop runs no Better OS code.

use std::collections::BTreeMap;
use std::path::PathBuf;

use login_supervisor::rules::{UNITS, limits};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/greeter")
}

/// Every `Key=value` of a unit file, by section, values in order.
fn unit(name: &str) -> BTreeMap<String, Vec<String>> {
    let text = std::fs::read_to_string(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut section = String::new();
    let mut keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            section = header.trim_end_matches(']').to_string();
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .unwrap_or_else(|| panic!("{name}: {line}"));
        keys.entry(format!("{section}.{key}"))
            .or_default()
            .push(value.to_string());
    }
    keys
}

fn one(unit: &BTreeMap<String, Vec<String>>, key: &str) -> String {
    let values = unit.get(key).unwrap_or_else(|| panic!("{key} is missing"));
    assert_eq!(values.len(), 1, "{key}: {values:?}");
    values[0].clone()
}

/// Every unit name a list of values names.
fn named(unit: &BTreeMap<String, Vec<String>>, key: &str) -> Vec<String> {
    unit.get(key)
        .into_iter()
        .flatten()
        .flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

const SUPERVISOR: &str = "better-os-login-supervisor.service";
const FILES: [&str; 5] = [
    SUPERVISOR,
    "better-os-greeter.service",
    "better-os-login-fallback@.service",
    "better-os-login-backstop.service",
    "better-os-login-backstop.timer",
];

/// The dependencies that carry a stop, a restart or a start's conflict
/// from one unit to another.
const PROPAGATING: [&str; 8] = [
    "Unit.PartOf",
    "Unit.BindsTo",
    "Unit.Requires",
    "Unit.Requisite",
    "Unit.Conflicts",
    "Unit.PropagatesStopTo",
    "Unit.StopPropagatedFrom",
    "Unit.PropagatesReloadTo",
];

#[test]
fn the_supervisor_depends_on_nothing_that_can_stop_it() {
    let supervisor = unit(SUPERVISOR);
    for key in PROPAGATING {
        assert!(
            !supervisor.contains_key(key),
            "the supervisor's {key} would let another unit stop or restart it"
        );
    }
    // Nor does any other unit of the login path name it in one.
    for file in FILES.iter().filter(|f| **f != SUPERVISOR) {
        let other = unit(file);
        for key in PROPAGATING
            .iter()
            .chain(&["Unit.OnFailure", "Unit.OnSuccess"])
        {
            assert!(
                !named(&other, key).iter().any(|n| n == SUPERVISOR),
                "{file}'s {key} names the supervisor"
            );
        }
    }
    // It is not ordered after logind or D-Bus: a hung one is a stage it
    // watches, never a start it waits for.
    for after in named(&supervisor, "Unit.After") {
        assert!(
            !after.contains("logind") && !after.contains("dbus") && !after.contains("better-os"),
            "After={after}"
        );
    }
}

#[test]
fn the_supervisor_restarts_itself_and_hands_over_to_the_backstop() {
    let s = unit(SUPERVISOR);
    assert_eq!(one(&s, "Service.Type"), "notify");
    assert_eq!(one(&s, "Service.WatchdogSec"), "10");
    assert_eq!(one(&s, "Service.TimeoutStartSec"), "10");
    assert_eq!(one(&s, "Service.Restart"), "always");
    assert_eq!(one(&s, "Service.RestartSec"), "1");
    // systemd 255 runs OnFailure= on every failure otherwise, restarts
    // included: one crash would start the backstop's text login.
    assert_eq!(one(&s, "Service.RestartMode"), "direct");
    assert_eq!(one(&s, "Unit.StartLimitIntervalSec"), "60");
    assert_eq!(one(&s, "Unit.StartLimitBurst"), "5");
    assert_eq!(
        one(&s, "Unit.OnFailure"),
        "better-os-login-backstop.service"
    );
    assert!(named(&s, "Unit.Wants").contains(&"better-os-login-backstop.timer".to_string()));
    assert_eq!(
        one(&s, "Service.ExecStart"),
        "/usr/sbin/better-os-login-supervisor"
    );
    assert_eq!(one(&s, "Service.RuntimeDirectory"), "better-os");
    assert_eq!(one(&s, "Service.RuntimeDirectoryPreserve"), "yes");
    assert_eq!(one(&s, "Service.StateDirectory"), "better-os/boot-watch");
    assert!(!s.contains_key("Install.WantedBy") && !s.contains_key("Install.Alias"));
}

#[test]
fn the_supervisors_sandbox_is_the_security_reviews() {
    let s = unit(SUPERVISOR);
    assert!(
        !s.contains_key("Service.PrivateNetwork"),
        "a network namespace hides plymouthd's abstract socket"
    );
    for (key, value) in [
        ("Service.IPAddressDeny", "any"),
        ("Service.RestrictAddressFamilies", "AF_UNIX"),
        ("Service.ProtectSystem", "strict"),
        ("Service.ProtectHome", "yes"),
        ("Service.NoNewPrivileges", "yes"),
        ("Service.MemoryDenyWriteExecute", "yes"),
        ("Service.SystemCallFilter", "@system-service"),
    ] {
        assert_eq!(one(&s, key), value, "{key}");
    }
    assert_eq!(
        s["Service.DeviceAllow"],
        ["/dev/tty1 rw".to_string(), "/dev/tty2 rw".to_string()]
    );
    assert!(!s.contains_key("Service.ReadWritePaths"));
}

#[test]
fn the_greeters_unit_is_the_supervisors_to_start_and_waits_for_no_splash() {
    let g = unit("better-os-greeter.service");
    assert_eq!(one(&g, "Service.Restart"), "no");
    assert_eq!(one(&g, "Service.TimeoutStopSec"), "4");
    assert_eq!(one(&g, "Service.SendSIGKILL"), "yes");
    assert_eq!(one(&g, "Unit.OnFailure"), "plymouth-quit.service");
    assert_eq!(one(&g, "Unit.StartLimitIntervalSec"), "0");
    assert!(named(&g, "Unit.Conflicts").contains(&"getty@tty1.service".to_string()));
    assert!(
        !named(&g, "Unit.After")
            .iter()
            .any(|a| a == "plymouth-quit-wait.service"),
        "it would wait for a splash only the supervisor quits"
    );
    assert!(
        g.keys().all(|k| !k.starts_with("Install.")),
        "never enabled"
    );
    // Every stop fits the supervisor's 6 s: TimeoutStopSec plus 2 s to reap.
    let stop: u64 = one(&g, "Service.TimeoutStopSec").parse().unwrap();
    assert!((stop + 2) * 1_000_000 <= limits::STOP);
}

#[test]
fn the_text_login_starts_at_once_with_the_supervisors_message() {
    let f = unit("better-os-login-fallback@.service");
    assert_eq!(one(&f, "Unit.Conflicts"), "getty@%i.service");
    assert_eq!(one(&f, "Service.Type"), "simple");
    let exec = one(&f, "Service.ExecStart");
    assert!(exec.starts_with("-/sbin/agetty "), "{exec}");
    assert!(exec.contains("--issue-file /etc/issue:/run/better-os/login-fallback.issue"));
    assert_eq!(one(&f, "Service.TTYPath"), "/dev/%I");
    assert_eq!(one(&f, "Service.Restart"), "always");
}

#[test]
fn the_backstop_runs_only_standard_programs_under_its_three_conditions() {
    let b = unit("better-os-login-backstop.service");
    assert_eq!(one(&b, "Service.Type"), "oneshot");
    assert_eq!(
        b["Service.ExecCondition"],
        [
            "/usr/bin/test ! -e /run/better-os-greeter/ready-seen",
            "/usr/bin/test ! -e /run/better-os/login-fallback.active",
            "/usr/bin/test ! -e /run/better-os/login-held",
        ]
    );
    const STANDARD: [&str; 7] = [
        "/usr/bin/timeout",
        "/usr/bin/pkill",
        "/usr/bin/cp",
        "/usr/bin/touch",
        "/usr/bin/systemctl",
        "/usr/bin/chvt",
        "/usr/bin/test",
    ];
    for exec in b["Service.ExecStart"]
        .iter()
        .chain(&b["Service.ExecCondition"])
    {
        let program = exec
            .trim_start_matches('-')
            .split_whitespace()
            .next()
            .unwrap();
        assert!(
            STANDARD.contains(&program),
            "{program} is not a standard program"
        );
        assert!(!exec.contains("better-os-login-supervisor"), "{exec}");
    }
    let starts: Vec<&String> = b["Service.ExecStart"].iter().collect();
    let marker = starts
        .iter()
        .position(|e| e.contains("touch /run/better-os/login-fallback.active"))
        .unwrap();
    let start = starts
        .iter()
        .position(|e| {
            *e == "/usr/bin/systemctl --no-block start better-os-login-fallback@tty2.service"
        })
        .expect("the text login on tty2");
    assert!(marker < start, "the marker before the start");
    assert!(starts.iter().any(|e| e.contains("plymouth quit")));
    // The message it copies is the supervisor row's, with agetty's date
    // and time.
    let message = std::fs::read_to_string(dir().join("login-supervisor-failed.issue")).unwrap();
    assert!(message.starts_with(
        "The Better OS login screen couldn't be shown (the login supervisor couldn't run, \\d \\t).\n"
    ));
}

#[test]
fn the_backstop_timer_fires_at_ninety_seconds_to_the_second() {
    let t = unit("better-os-login-backstop.timer");
    assert_eq!(one(&t, "Timer.OnActiveSec"), "90");
    assert_eq!(one(&t, "Timer.AccuracySec"), "1s");
    assert_eq!(one(&t, "Timer.Unit"), "better-os-login-backstop.service");
    assert_eq!(limits::BACKSTOP, 90 * 1_000_000);
}

#[test]
fn every_unit_the_supervisor_may_name_has_its_file() {
    for name in UNITS {
        let file = match name.split_once('@') {
            Some((stem, _)) => format!("{stem}@.service"),
            None => name.to_string(),
        };
        assert!(dir().join(&file).is_file(), "{file}");
    }
    let policy = std::fs::read_to_string(dir().join("org.betteros.LoginSupervisor1.conf")).unwrap();
    assert!(policy.contains(r#"<allow own="org.betteros.LoginSupervisor1"/>"#));
    assert!(policy.contains(r#"send_member="LastRecord""#));
}
