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
const FILES: [&str; 9] = [
    SUPERVISOR,
    "better-os-greeter.service",
    "better-os-login-fallback@.service",
    "better-os-login-backstop.service",
    "better-os-login-backstop-failed.service",
    "better-os-login-backstop.timer",
    "better-os-login-backstop-failed.timer",
    "getty@tty1.service.d/better-os-login.conf",
    "plymouth-quit.service.d/better-os-login.conf",
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
    // A hang loop (watchdog 10 s, abort 5 s, restart 1 s) makes its five
    // starts in 80 s: inside the window, so it reaches OnFailure= too.
    assert_eq!(one(&s, "Unit.StartLimitIntervalSec"), "120");
    assert_eq!(one(&s, "Unit.StartLimitBurst"), "5");
    let hang_cycle = 10 + 5 + 1;
    assert!(4 * hang_cycle < 120);
    assert_eq!(
        one(&s, "Unit.OnFailure"),
        "better-os-login-backstop-failed.timer"
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
        [
            "/dev/tty1 rw".to_string(),
            "/dev/tty8 rw".to_string(),
            "/dev/tty9 rw".to_string()
        ]
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
    assert_eq!(one(&f, "Unit.StartLimitIntervalSec"), "0");
    // Past logind's automatic VTs (NAutoVTs=6, VTs 1 to 6): no autovt getty
    // is ever started there to conflict it out.
    for name in &UNITS[1..] {
        let number: u8 = name
            .trim_start_matches("better-os-login-fallback@tty")
            .trim_end_matches(".service")
            .parse()
            .unwrap();
        assert!(number > 6, "{name}");
    }
}

const STANDARD: [&str; 10] = [
    "/usr/bin/timeout",
    "/usr/bin/pkill",
    "/usr/bin/cp",
    "/usr/bin/touch",
    "/usr/bin/systemctl",
    "/usr/bin/chvt",
    "/usr/bin/test",
    "/bin/sh",
    "/usr/bin/find",
    "/usr/bin/grep",
];

/// The backstops' own steps: standard programs only, the text login started
/// before anything that may wait, and every wait bounded.
fn check_backstop_steps(file: &str) {
    let b = unit(file);
    assert_eq!(one(&b, "Service.Type"), "oneshot");
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
            "{file}: {program} is not a standard program"
        );
        // It may look at the supervisor's state; it never runs its program.
        assert!(
            !exec.contains("/usr/sbin/better-os-login-supervisor"),
            "{exec}"
        );
    }
    let starts: Vec<&String> = b["Service.ExecStart"].iter().collect();
    let position = |what: &str| {
        starts
            .iter()
            .position(|e| e.contains(what))
            .unwrap_or_else(|| panic!("{file}: no step with {what}"))
    };
    let start = position("systemctl --no-block start better-os-login-fallback@tty9.service");
    assert!(
        position("cp --update=none") < start,
        "the message before the start"
    );
    assert!(start < position("touch /run/better-os/login-fallback.active"));
    assert!(
        start < position("plymouth quit"),
        "the prompt before the splash's wait"
    );
    for step in &starts {
        if step.contains("plymouth quit") || step.contains("chvt") {
            assert!(
                step.starts_with("-/usr/bin/timeout "),
                "{file}: unbounded {step}"
            );
        }
    }
    let bounded: u64 = starts
        .iter()
        .filter_map(|e| e.strip_prefix("-/usr/bin/timeout "))
        .map(|rest| {
            rest.split_whitespace()
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        })
        .sum();
    // The timer at 82 s plus every bounded step: the text login by 90 s.
    assert!(
        limits::BACKSTOP / 1_000_000 + bounded <= 90,
        "{file}: {bounded} s of waits"
    );
}

#[test]
fn the_timers_backstop_runs_only_standard_programs_under_its_three_conditions() {
    check_backstop_steps("better-os-login-backstop.service");
    let b = unit("better-os-login-backstop.service");
    assert_eq!(
        b["Service.ExecCondition"],
        [
            "/usr/bin/test ! -e /run/better-os-greeter/ready-seen",
            "/usr/bin/test ! -e /run/better-os/login-fallback.active",
            "/usr/bin/test ! -e /run/better-os/login-held",
        ]
    );
    // The message it copies is the supervisor row's, with agetty's date
    // and time.
    let message = std::fs::read_to_string(dir().join("login-supervisor-failed.issue")).unwrap();
    assert!(message.starts_with(
        "The Better OS login screen couldn't be shown (the login supervisor couldn't run, \\d \\t).\n"
    ));
}

