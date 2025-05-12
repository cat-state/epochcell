use std::{
    cell::{Cell, UnsafeCell},
    ops::{Deref, DerefMut},
};

pub struct EpochCell<T> {
    epoch: Cell<u32>,
    val: UnsafeCell<T>,
}

pub struct RefMut<'a, T> {
    ptr: *mut T,
    epoch: &'a Cell<u32>,
    mark: u32,
}

pub struct Ref<'a, T>(RefMut<'a, T>);

impl<T> EpochCell<T> {
    pub fn new(val: T) -> Self {
        EpochCell {
            epoch: Cell::new(0u32),
            val: UnsafeCell::new(val),
        }
    }

    pub fn borrow(&self) -> Ref<'_, T> {
        let cur = self.epoch.get();
        Ref(RefMut {
            ptr: self.val.get(),
            epoch: &self.epoch,
            mark: cur,
        })
    }

    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        let cur = self.epoch.get();
        self.epoch.set(cur + 1);
        RefMut {
            ptr: self.val.get(),
            epoch: &self.epoch,
            mark: cur,
        }
    }


    pub fn get_mut(&mut self) -> &'_ mut T {
        self.val.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.val.into_inner()
    }
}

impl<'a, T> Drop for RefMut<'a, T> {
    fn drop(&mut self) {
        if self.epoch.get() == self.mark + 1 {
            self.epoch.set(self.mark);
        }
    }
}

impl<'a, T> Deref for RefMut<'a, T> {
    type Target = T;
    #[track_caller]
    fn deref(&self) -> &T {
        assert_eq!(self.epoch.get(), self.mark + 1, "stale borrow");
        unsafe { self.ptr.as_ref().expect("nullptr") }
    }
}

impl<'a, T> DerefMut for RefMut<'a, T> {
    #[track_caller]
    fn deref_mut(&mut self) -> &mut T {
        assert_eq!(self.epoch.get(), self.mark + 1, "stale mut borrow");
        unsafe { self.ptr.as_mut().expect("nullptr") }
    }
}

impl<'a, T> Deref for Ref<'a, T> {
    type Target = T;
    #[track_caller]
    fn deref(&self) -> &T {
        self.0.deref()
    }
}

fn main() {
    println!("Hello, world!");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let c = EpochCell::new(0u32);
        {
            let mut a = c.borrow_mut();
            {
                let mut b = c.borrow_mut();
                *b = 2;
            }
            *a = 1;
        }
        assert_eq!(c.into_inner(), 1);
    }


    #[test]
    fn test_get_mut() {
        let mut c = EpochCell::new(0u32);
        {
            let a = c.get_mut();
            *a = 1;
        }
        assert_eq!(c.into_inner(), 1);
    }


    #[test]
    #[should_panic]
    fn test_refcell_panics() {
        let c = std::cell::RefCell::new(0u32);
        {
            let mut a = c.borrow_mut();
            {
                let mut b = c.borrow_mut();
                *b = 2;
            }
            *a = 1;
        }
        assert_eq!(c.into_inner(), 1);
    }

    #[test]
    #[should_panic]
    fn test_stale() {
        let c = EpochCell::new(0u32);
        {
            let a = c.borrow_mut();
            let _b = c.borrow_mut();
            let _ = *a; // panic from stale deref
        }
    }

    #[test]
    fn test_latest() {
        let c = EpochCell::new(0u32);
        {
            {
                let _a = c.borrow_mut();
            }
            let b = c.borrow_mut();
            let _ = *b;
        }
    }

    #[test]
    fn test_forget_outer() {
        let c = EpochCell::new(0u32);
        {
            let a = c.borrow_mut();
            std::mem::forget(a);
            let mut b = c.borrow_mut();
            *b = 1;
        }
        assert_eq!(c.into_inner(), 1);
    }

    #[test]
    fn test_vec() {
        let c = EpochCell::new(vec![0u32]);
        {
            let mut a = c.borrow_mut();
            a.push(1);
            {
                let mut b = c.borrow_mut();
                b.pop();
                b.push(2);
            }
            a.push(3);
        }
        assert_eq!(c.into_inner(), vec![0u32, 2, 3]);
    }

    /* 1. deep recursion pushes/pops 1 000 times */
    fn recurse(cell: &EpochCell<u32>, depth: u32) {
        if depth == 0 {
            return;
        }
        let mut g = cell.borrow_mut();
        *g += 1;
        recurse(cell, depth - 1);
        *g += 1;
    }
    #[test]
    fn deep_recursion() {
        let c = EpochCell::new(0);
        recurse(&c, 1_000);
        assert_eq!(c.into_inner(), 2_000);
    }

    /* 2. guards dropped out‑of‑stack order via Vec::swap_remove */
    #[test]
    fn vector_shuffle_drop() {
        let c = EpochCell::new(0u8);
        {
            let mut v = Vec::new();
            for _ in 0..4 {
                v.push(c.borrow_mut());
            }
            while let Some(mut g) = if v.len() > 0 {
                Some(v.swap_remove(v.len() - 1))
            } else {
                None
            } {
                *g = *g + 1;
            }
        }

        assert_eq!(c.into_inner(), 4);
    }

    /* 3. unwind in inner scope restores epoch */
    #[test]
    fn unwind_restores() {
        let c = EpochCell::new(0u8);
        {
            let mut outer = c.borrow_mut();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _inner = c.borrow_mut(); // pushes
                panic!("boom");
            }));
            *outer = 5; // must succeed
        }
        assert_eq!(c.into_inner(), 5);
    }

    /* 4. raw‑pointer escape must not allow UB but should panic on use */
    #[cfg(miri)]
    #[test]
    fn raw_pointer_escape_panics() {
        let c = EpochCell::new(0u8);
        let mut g = c.borrow_mut();
        let p: *mut u8 = &mut *g;
        let _inner = c.borrow_mut(); // makes g stale
        unsafe { std::ptr::write(p, 1); } // uhh miri?
        let _ = *_inner;
    }

    /* 5. zero‑sized type still works */
    #[test]
    fn zst_ok() {
        let c = EpochCell::new(());
        {
            let _g = c.borrow_mut();
        }
        c.borrow(); // no panic
    }

    /* 6. DST slice inside the cell
     * TODO: make this work
    #[test]
    fn dst_slice() {
        let c: EpochCell<[u8]> = EpochCell::new([1, 2, 3]);
        {
            let mut g = c.borrow_mut();
            g[1] = 42;
            {
                let mut h = c.borrow_mut();
                h[2] = 99;
            }
            assert_eq!(g[2], 99);
        }
        assert_eq!(&*c.borrow(), &[1, 42, 99]);
    }
    */
}
