//! A `Secret` and the greetd message built from it leave no copy of the
//! password in memory they give back (the security review's "The password").
//!
//! The global allocator below looks at every block before it is freed and
//! counts the blocks that still hold the marker. A dropped `Secret` must give
//! its pages back zeroed. The marker is only ever in the binary's read-only
//! data and in the buffers under test, so any freed block holding it is one
//! of them, or the positive control that proves the check sees a copy.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use login_screen_core::Secret;
use login_screen_core::greetd::post_auth_message_response;

const MARKER: &[u8] = b"Tr0ub4dor&3-login-screen-core";

static FREED_WITH_MARKER: AtomicUsize = AtomicUsize::new(0);

struct Watching;

fn holds_marker(bytes: &[u8]) -> bool {
    bytes.len() >= MARKER.len() && bytes.windows(MARKER.len()).any(|window| window == MARKER)
}

// SAFETY: every call is forwarded to the system allocator unchanged; the
// block is only read, before it is freed.
unsafe impl GlobalAlloc for Watching {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let bytes = unsafe { std::slice::from_raw_parts(pointer, layout.size()) };
        if holds_marker(bytes) {
            FREED_WITH_MARKER.fetch_add(1, Ordering::SeqCst);
        }
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let bytes = unsafe { std::slice::from_raw_parts(pointer, layout.size()) };
        if holds_marker(bytes) {
            FREED_WITH_MARKER.fetch_add(1, Ordering::SeqCst);
        }
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Watching = Watching;

fn marker_text() -> &'static str {
    std::str::from_utf8(MARKER).unwrap()
}

// One test, so no other test thread frees a block while this one counts.
#[test]
fn no_freed_memory_holds_the_password() {
    // The check sees a copy that is freed without being wiped.
    let before = FREED_WITH_MARKER.load(Ordering::SeqCst);
    drop(std::hint::black_box(MARKER.to_vec()));
    assert_eq!(
        FREED_WITH_MARKER.load(Ordering::SeqCst),
        before + 1,
        "the positive control was not seen"
    );

    // A Secret dropped with the password in it.
    let before = FREED_WITH_MARKER.load(Ordering::SeqCst);
    let mut secret = Secret::new();
    secret.push_str(marker_text()).unwrap();
    assert_eq!(secret.expose_str(), marker_text());
    drop(secret);
    assert_eq!(
        FREED_WITH_MARKER.load(Ordering::SeqCst),
        before,
        "a Secret was freed unwiped"
    );

    // A Secret wiped on demand, then dropped.
    let mut secret = Secret::new();
    for character in marker_text().chars() {
        secret.push_char(character).unwrap();
    }
    secret.wipe();
    assert!(secret.is_empty());
    drop(secret);
    assert_eq!(FREED_WITH_MARKER.load(Ordering::SeqCst), before);

    // The greetd message built from it, and the Secret, both dropped.
    let mut secret = Secret::new();
    secret.push_str(marker_text()).unwrap();
    let message = post_auth_message_response(Some(&secret));
    assert!(holds_marker(message.as_bytes()));
    drop(message);
    drop(secret);
    assert_eq!(
        FREED_WITH_MARKER.load(Ordering::SeqCst),
        before,
        "the greetd message was freed unwiped"
    );
}