#[test]
fn the_failed_supervisors_backstop_looks_at_the_present_not_at_the_boots_history() {
    check_backstop_steps("better-os-login-backstop-failed.service");
    let b = unit("better-os-login-backstop-failed.service");
    let conditions = &b["Service.ExecCondition"];
    for history in ["ready-seen", "login-held"] {
        assert!(
            !conditions.iter().any(|c| c.contains(history)),
            "{history} is this boot's history, not the present"
        );
    }
    assert!(conditions.iter().any(|c| c.contains("is-active --quiet better-os-login-fallback@tty8.service better-os-login-fallback@tty9.service")));
    // A live greeter is one that says `ready` and wrote it in the last
    // 30 s: one stuck on `starting` is no login screen.
    assert!(
        conditions
            .iter()
            .any(|c| c.contains("/run/better-os-greeter/alive -newermt")
                && c.contains("grep -D skip -qx ready /run/better-os-greeter/alive"))
    );
    // And only while the supervisor does not run: the timer that looks
    // again every 30 s must never act beside a supervisor started again.
    assert!(conditions.iter().any(|c| {
        c.contains("! /usr/bin/systemctl is-active --quiet better-os-login-supervisor.service")
    }));
    let t = unit("better-os-login-backstop-failed.timer");
    assert_eq!(one(&t, "Timer.OnActiveSec"), "0");
    assert_eq!(one(&t, "Timer.OnUnitInactiveSec"), "30");
    assert_eq!(one(&t, "Timer.AccuracySec"), "1s");
    assert_eq!(
        one(&t, "Timer.Unit"),
        "better-os-login-backstop-failed.service"
    );
    assert!(
        conditions
            .iter()
            .any(|c| c.contains("SERVICE=greetd") && c.contains("-D skip"))
    );
}

#[test]
fn the_backstop_timer_counts_from_the_supervisors_first_start() {
    let t = unit("better-os-login-backstop.timer");
    assert_eq!(one(&t, "Timer.OnActiveSec"), "82");
    assert_eq!(one(&t, "Timer.AccuracySec"), "1s");
    assert_eq!(one(&t, "Timer.Unit"), "better-os-login-backstop.service");
    assert_eq!(limits::BACKSTOP, 82 * 1_000_000);
    // Not the default dependencies, which order a timer only after
    // sysinit.target, but right before the supervisor, after everything
    // the supervisor waits for.
    assert_eq!(one(&t, "Unit.DefaultDependencies"), "no");
    assert!(named(&t, "Unit.Before").contains(&SUPERVISOR.to_string()));
    let supervisor_after = named(&unit(SUPERVISOR), "Unit.After");
    let timer_after = named(&t, "Unit.After");
    for after in &supervisor_after {
        assert!(
            timer_after.contains(after),
            "the timer is not after {after}"
        );
    }
    assert!(!named(&t, "Unit.Before").contains(&"timers.target".to_string()));
}

#[test]
fn getty_on_tty1_and_plymouth_quit_stand_aside_only_while_the_supervisor_runs() {
    for file in [
        "getty@tty1.service.d/better-os-login.conf",
        "plymouth-quit.service.d/better-os-login.conf",
    ] {
        let d = unit(file);
        let condition = one(&d, "Service.ExecCondition");
        // The supervisor's own state in this boot, never the link: a boot
        // that does not start the display manager keeps stock behaviour.
        assert_eq!(
            condition,
            "/bin/sh -c 'case \"$$(/usr/bin/timeout 5 /usr/bin/systemctl show --property=ActiveState --value better-os-login-supervisor.service 2>/dev/null)\" in active|activating|reloading) exit 1 ;; esac'",
            "{file}"
        );
        assert!(!condition.contains("display-manager.service"), "{file}");
        assert_eq!(one(&d, "Unit.After"), SUPERVISOR, "{file}");
        assert_eq!(d.len(), 2, "{file} does nothing else");
    }
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
