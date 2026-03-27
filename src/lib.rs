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
/// Manages `DUCKS` buffers of size `DUCK_SIZE` bytes each.
/// Provides thread-safe allocation and deallocation via atomic flags.
pub struct Badewanne<const DUCK_SIZE: usize, const DUCKS: usize> {
    ducks: [UnsafeCell<[u8; DUCK_SIZE]>; DUCKS],
    swimming: [AtomicBool; DUCKS],
}

impl<const DUCK_SIZE: usize, const DUCKS: usize> Badewanne<DUCK_SIZE, DUCKS> {
    /// Creates a new pool with all buffers initially available.
    pub fn new() -> Self {
        Self {
            ducks: array::from_fn(|_| UnsafeCell::new([0u8; DUCK_SIZE])),
            swimming: array::from_fn(|_| AtomicBool::new(true)),
        }
    }

    /// Attempts to allocate a buffer from the pool.
    ///
    /// Returns `Some(Duck)` if a buffer is available, `None` if all buffers are in use.
    /// The returned `Duck` automatically releases the buffer when dropped.
    pub fn try_grab_duck(&self) -> Option<Duck<'_, DUCK_SIZE, DUCKS>> {
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

impl<const DUCK_SIZE: usize, const DUCKS: usize> Default for Badewanne<DUCK_SIZE, DUCKS> {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to an allocated buffer from the pool.
///
/// Provides exclusive mutable access to a `DUCK_SIZE` byte buffer.
/// Automatically returns the buffer to the pool when dropped.
pub struct Duck<'a, const DUCK_SIZE: usize, const DUCKS: usize> {
    wanne: &'a Badewanne<DUCK_SIZE, DUCKS>,
    duck_idx: usize,
}

impl<'a, const DUCK_SIZE: usize, const DUCKS: usize> Drop for Duck<'a, DUCK_SIZE, DUCKS> {
    fn drop(&mut self) {
        // SAFETY: A Duck can only be obtained from the Badewanne so it is safe to index directly
        self.wanne.swimming[self.duck_idx].store(true, Ordering::Release);
    }
}

impl<'a, const DUCK_SIZE: usize, const DUCKS: usize> Duck<'a, DUCK_SIZE, DUCKS> {
    fn new(wanne: &'a Badewanne<DUCK_SIZE, DUCKS>, slot: usize) -> Self {
        Self {
            wanne,
            duck_idx: slot,
        }
    }

    fn as_slice(&self) -> &[u8] {
        // SAFETY: Each Duck has exclusive access to its slot via the atomic flag
        // in the Badewanne's swimming array, so no other Duck or thread can access
        // this slot simultaneously.
        unsafe { &*self.wanne.ducks[self.duck_idx].get() }
    }

    fn as_slice_mut(&mut self) -> &mut [u8] {
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
unsafe impl<const DUCK_SIZE: usize, const DUCKS: usize> Sync for Badewanne<DUCK_SIZE, DUCKS> {}

// SAFETY: Badewanne can be sent between threads because:
// 1. It only contains Send types (UnsafeCell and AtomicBool)
// 2. No thread-specific data or resources
unsafe impl<const DUCK_SIZE: usize, const DUCKS: usize> Send for Badewanne<DUCK_SIZE, DUCKS> {}

// SAFETY: Duck can be sent between threads because:
// 1. It contains a reference to Badewanne (which is Sync)
// 2. The Duck has exclusive access to its slot via atomic flag
// 3. No thread-specific data or resources
unsafe impl<'a, const DUCK_SIZE: usize, const DUCKS: usize> Send for Duck<'a, DUCK_SIZE, DUCKS> {}

impl<'a, const DUCK_SIZE: usize, const DUCKS: usize> Deref for Duck<'a, DUCK_SIZE, DUCKS> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'a, const DUCK_SIZE: usize, const DUCKS: usize> DerefMut for Duck<'a, DUCK_SIZE, DUCKS> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_slice_mut()
    }
}

impl<'a, const DUCK_SIZE: usize, const DUCKS: usize> AsRef<[u8]> for Duck<'a, DUCK_SIZE, DUCKS> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<'a, const DUCK_SIZE: usize, const DUCKS: usize> AsMut<[u8]> for Duck<'a, DUCK_SIZE, DUCKS> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_slice_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api() {
        let wanne = Badewanne::<256, 8>::new();
        let mut duck0 = wanne.try_grab_duck().unwrap();
        let duck1 = wanne.try_grab_duck().unwrap();
        duck0[1] = 1; // Can use [] operator via DerefMut
        drop(duck1);
        duck0[0] = 1;
    }

    #[test]
    fn multi_threaded() {
        let wanne = Badewanne::<256, 8>::new();

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
        let wanne = Badewanne::<256, 8>::new();
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
}
