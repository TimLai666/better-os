//! The USB-C recovery's decisions against kernel log excerpts from ticket
//! 101 (the owner's M5406WA, kernel 7.0.0-42, message text only: no host
//! name, address, or anything else about the person).

use hardware_core::usb_recovery::*;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/usb-recovery/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

fn address() -> PciAddress {
    PciAddress::parse("0000:63:00.4").unwrap()
}

fn affected() -> UsbPresent {
    UsbPresent {
        model: MachineModel {
            board_name: Some("M5406WA".into()),
            product_name: Some("ASUS Vivobook S 14 M5406WA_M5406WA".into()),
            sys_vendor: Some("ASUSTeK COMPUTER INC.".into()),
        },
        typec_ports: 2,
        supplies: vec![
            UsbCSupply {
                name: "ucsi-source-psy-USBC000:001".into(),
                online: Some(true),
            },
            UsbCSupply {
                name: "ucsi-source-psy-USBC000:002".into(),
                online: Some(false),
            },
        ],
        mains_online: Some(true),
        controllers: vec![XhciFunction {
            address: address(),
            vendor: 0x1022,
            device: 0x151e,
            driver: Some("xhci_hcd".into()),
            root_hubs: 0,
            devices: 0,
        }],
    }
}

#[test]
fn the_ucsi_timeouts_after_a_resume_are_found_and_active() {
    let findings = classify(&fixture("ucsi-after-resume.log"));
    assert_eq!(
        findings.ucsi,
        UcsiFindings {
            timeouts: 1,
            notification_failures: 1,
            reloads: 0,
            active: true,
        }
    );
    assert!(findings.controllers.is_empty());
    assert_eq!(findings.sleep.attempts, 1);
    assert_eq!(findings.sleep.streak, 0);
    assert_eq!(ucsi_reload(&findings, &affected()), Ok(()));
}

#[test]
fn a_verified_reload_ends_the_fault_until_the_next_timeout() {
    let findings = classify(&fixture("ucsi-reloaded.log"));
    assert!(!findings.ucsi.active);
    assert_eq!(findings.ucsi.reloads, 1);
    assert_eq!(
        ucsi_reload(&findings, &affected()),
        Err(Refusal::NothingToReset)
    );
    let again = format!(
        "{}ucsi_acpi USBC000:00: failed to re-enable notifications (-110)\n",
        fixture("ucsi-reloaded.log")
    );
    assert!(classify(&again).ucsi.active);
}

#[test]
fn the_dead_host_and_the_blocked_sleep_are_found() {
    let findings = classify(&fixture("xhci-died-sleep-blocked.log"));
    assert_eq!(
        findings.controllers,
        vec![ControllerDeath {
            address: address(),
            page_fault: true,
            restore_timeout: true,
            post_resume_error: true,
            rebound: false,
        }]
    );
    assert!(findings.hub_work_hung);
    assert_eq!(
        findings.sleep,
        SleepFindings {
            attempts: 3,
            blocked_by_hub_work: 2,
            streak: 2,
        }
    );
    assert_eq!(findings.sleep.run_start(), 2);
    // The hub work is stuck: only a restart frees it, and nothing is
    // unbound.
    assert_eq!(
        rebind_target(&findings, &affected(), false),
        Err(Refusal::RestartNeeded)
    );
}

