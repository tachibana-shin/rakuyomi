//! Compatibility shims for Android API < 21.
//!
//! The armv7 build targets API 18, but some dependencies (quickjs's
//! `js_cond_init`, rustix's `futimens`) reference bionic symbols that were
//! only added in API 21. These no-op equivalents let the API 18 build link;
//! they are never exercised at runtime on supported devices.
//!
//! * `futimens` — bionic itself implements it via `utimensat` (available
//!   since API 10), so delegate to that.
//! * `pthread_condattr_setclock` — only reachable through quickjs's
//!   `Atomics.wait` (requires worker threads, which RakuYomi never uses),
//!   so pretend success and leave the default (realtime) clock.
#![cfg(all(target_os = "android", feature = "api_18"))]

use std::os::raw::{c_int, c_void};

/// Sets a file descriptor's timestamps, like bionic's `futimens` (API 21+).
///
/// Equivalent to `utimensat(fd, NULL, times, 0)` — the same implementation
/// bionic uses.
#[no_mangle]
#[used]
pub unsafe extern "C" fn futimens(fd: c_int, times: *const libc::timespec) -> c_int {
    unsafe { libc::utimensat(fd, std::ptr::null(), times, 0) }
}

/// Sets the clock used by a `pthread_condattr_t` (API 21+).
///
/// Bionic's implementation just stores the clock id; our no-op leaves the
/// default (realtime) clock, which is fine since the only caller (quickjs's
/// `Atomics.wait`) is never exercised by RakuYomi.
#[no_mangle]
#[used]
pub unsafe extern "C" fn pthread_condattr_setclock(_attr: *mut c_void, _clock_id: c_int) -> c_int {
    0
}
