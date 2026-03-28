use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ops::Deref;
use core::ops::DerefMut;
use core::ptr::NonNull;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

pub struct Badewanne<T, const SIZE: usize> {
    ducks: UnsafeCell<MaybeUninit<T>>,
    swimming: AtomicBool,
}

unsafe impl<T: Send, const SIZE: usize> Send for Badewanne<T, SIZE> {}
unsafe impl<T: Send, const SIZE: usize> Sync for Badewanne<T, SIZE> {}

impl<T, const SIZE: usize> Badewanne<T, SIZE> {
    pub fn new() -> Self {
        Self {
            ducks: UnsafeCell::new(MaybeUninit::uninit()),
            swimming: AtomicBool::new(true),
        }
    }

    fn grab_duck(&self) -> Option<NonNull<MaybeUninit<T>>> {
        if self
            .swimming
            .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return Some(unsafe { NonNull::new_unchecked(self.ducks.get()) });
        }
        None
    }
}

impl<T, const SIZE: usize> Default for Badewanne<T, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Duck<'wanne, T, const SIZE: usize> {
    wanne: &'wanne Badewanne<T, SIZE>,
    duck: NonNull<T>,
}

unsafe impl<'wanne, T: Send, const SIZE: usize> Send for Duck<'wanne, T, SIZE> {}
unsafe impl<'wanne, T: Sync, const SIZE: usize> Sync for Duck<'wanne, T, SIZE> {}

impl<'wanne, T, const SIZE: usize> Duck<'wanne, T, SIZE> {
    pub fn new_in(x: T, wanne: &'wanne Badewanne<T, SIZE>) -> Option<Self> {
        wanne.grab_duck().map(|mut ptr| {
            unsafe { ptr.as_mut().write(x) };
            Self {
                wanne,
                duck: ptr.cast::<T>(),
            }
        })
    }
}

impl<'wanne, T, const SIZE: usize> AsMut<T> for Duck<'wanne, T, SIZE> {
    fn as_mut(&mut self) -> &mut T {
        self
    }
}

impl<'wanne, T, const SIZE: usize> AsRef<T> for Duck<'wanne, T, SIZE> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<'wanne, T, const SIZE: usize> Deref for Duck<'wanne, T, SIZE> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.duck.as_ref() }
    }
}

impl<'wanne, T, const SIZE: usize> DerefMut for Duck<'wanne, T, SIZE> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.duck.as_mut() }
    }
}

impl<'wanne, T, const SIZE: usize> Drop for Duck<'wanne, T, SIZE> {
    fn drop(&mut self) {
        unsafe {
            core::ptr::drop_in_place(self.duck.as_ptr());
        }
        self.wanne.swimming.store(true, Ordering::Release);
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
    fn api() {
        struct MyType {
            value: i32,
            drop_cnt: Arc<AtomicUsize>,
        }

        impl MyType {
            fn new(drop_cnt: Arc<AtomicUsize>) -> Self {
                Self { value: 0, drop_cnt }
            }

            fn set(&mut self, value: i32) {
                self.value = value;
            }
        }

        impl Drop for MyType {
            fn drop(&mut self) {
                self.drop_cnt.fetch_add(1, Ordering::Relaxed);
            }
        }

        let drop_cnt = Arc::new(AtomicUsize::new(0));

        let wanne = Arc::new(Badewanne::<MyType, 1>::new());
        let mut duck = Duck::new_in(MyType::new(drop_cnt.clone()), &wanne).unwrap();
        duck.set(1);
        drop(duck);

        assert_eq!(drop_cnt.load(Ordering::Relaxed), 1);

        std::thread::spawn({
            let wanne = wanne.clone();
            let drop_cnt = drop_cnt.clone();
            move || {
                let mut duck = Duck::new_in(MyType::new(drop_cnt), &wanne).unwrap();
                duck.set(2);
            }
        })
        .join()
        .unwrap();

        assert_eq!(drop_cnt.load(Ordering::Relaxed), 2);
    }
}