#[test]
fn a_dead_host_with_no_stuck_hub_work_is_rebound_and_only_that_one() {
    let log = fixture("xhci-died-sleep-blocked.log");
    let died_only: String = log
        .lines()
        .take_while(|line| !line.starts_with("ucsi_acpi USBC000:00: failed"))
        .map(|line| format!("{line}\n"))
        .collect();
    let findings = classify(&died_only);
    assert!(!findings.hub_work_hung);
    assert_eq!(rebind_target(&findings, &affected(), false), Ok(address()));
    // Seen stuck in /proc a moment ago: a restart, not an unbind.
    assert_eq!(
        rebind_target(&findings, &affected(), true),
        Err(Refusal::RestartNeeded)
    );
    // Another kind of controller, another driver, or none at that address.
    let mut other = affected();
    other.controllers[0].device = 0x151f;
    assert_eq!(
        rebind_target(&findings, &other, false),
        Err(Refusal::ControllerNotRecognised)
    );
    let mut unbound = affected();
    unbound.controllers[0].driver = None;
    assert_eq!(
        rebind_target(&findings, &unbound, false),
        Err(Refusal::ControllerNotRecognised)
    );
    let mut gone = affected();
    gone.controllers.clear();
    assert_eq!(
        rebind_target(&findings, &gone, false),
        Err(Refusal::ControllerNotRecognised)
    );
    // Rebound and verified: nothing more to do.
    let rebound = classify(&format!("{died_only}{}\n", rebound_marker(&address())));
    assert_eq!(
        rebind_target(&rebound, &affected(), false),
        Err(Refusal::NothingToReset)
    );
}

#[test]
fn a_machine_that_is_not_affected_is_offered_nothing() {
    let mut other = affected();
    other.model.board_name = Some("UX3405MA".into());
    for log in ["ucsi-after-resume.log", "xhci-died-sleep-blocked.log"] {
        let findings = classify(&fixture(log));
        assert_eq!(ucsi_reload(&findings, &other), Err(Refusal::NotAffected));
        assert_eq!(
            rebind_target(&findings, &other, false),
            Err(Refusal::NotAffected)
        );
    }
}

#[test]
fn a_working_sleep_or_another_failure_ends_the_run() {
    assert_eq!(classify(&fixture("healthy.log")).sleep.streak, 0);
    let broken = classify(&fixture("streak-broken.log"));
    assert_eq!(broken.sleep.attempts, 4);
    assert_eq!(broken.sleep.blocked_by_hub_work, 3);
    assert_eq!(broken.sleep.streak, 1);
    assert_eq!(classify("").sleep, SleepFindings::default());
}

#[test]
fn the_overnight_guard_notifies_once_and_powers_off_only_when_chosen_and_safe() {
    let findings = classify(&fixture("xhci-died-sleep-blocked.log"));
    let on = UsbRecoverySettings::default().with(UsbChoice::ControllerAutomatic, true);
    let closed = GuardWorld {
        lid_closed: Some(true),
        ..GuardWorld::default()
    };
    // Off: nothing at all.
    assert_eq!(
        guard(&findings, &UsbRecoverySettings::default(), closed),
        GuardAction::default()
    );
    // On, the default: notify, never power off.
    assert_eq!(
        guard(&findings, &on, closed),
        GuardAction {
            notify: true,
            power_off: false
        }
    );
    // Once per run of failures.
    let told = GuardWorld {
        notified_run: Some(findings.sleep.run_start()),
        ..closed
    };
    assert!(!guard(&findings, &on, told).notify);
    // Chosen: off only with the lid closed, no block on shutdown, nobody
    // else's session.
    let chosen = on.with(UsbChoice::PowerOffWhenSleepBlocked, true);
    assert!(guard(&findings, &chosen, closed).power_off);
    for world in [
        GuardWorld::default(),
        GuardWorld {
            lid_closed: Some(false),
            ..GuardWorld::default()
        },
        GuardWorld {
            shutdown_blocked: true,
            ..closed
        },
        GuardWorld {
            other_people: true,
            ..closed
        },
    ] {
        assert!(!guard(&findings, &chosen, world).power_off, "{world:?}");
    }
    // One failure is not a loop.
    let once = classify(&fixture("streak-broken.log"));
    assert_eq!(guard(&once, &chosen, closed), GuardAction::default());
}

