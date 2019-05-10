// Copyright 2016 Amanieu d'Antras
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

// REVIEW: this module needs a doc comment going into more depth about how it's
// implementing the `ThreadParker` protocol.

use super::libstd::{thread, time::Instant};
use core::{
    ptr,
    sync::atomic::{AtomicI32, Ordering},
};
use libc;

// REVIEW: these constants should be added to libc, verified, and used from
// there
const FUTEX_WAIT: i32 = 0;
const FUTEX_WAKE: i32 = 1;
const FUTEX_PRIVATE: i32 = 128;

// x32 Linux uses a non-standard type for tv_nsec in timespec.
// See https://sourceware.org/bugzilla/show_bug.cgi?id=16437
//
// REVIEW: is this something that should be fixed in `libc`?
#[cfg(all(target_arch = "x86_64", target_pointer_width = "32"))]
#[allow(non_camel_case_types)]
type tv_nsec_t = i64;
#[cfg(not(all(target_arch = "x86_64", target_pointer_width = "32")))]
#[allow(non_camel_case_types)]
type tv_nsec_t = libc::c_long;

// Helper type for putting a thread to sleep until some other thread wakes it up
pub struct ThreadParker {
    // REVIEW: is it really worth it to gate this entire module on the existence
    // of `AtomicI32` rather than using `AtomicUsize`?
    futex: AtomicI32,
}

impl ThreadParker {
    pub const IS_CHEAP_TO_CONSTRUCT: bool = true;

    #[inline]
    pub fn new() -> ThreadParker {
        ThreadParker {
            futex: AtomicI32::new(0),
        }
    }

    // Prepares the parker. This should be called before adding it to the queue.
    #[inline]
    pub fn prepare_park(&self) {
        // REVIEW: I suspect this isn't the first time I'm going to run into
        // this, but non-`SeqCst` ordering makes me very uncomfortable. The
        // current policy of libstd is to use `SeqCst` everywhere. If profiling
        // shows that it's hot *and* there's proven code elsewhere (generally
        // C++) that has thought through the orderings, then orderings are
        // selectively changed away from `SeqCst`.
        //
        // Have spots like this really been profiles to show them as hot enough
        // to deserve non-`SeqCst` orderings?
        self.futex.store(1, Ordering::Relaxed);
    }

    // Checks if the park timed out. This should be called while holding the
    // queue lock after park_until has returned false.
    #[inline]
    pub fn timed_out(&self) -> bool {
        self.futex.load(Ordering::Relaxed) != 0
    }

    // Parks the thread until it is unparked. This should be called after it has
    // been added to the queue, after unlocking the queue.
    #[inline]
    pub fn park(&self) {
        while self.futex.load(Ordering::Acquire) != 0 {
            self.futex_wait(None);
        }
    }

    // Parks the thread until it is unparked or the timeout is reached. This
    // should be called after it has been added to the queue, after unlocking
    // the queue. Returns true if we were unparked and false if we timed out.
    #[inline]
    pub fn park_until(&self, timeout: Instant) -> bool {
        while self.futex.load(Ordering::Acquire) != 0 {
            let now = Instant::now();
            if timeout <= now {
                return false;
            }
            let diff = timeout - now;
            if diff.as_secs() as libc::time_t as u64 != diff.as_secs() {
                // Timeout overflowed, just sleep indefinitely
                // REVIEW: elsewhere in libstd when we encounter this situation
                // we simply loop until the timeout elapses, could that be done
                // here instead of parking indefinitely? It's a bit of a moot
                // point in the sense that indefinitely vs sleeping for years
                // isn't really that different, but it's probably good to be
                // consistent.
                self.park();
                return true;
            }
            let ts = libc::timespec {
                tv_sec: diff.as_secs() as libc::time_t,
                tv_nsec: diff.subsec_nanos() as tv_nsec_t,
            };
            self.futex_wait(Some(ts));
        }
        true
    }

    #[inline]
    fn futex_wait(&self, ts: Option<libc::timespec>) {
        let ts_ptr = ts
            .as_ref()
            .map(|ts_ref| ts_ref as *const _)
            .unwrap_or(ptr::null());
        let r = unsafe {
            libc::syscall(
                libc::SYS_futex,
                &self.futex,
                FUTEX_WAIT | FUTEX_PRIVATE,
                1,
                ts_ptr,
            )
        };
        debug_assert!(r == 0 || r == -1);
        if r == -1 {
            unsafe {
                // REVIEW: instead of `libc::__errno_location` can this use
                // `io::Error::last_os_error`?
                debug_assert!(
                    *libc::__errno_location() == libc::EINTR
                        || *libc::__errno_location() == libc::EAGAIN
                        || (ts.is_some() && *libc::__errno_location() == libc::ETIMEDOUT)
                );

                // REVIEW: what's the platform compatibility of the futex
                // syscall? Does it fit libstd's platform compatibility?
            }
        }
    }

    // Locks the parker to prevent the target thread from exiting. This is
    // necessary to ensure that thread-local ThreadData objects remain valid.
    // This should be called while holding the queue lock.
    #[inline]
    pub fn unpark_lock(&self) -> UnparkHandle {
        // We don't need to lock anything, just clear the state
        self.futex.store(0, Ordering::Release);

        UnparkHandle { futex: &self.futex }
    }
}

// Handle for a thread that is about to be unparked. We need to mark the thread
// as unparked while holding the queue lock, but we delay the actual unparking
// until after the queue lock is released.
pub struct UnparkHandle {
    futex: *const AtomicI32,
}

impl UnparkHandle {
    // Wakes up the parked thread. This should be called after the queue lock is
    // released to avoid blocking the queue for too long.
    #[inline]
    pub fn unpark(self) {
        // The thread data may have been freed at this point, but it doesn't
        // matter since the syscall will just return EFAULT in that case.
        let r =
            unsafe { libc::syscall(libc::SYS_futex, self.futex, FUTEX_WAKE | FUTEX_PRIVATE, 1) };
        debug_assert!(r == 0 || r == 1 || r == -1);
        if r == -1 {
            debug_assert_eq!(unsafe { *libc::__errno_location() }, libc::EFAULT);
        }
    }
}

#[inline]
pub fn thread_yield() {
    thread::yield_now();
}
