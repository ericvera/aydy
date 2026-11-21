//! Locking shared state so that one panic cannot stop everything that shares it.

use std::sync::{Mutex, MutexGuard};

/// Locks `value`, recovering from a mutex poisoned by an earlier panic.
///
/// A panic anywhere near the registry must not stop session tracking, and a
/// poisoned lock left unrecovered would stop it permanently.
pub fn lock_recovering<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn a_mutex_poisoned_by_a_panicking_thread_still_yields_its_value() {
        let shared = Arc::new(Mutex::new(7));
        let poisoner = Arc::clone(&shared);
        std::thread::spawn(move || {
            let mut guard = lock_recovering(&poisoner);
            *guard = 8;
            panic!("poisoning the lock");
        })
        .join()
        .expect_err("the poisoning thread to have panicked");

        assert!(shared.is_poisoned());
        assert_eq!(*lock_recovering(&shared), 8);
    }
}
