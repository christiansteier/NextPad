//! Sperren, die nach einem Absturz unter der Sperre weiterlaufen.
//!
//! Stürzt ein Handler ab, während er eine Sperre hält, gilt sie als vergiftet. Der Server
//! arbeitet mit dem Inhalt weiter, statt bei jedem späteren Zugriff selbst abzustürzen, und
//! schreibt die Stelle ins Protokoll, damit der Fehler nicht unbemerkt bleibt.
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[track_caller]
fn vergiftet<G>(e: PoisonError<G>) -> G {
    tracing::error!("Sperre nach einem Absturz vergiftet, arbeite mit dem Inhalt weiter ({})", std::panic::Location::caller());
    e.into_inner()
}

pub trait Sperre<T> {
    fn sperren(&self) -> MutexGuard<'_, T>;
}

impl<T> Sperre<T> for Mutex<T> {
    #[track_caller]
    fn sperren(&self) -> MutexGuard<'_, T> { self.lock().unwrap_or_else(vergiftet) }
}

pub trait LeseSperre<T> {
    fn lesen(&self) -> RwLockReadGuard<'_, T>;
    fn schreiben(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T> LeseSperre<T> for RwLock<T> {
    #[track_caller]
    fn lesen(&self) -> RwLockReadGuard<'_, T> { self.read().unwrap_or_else(vergiftet) }
    #[track_caller]
    fn schreiben(&self) -> RwLockWriteGuard<'_, T> { self.write().unwrap_or_else(vergiftet) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vergiftete_sperre_liefert_den_inhalt() {
        let m = std::sync::Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || { let _g = m2.lock().unwrap(); panic!("Absturz unter der Sperre") }).join();
        assert!(m.is_poisoned());
        *m.sperren() += 1;
        assert_eq!(*m.sperren(), 2);
        let r = std::sync::Arc::new(RwLock::new(5));
        let r2 = r.clone();
        let _ = std::thread::spawn(move || { let _g = r2.write().unwrap(); panic!("Absturz unter der Sperre") }).join();
        assert_eq!(*r.lesen(), 5);
        *r.schreiben() = 6;
        assert_eq!(*r.lesen(), 6);
    }
}
