#![no_std]
#![deny(warnings)]
#![deny(clippy::missing_docs_in_private_items)]

//! Defines a [`Send`] and [`Sync`] version of [`core::cell::RefCell`], which aborts the program
//! if an attempted borrow fails.

use core::any::*;
use core::cell::*;
use core::mem::*;
use core::ops::*;
use core::sync::atomic::*;
pub use mutability_marker::*;

/// A lightweight reference-counted cell. Aborts the program when borrows conflict.
#[derive(Debug, Default)]
pub struct RwCell<T> {
    /// The inner cell data.
    inner: ReadCell<RwCellInner<T>>,
}

impl<T> RwCell<T> {
    /// Creates a new cell that wraps the provided value.
    #[inline(always)]
    pub const fn new(value: T) -> Self {
        Self {
            inner: ReadCell::new(RwCellInner {
                borrow_state: AtomicU16::new(0),
                value: UnsafeCell::new(value),
            }),
        }
    }

    /// Immutably borrows the value of this cell.
    #[inline(always)]
    pub fn borrow(&self) -> RwCellGuard<Const, T> {
        unsafe {
            Self::abort_if(
                self.inner.borrow_state.fetch_add(1, Ordering::AcqRel) >= u16::MAX - 1,
                "Attempted to immutably borrow cell while it was mutably borrowed.",
            );
            RwCellGuard {
                value: &*(self.inner.value.get() as *const T),
                borrow_state: &self.inner.borrow_state,
            }
        }
    }

    /// Mutably borrows the value of this cell.
    #[inline(always)]
    pub fn borrow_mut(&self) -> RwCellGuard<Mut, T> {
        unsafe {
            Self::abort_if(
                self.inner.borrow_state.swap(u16::MAX, Ordering::AcqRel) != 0,
                "Attempted to mutably borrow cell while other borrows already existed.",
            );
            RwCellGuard {
                value: &mut *self.inner.value.get(),
                borrow_state: &self.inner.borrow_state,
            }
        }
    }

    /// Determines whether this cell is free to be borrowed.
    #[inline(always)]
    pub fn free(&self) -> bool {
        self.inner.borrow_state.load(Ordering::Acquire) == 0
    }

    /// Aborts the program if the given condition is true.
    #[inline(always)]
    fn abort_if(condition: bool, reason: &str) {
        if condition {
            AbortPanic::abort(reason);
        }
    }
}

/// Stores the inner data for a read-write cell.
#[derive(Debug, Default)]
struct RwCellInner<T> {
    /// The value contained in the cell.
    value: UnsafeCell<T>,
    /// The borrow counter.
    borrow_state: AtomicU16,
}

/// A resource guard that dynamically controls the lifetime of a mutable read-write cell borrow.
#[derive(Debug)]
pub struct RwCellGuard<'a, M: Mutability, T: 'a + ?Sized> {
    /// The value currently being borrowed.
    value: M::Ref<'a, T>,
    /// The borrow counter.
    borrow_state: &'a AtomicU16,
}

impl<'a, M: Mutability, T: 'a + ?Sized> RwCellGuard<'a, M, T> {
    /// Removes the lifetime from a guard, allowing it to be freely used in
    /// other parts of the program.
    ///
    /// # Safety
    ///
    /// For this function to be sound, the underlying cell must not be moved or
    /// destroyed while this guard exists.
    #[inline(always)]
    pub unsafe fn detach(self) -> RwCellGuard<'static, M, T> {
        transmute(self)
    }
}

impl<'a, T: 'a + ?Sized> RwCellGuard<'a, Const, T> {
    /// Creates a reference to a specific portion of a value.
    #[inline(always)]
    pub fn map<U, F>(orig: Self, f: F) -> RwCellGuard<'a, Const, U>
    where
        F: FnOnce(&T) -> &U,
        U: ?Sized,
    {
        let result = RwCellGuard {
            value: f(orig.value),
            borrow_state: orig.borrow_state,
        };
        forget(orig);
        result
    }
}

impl<'a, T: 'a + ?Sized> RwCellGuard<'a, Mut, T> {
    /// Creates a reference to a specific portion of a value.
    #[inline(always)]
    pub fn map<U, F>(orig: Self, f: F) -> RwCellGuard<'a, Mut, U>
    where
        F: FnOnce(&mut T) -> &mut U,
        U: ?Sized,
    {
        let RwCellGuardDestructure {
            value,
            borrow_state,
        } = orig.into();

        RwCellGuard {
            value: f(value),
            borrow_state,
        }
    }
}

impl<'a, M: Mutability, T: 'a + ?Sized> Deref for RwCellGuard<'a, M, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'a, T: 'a + ?Sized> DerefMut for RwCellGuard<'a, Mut, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

impl<'a, M: Mutability, T: 'a + ?Sized> Drop for RwCellGuard<'a, M, T> {
    #[inline(always)]
    fn drop(&mut self) {
        if TypeId::of::<M>() == TypeId::of::<Mut>() {
            self.borrow_state.store(0, Ordering::Release);
        }
        else {
            self.borrow_state.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// Allows for destructure a mutable guard without running its destructor.
struct RwCellGuardDestructure<'a, M: Mutability, T: ?Sized + 'a> {
    /// The value currently being borrowed.
    value: M::Ref<'a, T>,
    /// The borrow counter.
    borrow_state: &'a AtomicU16,
}

impl<'a, M: Mutability, T: ?Sized> From<RwCellGuard<'a, M, T>> for RwCellGuardDestructure<'a, M, T> {
    #[inline(always)]
    fn from(value: RwCellGuard<'a, M, T>) -> Self {
        unsafe {
            let mut result = MaybeUninit::uninit();
            (result.as_mut_ptr() as *mut RwCellGuard<M, T>).write(value);
            result.assume_init()
        }
    }
}

/// Implements an uncatchable panic.
struct AbortPanic(*const str);

impl AbortPanic {
    /// Immediately aborts the program with the given message.
    #[allow(unused_variables)]
    #[inline(always)]
    fn abort(message: &str) -> ! {
        let guard = Self(message);
        panic!("{:?}", message);
    }
}

impl Drop for AbortPanic {
    fn drop(&mut self) {
        unsafe {
            panic!("{:?}", &*self.0);
        }
    }
}

/// A read-only cell that allows immutable references for the inner data to
/// be held simultaneously as mutable references to the outer data.
#[derive(Debug, Default)]
struct ReadCell<T> {
    /// The underlying value.
    inner: UnsafeCell<T>,
}

impl<T> ReadCell<T> {
    /// Creates a new cell with the given value.
    #[inline(always)]
    pub const fn new(value: T) -> Self {
        Self {
            inner: UnsafeCell::new(value),
        }
    }
}

impl<T> Deref for ReadCell<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.get() }
    }
}