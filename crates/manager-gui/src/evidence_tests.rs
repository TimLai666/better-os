//! Which refusals a person is told are a machine mismatch.
//!
//! The daemon refuses a plan with `daemon.plan_rejected` for a release or an
//! architecture mismatch, and the client raises the same key for other
//! refusals with their own detail. Only the first kind means the machine is
//! the wrong one.

use crate::app::plan_rejection_is_host_mismatch;

#[test]
fn a_release_mismatch_from_the_daemon_is_a_machine_mismatch() {
    assert!(plan_rejection_is_host_mismatch(
        "daemon.error.plan_rejected:plan targets release 24.04 but this host is 18"
    ));
}

#[test]
fn an_architecture_mismatch_from_the_daemon_is_a_machine_mismatch() {
    assert!(plan_rejection_is_host_mismatch(
        "daemon.error.plan_rejected:plan targets architecture arm64 but this host is amd64"
    ));
}

#[test]
fn a_bare_refusal_without_a_detail_is_not_a_machine_mismatch() {
    assert!(!plan_rejection_is_host_mismatch("daemon.plan_rejected"));
}

#[test]
fn a_refusal_for_a_component_outside_the_catalog_is_not_a_machine_mismatch() {
    assert!(!plan_rejection_is_host_mismatch(
        "daemon.error.plan_rejected:better-x is not a Better OS component"
    ));
}

#[test]
fn a_refusal_for_an_oversized_artifact_is_not_a_machine_mismatch() {
    assert!(!plan_rejection_is_host_mismatch(
        "daemon.error.plan_rejected:artifact too large"
    ));
}