#[test]
fn a_reload_is_verified_by_both_ports_and_the_charger() {
    let restored = verify_ucsi(&affected());
    assert!(restored.complete(), "{restored:?}");
    let mut one_port = affected();
    one_port.typec_ports = 1;
    assert!(!verify_ucsi(&one_port).complete());
    let mut not_shown = affected();
    for supply in &mut not_shown.supplies {
        supply.online = Some(false);
    }
    assert_eq!(
        verify_ucsi(&not_shown),
        Restored::Ucsi {
            ports: 2,
            expected_ports: 2,
            charger: ChargerShown::NotShown
        }
    );
    not_shown.mains_online = Some(false);
    assert!(verify_ucsi(&not_shown).complete());
}

#[test]
fn a_rebind_is_verified_by_the_driver_and_its_root_hubs() {
    assert!(!verify_rebind(&address(), &affected()).complete());
    let mut back = affected();
    back.controllers[0].root_hubs = 2;
    back.controllers[0].devices = 1;
    assert_eq!(
        verify_rebind(&address(), &back),
        Restored::Controller {
            address: address(),
            bound: true,
            root_hubs: 2,
            devices: 1
        }
    );
    assert!(verify_rebind(&address(), &back).complete());
}

#[test]
fn the_hub_work_is_stuck_only_when_the_same_worker_stays_blocked() {
    let worker = |pid, comm: &str, state| Worker {
        pid,
        comm: comm.into(),
        state,
    };
    let stuck = [worker(1115178, "kworker/12:2+usb_hub_wq", 'D')];
    assert!(hub_work_stuck(&stuck, &stuck));
    assert!(!hub_work_stuck(&stuck, &[]));
    assert!(!hub_work_stuck(
        &stuck,
        &[worker(1115178, "kworker/12:2-usb_hub_wq", 'I')]
    ));
    assert!(!hub_work_stuck(
        &[worker(7, "kworker/1:0+usb_hub_wq", 'R')],
        &[worker(7, "kworker/1:0+usb_hub_wq", 'D')]
    ));
    assert!(!hub_work_stuck(
        &[worker(7, "kworker/1:0+events", 'D')],
        &[worker(7, "kworker/1:0+events", 'D')]
    ));
}

/// The query's `--grep` keeps every line the classifier reads and drops
/// the rest, so a grepped log classifies exactly like the whole one. PCRE2
/// and Rust's `regex` agree on this pattern's syntax.
#[test]
fn the_grep_keeps_exactly_what_the_classifier_reads() {
    let pattern = regex::Regex::new(KERNEL_LOG_PATTERN).unwrap();
    for name in [
        "ucsi-after-resume.log",
        "ucsi-reloaded.log",
        "xhci-died-sleep-blocked.log",
        "healthy.log",
        "streak-broken.log",
    ] {
        let whole = fixture(name);
        let grepped: String = whole
            .lines()
            .filter(|line| pattern.is_match(line))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(classify(&grepped), classify(&whole), "{name}");
    }
    assert!(!pattern.is_match("    in-flight: 1115178:hub_event for 48s"));
    assert!(!pattern.is_match("usb 1-1: USB disconnect, device number 2"));
    let args = kernel_log_args(true);
    assert!(args.contains(&format!("--grep={KERNEL_LOG_PATTERN}")));
    assert!(
        !kernel_log_args(false)
            .iter()
            .any(|a| a.starts_with("--grep"))
    );
    assert_eq!(
        &args[args.len() - 4..],
        [
            "_TRANSPORT=kernel",
            "+",
            "_UID=0",
            "SYSLOG_IDENTIFIER=better-manager-daemon"
        ]
    );
}

#[test]
fn findings_carry_no_log_text() {
    let findings = classify(&fixture("xhci-died-sleep-blocked.log"));
    let wire = serde_json::to_string(&findings).unwrap();
    for text in ["hub_event", "USBC000", "AMD-Vi", "kworker", "1115178"] {
        assert!(!wire.contains(text), "{text} in {wire}");
    }
    let back: UsbFindings = serde_json::from_str(&wire).unwrap();
    assert_eq!(back, findings);
    assert!(serde_json::from_str::<PciAddress>("\"../../x\"").is_err());
}
