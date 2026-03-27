#![cfg_attr(not(test), no_std)]

use core::{
    array,
    cell::UnsafeCell,
    convert::{AsMut, AsRef},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

/// Thread-safe fixed-size object pool for byte buffers.
///
/// Manages `DUCKS` buffers of size `T` bytes each.
/// Provides thread-safe allocation and deallocation via atomic flags.
pub struct Badewanne<T, const DUCKS: usize> {
    ducks: [UnsafeCell<T>; DUCKS],
    swimming: [AtomicBool; DUCKS],
}

impl<T, const DUCKS: usize> Badewanne<T, DUCKS> {
    fn new_internal(ducks: [UnsafeCell<T>; DUCKS]) -> Self {
        Self {
            ducks,
            swimming: array::from_fn(|_| AtomicBool::new(true)),
        }
    }

    pub fn from_fn<F: FnMut(usize) -> T>(mut f: F) -> Self {
        let ducks = array::from_fn(|i| UnsafeCell::new(f(i)));
        Self::new_internal(ducks)
    }
}

impl<T: Clone, const DUCKS: usize> Badewanne<T, DUCKS> {
    pub fn with_init(initial: T) -> Self {
        let ducks = array::from_fn(|_| UnsafeCell::new(initial.clone()));
        Self::new_internal(ducks)
    }
}

impl<T: Default, const DUCKS: usize> Badewanne<T, DUCKS> {
    /// Creates a new pool with all buffers initially available.
    pub fn new() -> Self {
        let ducks = array::from_fn(|_| UnsafeCell::new(Default::default()));
        Self::new_internal(ducks)
    }
}

// impl<T, const DUCKS: usize> Badewanne<T, DUCKS>
// where
//     T: Copy, // for safety
// {
//     pub unsafe fn new_uninit() -> Self {
//         Self {
//             ducks: array::from_fn(|_| UnsafeCell::new(unsafe { core::mem::zeroed() })),
//             swimming: array::from_fn(|_| AtomicBool::new(true)),
//         }
//     }
// }

impl<T, const DUCKS: usize> Badewanne<T, DUCKS> {
    /// Attempts to allocate a buffer from the pool.
    ///
    /// Returns `Some(Duck)` if a buffer is available, `None` if all buffers are in use.
    /// The returned `Duck` automatically releases the buffer when dropped.
    pub fn try_grab_duck(&self) -> Option<Duck<'_, T, DUCKS>> {
        self.swimming
            .iter()
            .enumerate()
            .find_map(|(slot_idx, used)| {
                used.compare_exchange(true, false, Ordering::Acquire, Ordering::Acquire)
                    .ok()
                    .map(|_| Duck::new(self, slot_idx))
            })
    }
}

impl<T: Default, const DUCKS: usize> Default for Badewanne<T, DUCKS> {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to an allocated buffer from the pool.
///
/// Provides exclusive mutable access to a `T`.
/// Automatically returns the buffer to the pool when dropped.
pub struct Duck<'a, T, const DUCKS: usize> {
    wanne: &'a Badewanne<T, DUCKS>,
    duck_idx: usize,
}

impl<'a, T, const DUCKS: usize> Drop for Duck<'a, T, DUCKS> {
    fn drop(&mut self) {
        // SAFETY: A Duck can only be obtained from the Badewanne so it is safe to index directly
        self.wanne.swimming[self.duck_idx].store(true, Ordering::Release);
    }
}

impl<'a, T, const DUCKS: usize> Duck<'a, T, DUCKS> {
    fn new(wanne: &'a Badewanne<T, DUCKS>, slot: usize) -> Self {
        Self {
            wanne,
            duck_idx: slot,
        }
    }

    fn as_ref(&self) -> &T {
        // SAFETY: Each Duck has exclusive access to its slot via the atomic flag
        // in the Badewanne's swimming array, so no other Duck or thread can access
        // this slot simultaneously.
        unsafe { &*self.wanne.ducks[self.duck_idx].get() }
    }

    fn as_ref_mut(&mut self) -> &mut T {
        // SAFETY: Each Duck has exclusive access to its slot via the atomic flag
        // in the Badewanne's swimming array, so no other Duck or thread can access
        // this slot simultaneously.
        unsafe { &mut *self.wanne.ducks[self.duck_idx].get() }
    }
}

