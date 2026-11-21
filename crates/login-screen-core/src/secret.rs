//! The password buffer (`engineering/specs/login-screen-security.md`, "The
//! password").
//!
//! A password lives in one fixed-capacity buffer that is allocated once and
//! never grows, so no copy of it is ever left behind by a reallocation the way
//! a `String` leaves one. The buffer:
//!
//! - is its own page-aligned allocation, so locking and unlocking its pages
//!   never touches a page another value shares (Linux does not count `mlock`
//!   calls, so a `munlock` on a shared page would unlock the neighbour too);
//! - is locked in memory with `mlock`, so it is never written to swap, and is
//!   marked `MADV_DONTDUMP`, so it never reaches a core file;
//! - is wiped with `zeroize` on [`Secret::wipe`] and when it is dropped;
//! - implements no `Debug`, `Display`, `Clone`, `Copy` or `Serialize`, so it
//!   cannot reach a log line, an error message or a panic message by
//!   formatting, and it cannot be duplicated.
//!
//! A failed `mlock` (an `RLIMIT_MEMLOCK` too small) does not stop a login: the
//! buffer still wipes itself, and [`Secret::is_locked`] says the pages may be
//! swapped. That degradation is not silent: the program writes
//! [`NOT_LOCKED_LOG_LINE`] to the journal once (tickets 262 and 264), which
//! names no content.
//!
//! A page size the kernel will not give (`sysconf(_SC_PAGESIZE)` failing, or
//! a value that is not a power of two) is an error, never a guessed 4096:
//! on an arm64 kernel with 16 KiB or 64 KiB pages a guess would break the
//! alignment the rest relies on.

use std::alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error};
use std::ptr::NonNull;
use std::sync::OnceLock;

use zeroize::Zeroize;

/// The one journal line a program writes when a buffer's pages could not be
/// locked. It says what happened and why, and nothing that was typed.
pub const NOT_LOCKED_LOG_LINE: &str = "BETTER_OS_SECRET_LOCKED=no: a password buffer could not be locked in memory (mlock failed, RLIMIT_MEMLOCK too small); it may be written to swap";

/// Why a password buffer could not be made.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SecretError {
    #[error("the kernel did not report a usable page size")]
    PageSize,
}

/// How many bytes a password may have. Better OS's password field stops here
/// (256 characters of a keyboard layout's ASCII).
pub const SECRET_CAPACITY: usize = 256;

/// A field that is full refuses the character rather than dropping a part of
/// it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("the password field is full")]
pub struct SecretFull;

/// One page-aligned, locked, self-wiping allocation of at least `len` bytes.
struct LockedPages {
    pointer: NonNull<u8>,
    layout: Layout,
    locked: bool,
}

/// A `sysconf(_SC_PAGESIZE)` answer as a page size, or `None` when it is not
/// one.
fn usable_page_size(answer: libc::c_long) -> Option<usize> {
    usize::try_from(answer)
        .ok()
        .filter(|size| size.is_power_of_two())
}

fn page_size() -> Result<usize, SecretError> {
    static PAGE: OnceLock<Option<usize>> = OnceLock::new();
    // SAFETY: sysconf has no preconditions.
    PAGE.get_or_init(|| usable_page_size(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }))
        .ok_or(SecretError::PageSize)
}

impl LockedPages {
    fn new(len: usize) -> Result<Self, SecretError> {
        let page = page_size()?;
        let size = len.div_ceil(page).max(1) * page;
        let layout = Layout::from_size_align(size, page).map_err(|_| SecretError::PageSize)?;
        // SAFETY: the layout has a non-zero size.
        let raw = unsafe { alloc_zeroed(layout) };
        let Some(pointer) = NonNull::new(raw) else {
            handle_alloc_error(layout)
        };
        // SAFETY: `pointer` names `size` bytes this value owns, page-aligned.
        let locked = unsafe { libc::mlock(pointer.as_ptr().cast(), size) } == 0;
        // SAFETY: as above; a failure only means the pages may reach a core
        // file, which `PR_SET_DUMPABLE` (ticket 263) also prevents.
        unsafe {
            libc::madvise(pointer.as_ptr().cast(), size, libc::MADV_DONTDUMP);
        }
        Ok(Self {
            pointer,
            layout,
            locked,
        })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: the allocation is `layout.size()` initialized bytes owned
        // by this value for as long as it lives.
        unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.layout.size()) }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `bytes`, and `&mut self` makes the borrow unique.
        unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr(), self.layout.size()) }
    }
}

impl Drop for LockedPages {
    fn drop(&mut self) {
        self.bytes_mut().zeroize();
        // SAFETY: the same range `new` locked and allocated with this layout.
        unsafe {
            if self.locked {
                libc::munlock(self.pointer.as_ptr().cast(), self.layout.size());
            }
            dealloc(self.pointer.as_ptr(), self.layout);
        }
    }
}

// SAFETY: the allocation is owned by exactly one value and has no interior
// mutability; moving it to another thread moves the ownership with it.
unsafe impl Send for LockedPages {}
// SAFETY: shared references only read.
unsafe impl Sync for LockedPages {}

