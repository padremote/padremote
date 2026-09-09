//! Locking that survives a panic.
//!
//! `Mutex::lock().unwrap()` is fine in a test and wrong in a daemon. If any
//! thread ever panics while holding one of these locks, the mutex is *poisoned*
//! and every later `unwrap()` panics too - so a single bug in one gesture would
//! take the app permanently deaf while the menu-bar icon still claimed it was
//! running, and only a quit-and-relaunch would fix it.
//!
//! Poisoning is the right default for data that must not be read half-written.
//! It is the wrong default here: the guarded state is a gesture recognizer, a
//! device list and an input backend, all of which are *rebuilt* by the next
//! touch or the next connection. Carrying on with whatever is in there beats
//! refusing to work at all, so a poisoned lock is taken anyway and reported
//! once, loudly enough to be found in a log.

use std::sync::{Mutex, MutexGuard};

pub trait MutexExt<T> {
    /// Lock, taking the value even if a previous holder panicked.
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                // Worth saying: the panic that poisoned it happened somewhere
                // else and may well have been swallowed by the task that hit it.
                tracing::error!(
                    "recovered a poisoned lock - something panicked earlier; \
                     gestures may have been dropped"
                );
                poisoned.into_inner()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_poisoned_lock_is_still_usable() {
        let m = Arc::new(Mutex::new(41));
        let victim = m.clone();
        // Poison it: panic with the guard held.
        let _ = std::thread::spawn(move || {
            let _guard = victim.lock().unwrap();
            panic!("boom");
        })
        .join();

        assert!(m.lock().is_err(), "the lock really is poisoned");
        *m.locked() += 1;
        assert_eq!(*m.locked(), 42, "and work continues through it");
    }
}
