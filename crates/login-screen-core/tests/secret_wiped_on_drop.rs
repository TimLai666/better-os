//! A `Secret` and the greetd message built from it leave no copy of the
//! password in memory they give back (the security review's "The password").
//!
//! The global allocator below hands out every block zeroed (a growing
//! `realloc` is a new zeroed block, a copy and a free), so every byte of every
//! block has been written when it is read; it looks at every block before it
//! is freed and counts those that still hold the marker. A dropped `Secret`
//! must give its pages back zeroed, and no code may have copied the password
//! into an ordinary heap block (a `String`, a `Vec`) and let it go unwiped:
//! the test fails on either. The positive control is such a copy.

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

/// Counts a block that still holds the marker.
///
/// # Safety
/// `pointer` and `layout` name a live block this allocator handed out
/// zeroed, so all of it is initialized.
unsafe fn check(pointer: *mut u8, layout: Layout) {
    let bytes = unsafe { std::slice::from_raw_parts(pointer, layout.size()) };
    if holds_marker(bytes) {
        FREED_WITH_MARKER.fetch_add(1, Ordering::SeqCst);
    }
}

// SAFETY: every block comes from the system allocator's `alloc_zeroed` and
// goes back through its `dealloc` with the same layout; a block is only read
// before it is freed. `realloc` keeps the old block's contents up to the
// smaller size, as `GlobalAlloc` requires.
unsafe impl GlobalAlloc for Watching {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe {
            check(pointer, layout);
            System.dealloc(pointer, layout)
        }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return std::ptr::null_mut();
        };
        unsafe {
            let fresh = System.alloc_zeroed(new_layout);
            if fresh.is_null() {
                return fresh;
            }
            std::ptr::copy_nonoverlapping(pointer, fresh, layout.size().min(new_size));
            self.dealloc(pointer, layout);
            fresh
        }
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
    // The check sees a copy in an ordinary heap block, freed without a wipe.
    let before = FREED_WITH_MARKER.load(Ordering::SeqCst);
    drop(std::hint::black_box(marker_text().to_string()));
    assert_eq!(
        FREED_WITH_MARKER.load(Ordering::SeqCst),
        before + 1,
        "the positive control was not seen"
    );
    // And one moved by a growing reallocation.
    let before = FREED_WITH_MARKER.load(Ordering::SeqCst);
    let mut grown = Vec::with_capacity(MARKER.len());
    grown.extend_from_slice(MARKER);
    grown.reserve(4096);
    drop(std::hint::black_box(grown));
    assert!(FREED_WITH_MARKER.load(Ordering::SeqCst) > before);

    // A Secret dropped with the password in it.
    let before = FREED_WITH_MARKER.load(Ordering::SeqCst);
    let mut secret = Secret::try_new().unwrap();
    secret.push_str(marker_text()).unwrap();
    assert_eq!(secret.expose_str(), marker_text());
    drop(secret);
    assert_eq!(
        FREED_WITH_MARKER.load(Ordering::SeqCst),
        before,
        "a Secret was freed unwiped"
    );

    // A Secret wiped on demand, then dropped.
    let mut secret = Secret::try_new().unwrap();
    for character in marker_text().chars() {
        secret.push_char(character).unwrap();
    }
    secret.wipe();
    assert!(secret.is_empty());
    drop(secret);
    assert_eq!(FREED_WITH_MARKER.load(Ordering::SeqCst), before);

    // The greetd message built from it, and the Secret, both dropped.
    let mut secret = Secret::try_new().unwrap();
    secret.push_str(marker_text()).unwrap();
    let message = post_auth_message_response(Some(&secret)).unwrap();
    assert!(holds_marker(message.frame_bytes()));
    drop(message);
    drop(secret);
    assert_eq!(
        FREED_WITH_MARKER.load(Ordering::SeqCst),
        before,
        "the greetd message, or a copy made while building it, was freed unwiped"
    );
}
