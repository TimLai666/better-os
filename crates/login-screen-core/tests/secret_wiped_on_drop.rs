//! A `Secret` and the greetd message built from it leave no copy of the
//! password in memory they give back (the security review's "The password").
//!
//! The global allocator below looks at every page-aligned block before it is
//! freed and counts those that still hold the marker. A dropped `Secret` must
//! give its pages back zeroed.
//!
//! Only page-aligned blocks are read: the password buffers are the only ones
//! here (each is its own page-aligned `alloc_zeroed` allocation, written only
//! a byte at a time, so every byte is initialized), and reading any other
//! block could read bytes nothing ever wrote, which Rust does not allow. The
//! positive control is a page-aligned block of the same kind, freed without
//! a wipe.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use login_screen_core::Secret;
use login_screen_core::greetd::post_auth_message_response;

const MARKER: &[u8] = b"Tr0ub4dor&3-login-screen-core";
/// The smallest page Linux uses; every password buffer is aligned to at
/// least this.
const PAGE: usize = 4096;

static FREED_WITH_MARKER: AtomicUsize = AtomicUsize::new(0);

struct Watching;

fn holds_marker(bytes: &[u8]) -> bool {
    bytes.len() >= MARKER.len() && bytes.windows(MARKER.len()).any(|window| window == MARKER)
}

/// Counts a page-aligned block that still holds the marker.
///
/// # Safety
/// `pointer` and `layout` name a live block; when the alignment is a page
/// it is a block made by `alloc_zeroed`, so all of it is initialized.
unsafe fn check(pointer: *mut u8, layout: Layout) {
    if layout.align() >= PAGE {
        let bytes = unsafe { std::slice::from_raw_parts(pointer, layout.size()) };
        if holds_marker(bytes) {
            FREED_WITH_MARKER.fetch_add(1, Ordering::SeqCst);
        }
    }
}

// SAFETY: every call is forwarded to the system allocator unchanged; a
// page-aligned block is only read, before it is freed. Plain `alloc` of a
// page-aligned layout is handed out zeroed, so even a block that did not come
// from `alloc_zeroed` is initialized when it is read.
unsafe impl GlobalAlloc for Watching {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() >= PAGE {
            unsafe { System.alloc_zeroed(layout) }
        } else {
            unsafe { System.alloc(layout) }
        }
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
        unsafe {
            check(pointer, layout);
            System.realloc(pointer, layout, new_size)
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
    // The check sees a copy in a page-aligned block freed without a wipe.
    let before = FREED_WITH_MARKER.load(Ordering::SeqCst);
    let layout = Layout::from_size_align(PAGE, PAGE).unwrap();
    // SAFETY: a non-zero layout; the block is written within its size and
    // freed with the same layout.
    unsafe {
        let block = std::alloc::alloc_zeroed(layout);
        assert!(!block.is_null());
        std::ptr::copy_nonoverlapping(MARKER.as_ptr(), block.add(100), MARKER.len());
        std::alloc::dealloc(std::hint::black_box(block), layout);
    }
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
    let message = post_auth_message_response(Some(&secret)).unwrap();
    assert!(holds_marker(message.as_bytes()));
    drop(message);
    drop(secret);
    assert_eq!(
        FREED_WITH_MARKER.load(Ordering::SeqCst),
        before,
        "the greetd message was freed unwiped"
    );
}