/// A fixed-capacity byte buffer on locked pages. The two public types below
/// are the only ways to fill one.
pub(crate) struct SecretBytes<const N: usize> {
    pages: LockedPages,
    len: usize,
}

impl<const N: usize> SecretBytes<N> {
    pub(crate) fn try_new() -> Result<Self, SecretError> {
        Ok(Self {
            pages: LockedPages::new(N)?,
            len: 0,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<(), SecretFull> {
        let end = self.len.checked_add(bytes.len()).ok_or(SecretFull)?;
        if end > N {
            return Err(SecretFull);
        }
        self.pages.bytes_mut()[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        if len < self.len {
            let end = self.len;
            self.pages.bytes_mut()[len..end].zeroize();
            self.len = len;
        }
    }

    pub(crate) fn overwrite(&mut self, at: usize, bytes: &[u8]) {
        self.pages.bytes_mut()[at..at + bytes.len()].copy_from_slice(bytes);
    }

    pub(crate) fn wipe(&mut self) {
        self.pages.bytes_mut().zeroize();
        self.len = 0;
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.pages.bytes()[..self.len]
    }

    pub(crate) fn is_locked(&self) -> bool {
        self.pages.locked
    }

    #[cfg(test)]
    pub(crate) fn storage(&self) -> &[u8] {
        self.pages.bytes()
    }
}

/// A password, or any answer to a PAM prompt, as the person types it.
///
/// It holds valid UTF-8 at all times, because it is changed only a character
/// at a time.
pub struct Secret {
    bytes: SecretBytes<SECRET_CAPACITY>,
}

impl Default for Secret {
    fn default() -> Self {
        Self::new()
    }
}

impl Secret {
    /// A new, empty buffer, or the reason none can be made.
    pub fn try_new() -> Result<Self, SecretError> {
        Ok(Self {
            bytes: SecretBytes::try_new()?,
        })
    }

    /// A new, empty buffer. Panics when the kernel reports no usable page
    /// size, which Linux always does; a program that would rather say so uses
    /// [`Secret::try_new`].
    pub fn new() -> Self {
        Self::try_new().expect("the kernel reports a usable page size")
    }

    /// [`NOT_LOCKED_LOG_LINE`] when this buffer's pages are not locked, for
    /// the program to write to the journal once.
    pub fn lock_warning(&self) -> Option<&'static str> {
        (!self.is_locked()).then_some(NOT_LOCKED_LOG_LINE)
    }

    pub fn capacity(&self) -> usize {
        SECRET_CAPACITY
    }

    /// Bytes held.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.len() == 0
    }

    /// How many characters it holds, which is how many dots the field draws.
    pub fn char_count(&self) -> usize {
        self.expose_str().chars().count()
    }

    /// Appends one character, or refuses it whole when it does not fit.
    pub fn push_char(&mut self, character: char) -> Result<(), SecretFull> {
        let mut encoded = [0u8; 4];
        let text = character.encode_utf8(&mut encoded);
        let result = self.bytes.push(text.as_bytes());
        encoded.zeroize();
        result
    }

    /// Appends every character of `text`, or none of them when they do not
    /// all fit.
    pub fn push_str(&mut self, text: &str) -> Result<(), SecretFull> {
        if self.len() + text.len() > SECRET_CAPACITY {
            return Err(SecretFull);
        }
        self.bytes.push(text.as_bytes())
    }

    /// Removes the last character (Backspace) and wipes its bytes. Returns
    /// whether there was one.
    pub fn pop_char(&mut self) -> bool {
        let Some((start, _)) = self.expose_str().char_indices().next_back() else {
            return false;
        };
        self.bytes.truncate(start);
        true
    }

    /// Wipes every byte and empties it. The allocation stays for the next
    /// prompt.
    pub fn wipe(&mut self) {
        self.bytes.wipe();
    }

    /// Whether its pages are locked in memory (`mlock` succeeded).
    pub fn is_locked(&self) -> bool {
        self.bytes.is_locked()
    }

    /// The text, for the one place that hands it on: the greetd message
    /// builder or the PAM conversation reply.
    pub fn expose_str(&self) -> &str {
        // Only whole characters are ever pushed or removed.
        std::str::from_utf8(self.bytes.as_bytes()).expect("a Secret holds whole characters")
    }

    #[cfg(test)]
    pub(crate) fn storage(&self) -> &[u8] {
        self.bytes.storage()
    }
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;

    // Autoref probes: the inherent method exists only when the trait is
    // implemented, and wins over the blanket trait method when it does.
    struct Probe<T>(PhantomData<T>);

    trait NotDebug {
        fn is_debug(&self) -> bool {
            false
        }
    }
    impl<T> NotDebug for Probe<T> {}
    impl<T: std::fmt::Debug> Probe<T> {
        #[allow(dead_code)]
        fn is_debug(&self) -> bool {
            true
        }
    }

    trait NotDisplay {
        fn is_display(&self) -> bool {
            false
        }
    }
    impl<T> NotDisplay for Probe<T> {}
    impl<T: std::fmt::Display> Probe<T> {
        #[allow(dead_code)]
        fn is_display(&self) -> bool {
            true
        }
    }

    trait NotClone {
        fn is_clone(&self) -> bool {
            false
        }
    }
    impl<T> NotClone for Probe<T> {}
    impl<T: Clone> Probe<T> {
        #[allow(dead_code)]
        fn is_clone(&self) -> bool {
            true
        }
    }

    trait NotSerialize {
        fn is_serialize(&self) -> bool {
            false
        }
    }
    impl<T> NotSerialize for Probe<T> {}
    impl<T: serde::Serialize> Probe<T> {
        #[allow(dead_code)]
        fn is_serialize(&self) -> bool {
            true
        }
    }

    #[test]
    fn the_probes_see_a_trait_that_is_implemented() {
        let probe = Probe::<String>(PhantomData);
        assert!(probe.is_debug());
        assert!(probe.is_display());
        assert!(probe.is_clone());
        assert!(probe.is_serialize());
    }

    #[test]
    fn no_formatting_or_copying_trait_is_implemented() {
        let secret = Probe::<Secret>(PhantomData);
        assert!(!secret.is_debug());
        assert!(!secret.is_display());
        assert!(!secret.is_clone());
        assert!(!secret.is_serialize());
        let message = Probe::<crate::greetd::GreetdMessage>(PhantomData);
        assert!(!message.is_debug());
        assert!(!message.is_display());
        assert!(!message.is_clone());
        assert!(!message.is_serialize());
    }

    #[test]
    fn wipe_zeroes_every_byte_of_the_allocation() {
        let mut secret = Secret::new();
        secret.push_str("correct horse").unwrap();
        assert!(secret.storage().iter().any(|byte| *byte != 0));
        secret.wipe();
        assert!(secret.is_empty());
        assert!(secret.storage().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn backspace_wipes_the_bytes_it_removes() {
        let mut secret = Secret::new();
        secret.push_str("ab").unwrap();
        secret.push_char('é').unwrap();
        assert_eq!(secret.char_count(), 3);
        assert!(secret.pop_char());
        assert_eq!(secret.expose_str(), "ab");
        assert!(secret.storage()[2..].iter().all(|byte| *byte == 0));
        assert!(secret.pop_char());
        assert!(secret.pop_char());
        assert!(!secret.pop_char());
        assert!(secret.storage().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_full_field_refuses_the_whole_character() {
        let mut secret = Secret::new();
        secret.push_str(&"a".repeat(SECRET_CAPACITY - 1)).unwrap();
        assert_eq!(secret.push_char('é'), Err(SecretFull));
        assert_eq!(secret.len(), SECRET_CAPACITY - 1);
        secret.push_char('a').unwrap();
        assert_eq!(secret.push_char('a'), Err(SecretFull));
        assert_eq!(secret.push_str("a"), Err(SecretFull));
        assert_eq!(secret.len(), SECRET_CAPACITY);
    }

    #[test]
    fn the_allocation_is_page_aligned_and_whole_pages() {
        let secret = Secret::new();
        let storage = secret.storage();
        let page = page_size().unwrap();
        assert_eq!(storage.as_ptr() as usize % page, 0);
        assert_eq!(storage.len() % page, 0);
        assert!(storage.len() >= SECRET_CAPACITY);
    }

    #[test]
    fn the_pages_are_locked_where_the_memory_lock_limit_allows_it() {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: getrlimit writes into the struct it is given.
        let read = unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut limit) } == 0;
        // Room for this test's page beside every other test thread's buffers.
        test_prerequisites::require_that!(
            "an RLIMIT_MEMLOCK of at least 1 MiB",
            read && limit.rlim_cur >= 1024 * 1024
        );
        let secret = Secret::new();
        assert!(secret.is_locked());
        assert_eq!(secret.lock_warning(), None);
    }

    #[test]
    fn a_page_size_that_is_not_one_is_an_error_not_a_guess() {
        assert_eq!(usable_page_size(-1), None);
        assert_eq!(usable_page_size(0), None);
        assert_eq!(usable_page_size(3000), None);
        assert_eq!(usable_page_size(4096), Some(4096));
        assert_eq!(usable_page_size(16384), Some(16384));
        assert_eq!(usable_page_size(65536), Some(65536));
    }

    #[test]
    fn the_lock_warning_names_no_content() {
        assert!(NOT_LOCKED_LOG_LINE.starts_with("BETTER_OS_SECRET_LOCKED=no"));
        let mut secret = Secret::new();
        secret.push_str("hunter2").unwrap();
        // Whatever the buffer holds, the line is the same constant.
        if let Some(line) = secret.lock_warning() {
            assert_eq!(line, NOT_LOCKED_LOG_LINE);
        }
        assert!(!NOT_LOCKED_LOG_LINE.contains("hunter2"));
    }
}
