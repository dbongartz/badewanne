#![cfg_attr(not(test), no_std)]

use core::{
    array,
    borrow::Borrow,
    cell::UnsafeCell,
    fmt,
    hash::{Hash, Hasher},
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Badewanne<T, const SIZE: usize> {
    ducks: [UnsafeCell<MaybeUninit<T>>; SIZE],
    swimming: [AtomicBool; SIZE],
}

impl<T, const SIZE: usize> Badewanne<T, SIZE> {
    pub fn new() -> Self {
        Self {
            ducks: array::from_fn(|_| UnsafeCell::new(MaybeUninit::uninit())),
            swimming: array::from_fn(|_| AtomicBool::new(true)),
        }
    }

    fn grab_duck(&self) -> Option<(NonNull<MaybeUninit<T>>, &AtomicBool)> {
        self.swimming
            .iter()
            .zip(self.ducks.iter())
            .find_map(|(flag, cell)| {
                flag.compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
                    .ok()
                    // SAFETY: UnsafeCell::get() is never null.
                    .map(|_| (unsafe { NonNull::new_unchecked(cell.get()) }, flag))
            })
    }
}

// SAFETY: Badewanne owns T values inside UnsafeCell. Moving it across threads
// moves those T values, and sharing it across threads allows acquiring Ducks
// that give &mut T — both require T: Send.
unsafe impl<T: Send, const SIZE: usize> Send for Badewanne<T, SIZE> {}
unsafe impl<T: Send, const SIZE: usize> Sync for Badewanne<T, SIZE> {}

impl<T, const SIZE: usize> Default for Badewanne<T, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Duck<'a, T> {
    duck: NonNull<T>,
    slot: &'a AtomicBool,
}

impl<'a, T> Duck<'a, T> {
    pub fn new_in<const SIZE: usize>(x: T, wanne: &'a Badewanne<T, SIZE>) -> Option<Self> {
        wanne.grab_duck().map(|(mut ptr, slot)| {
            // SAFETY: We have exclusive access to this slot via the atomic flag.
            unsafe { ptr.as_mut().write(x) };
            Self {
                duck: ptr.cast::<T>(),
                slot,
            }
        })
    }
}

impl<T> Deref for Duck<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: duck points to an initialized T and we hold shared access (&self).
        // No mutable alias can exist simultaneously (DerefMut requires &mut self).
        unsafe { self.duck.as_ref() }
    }
}

impl<T> DerefMut for Duck<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: duck points to an initialized T and we hold exclusive access (&mut self).
        unsafe { self.duck.as_mut() }
    }
}

impl<T> AsRef<T> for Duck<'_, T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T> AsMut<T> for Duck<'_, T> {
    fn as_mut(&mut self) -> &mut T {
        self
    }
}

impl<T: fmt::Debug> fmt::Debug for Duck<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: fmt::Display> fmt::Display for Duck<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: fmt::Pointer> fmt::Pointer for Duck<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&**self, f)
    }
}

impl<T: PartialEq> PartialEq for Duck<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Hash> Hash for Duck<'_, T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl<T> Borrow<T> for Duck<'_, T> {
    fn borrow(&self) -> &T {
        self
    }
}

// SAFETY: Sending a Duck sends exclusive (&mut T) access to another thread:
// requires T: Send. Sharing &Duck exposes &T via Deref: requires T: Sync.
unsafe impl<'a, T: Send> Send for Duck<'a, T> {}
unsafe impl<'a, T: Sync> Sync for Duck<'a, T> {}

impl<T> Drop for Duck<'_, T> {
    fn drop(&mut self) {
        // SAFETY: duck was initialized in new_in and this Drop runs exactly once.
        unsafe { core::ptr::drop_in_place(self.duck.as_ptr()) };
        self.slot.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[test]
    fn multi_slot() {
        let wanne = Badewanne::<u32, 4>::new();

        let d0 = Duck::new_in(0u32, &wanne).unwrap();
        let d1 = Duck::new_in(1u32, &wanne).unwrap();
        let d2 = Duck::new_in(2u32, &wanne).unwrap();
        let d3 = Duck::new_in(3u32, &wanne).unwrap();

        // Pool is full — next grab must fail
        assert!(Duck::new_in(99u32, &wanne).is_none());

        // Dropping one slot makes it available again
        drop(d2);
        assert!(Duck::new_in(99u32, &wanne).is_some());

        drop(d0);
        drop(d1);
        drop(d3);
    }

    #[test]
    fn multi_threaded() {
        let wanne = Badewanne::<u32, 8>::new();

        std::thread::scope(|s| {
            for i in 0..8 {
                s.spawn({
                    let wanne = &wanne;
                    move || {
                        let duck = Duck::new_in(i as u32, wanne).unwrap();
                        assert_eq!(*duck, i as u32);
                    }
                });
            }
        });
    }

    #[test]
    fn drop_calls_destructor() {
        let drop_cnt = Arc::new(AtomicUsize::new(0));

        struct Droppable(Arc<AtomicUsize>);
        impl Drop for Droppable {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let wanne = Badewanne::<Droppable, 2>::new();
        let d0 = Duck::new_in(Droppable(drop_cnt.clone()), &wanne).unwrap();
        let d1 = Duck::new_in(Droppable(drop_cnt.clone()), &wanne).unwrap();

        assert_eq!(drop_cnt.load(Ordering::Relaxed), 0);
        drop(d0);
        assert_eq!(
            drop_cnt.load(Ordering::Relaxed),
            1,
            "destructor must run on Duck drop"
        );
        drop(d1);
        assert_eq!(
            drop_cnt.load(Ordering::Relaxed),
            2,
            "destructor must run on Duck drop"
        );

        // Both slots were returned — pool must be full again
        let _da = Duck::new_in(Droppable(drop_cnt.clone()), &wanne).unwrap();
        let _db = Duck::new_in(Droppable(drop_cnt.clone()), &wanne).unwrap();
        assert!(Duck::new_in(Droppable(drop_cnt.clone()), &wanne).is_none());
    }

    #[test]
    fn drop_pool_destroys_no_uninit_slots() {
        // When the pool drops, it must not run T's destructor on uninit slots.
        let drop_cnt = Arc::new(AtomicUsize::new(0));

        struct Droppable(Arc<AtomicUsize>);
        impl Drop for Droppable {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        {
            let wanne = Badewanne::<Droppable, 4>::new();
            let d = Duck::new_in(Droppable(drop_cnt.clone()), &wanne).unwrap();
            drop(d); // destructor runs here, slot goes back to uninit
            // wanne drops here with all slots uninit — must not call destructor again
        }

        assert_eq!(
            drop_cnt.load(Ordering::Relaxed),
            1,
            "destructor must run exactly once"
        );
    }
}