// SAFETY: Badewanne can be shared between threads because:
// 1. The AtomicBool in `used` array provides synchronization
// 2. Each Duck has exclusive access to its slot via the atomic flag
// 3. The UnsafeCell slots are only accessed through the synchronized `used` flags
unsafe impl<T, const DUCKS: usize> Sync for Badewanne<T, DUCKS> {}

// SAFETY: Badewanne can be sent between threads because:
// 1. It only contains Send types (UnsafeCell and AtomicBool)
// 2. No thread-specific data or resources
unsafe impl<T, const DUCKS: usize> Send for Badewanne<T, DUCKS> {}

// SAFETY: Duck can be sent between threads because:
// 1. It contains a reference to Badewanne (which is Sync)
// 2. The Duck has exclusive access to its slot via atomic flag
// 3. No thread-specific data or resources
unsafe impl<'a, T, const DUCKS: usize> Send for Duck<'a, T, DUCKS> {}

impl<'a, T, const DUCKS: usize> Deref for Duck<'a, T, DUCKS> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<'a, T, const DUCKS: usize> DerefMut for Duck<'a, T, DUCKS> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_ref_mut()
    }
}

impl<'a, T, const DUCKS: usize> AsRef<T> for Duck<'a, T, DUCKS> {
    fn as_ref(&self) -> &T {
        self.as_ref()
    }
}

impl<'a, T, const DUCKS: usize> AsMut<T> for Duck<'a, T, DUCKS> {
    fn as_mut(&mut self) -> &mut T {
        self.as_ref_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api() {
        let wanne = Badewanne::<[u8; 256], 8>::with_init([0u8; 256]);
        let mut duck0 = wanne.try_grab_duck().unwrap();
        let duck1 = wanne.try_grab_duck().unwrap();
        duck0[1] = 1; // Can use [] operator via DerefMut
        drop(duck1);
        duck0[0] = 1;
    }

    #[test]
    fn multi_threaded() {
        let wanne = Badewanne::<[u8; 256], 8>::from_fn(|_| [0u8; 256]);

        std::thread::scope(|s| {
            for i in 0..8 {
                s.spawn({
                    let wanne = &wanne;
                    move || {
                        let mut duck = wanne.try_grab_duck().unwrap();
                        duck[0] = i as u8; // Can use [] operator thanks to DerefMut
                    }
                });
            }
        });
    }

    #[test]
    fn deref_traits() {
        let wanne = Badewanne::<[u8; 256], 8>::with_init([0u8; 256]);
        let mut duck = wanne.try_grab_duck().unwrap();

        // Can use slice methods directly via Deref
        assert_eq!(duck.len(), 256);
        assert!(!duck.is_empty());

        // Can use [] operator via DerefMut
        duck[0] = 42;
        assert_eq!(duck[0], 42);

        // Can use AsRef/AsMut
        let _: &[u8] = duck.as_ref();
        let _: &mut [u8] = duck.as_mut();
    }

    #[test]
    fn generic_type() {
        #[derive(Debug, Default, Clone, PartialEq, Eq)]
        struct MyStruct {
            value: i32,
            name: &'static str,
        }

        // Test new() constructor
        let pool = Badewanne::<MyStruct, 4>::new();
        let duck = pool.try_grab_duck().unwrap();
        assert_eq!(*duck, MyStruct { value: 0, name: "" });

        // Test with_init() constructor
        let pool: Badewanne<MyStruct, 1> = Badewanne::with_init(MyStruct { value: 42, name: "test" });
        let mut duck = pool.try_grab_duck().unwrap();
        assert_eq!(*duck, MyStruct { value: 42, name: "test" });
        duck.value = 100;
        drop(duck);

        // Verify slot was returned
        let duck = pool.try_grab_duck().unwrap();
        assert_eq!(duck.value, 100);

        // Test from_fn() constructor
        let pool: Badewanne<MyStruct, 4> = Badewanne::from_fn(|i| MyStruct { value: i as i32, name: "fn" });
        for _ in 0..4 {
            let duck = pool.try_grab_duck().unwrap();
            assert_eq!(duck.name, "fn");
            drop(duck);
        }
    }
}
