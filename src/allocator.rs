// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::alloc::Layout;
use core::ptr;
#[cfg(feature = "driver")]
use core::ffi::c_void;

pub trait Allocator {
    /// # Safety
    /// `layout` must be valid.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8;

    /// # Safety
    /// `layout` must be valid.
    /// The returned memory is zero-initialized.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.alloc(layout) };
        if !ptr.is_null() {
            unsafe { ptr::write_bytes(ptr, 0, layout.size()) };
        }
        ptr
    }

    /// # Safety
    /// `ptr` must have been allocated by this allocator with the same `layout`.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);
}

pub struct GlobalAllocator;

impl Allocator for GlobalAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        alloc::alloc::alloc(layout)
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        alloc::alloc::dealloc(ptr, layout)
    }
}

#[cfg(feature = "driver")]
#[derive(Copy, Clone)]
pub enum PoolType {
    NonPagedNx,
    Paged,
}

#[cfg(feature = "driver")]
#[derive(Copy, Clone)]
pub struct WdkAllocator {
    pub pool: PoolType,
    pub tag: u32,
}

#[cfg(feature = "driver")]
impl WdkAllocator {
    #[inline]
    pub const fn new(pool: PoolType, tag: u32) -> Self {
        Self { pool, tag }
    }
}

#[cfg(feature = "driver")]
impl Allocator for WdkAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::NonNull::<u8>::dangling().as_ptr();
        }

        let flags = match self.pool {
            PoolType::NonPagedNx => POOL_FLAG_NON_PAGED,
            PoolType::Paged => POOL_FLAG_PAGED,
        };

        let ptr = unsafe { ExAllocatePool2(flags, layout.size(), self.tag) };
        ptr as *mut u8
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.size() == 0 {
            return;
        }
        unsafe { ExFreePoolWithTag(ptr as _, self.tag) }
    }
}

#[cfg(feature = "driver")]
const POOL_FLAG_PAGED: u64 = 0x0000_0001;
#[cfg(feature = "driver")]
const POOL_FLAG_NON_PAGED: u64 = 0x0000_0040;

#[cfg(feature = "driver")]
unsafe extern "C" {
    fn ExAllocatePool2(flags: u64, number_of_bytes: usize, tag: u32) -> *mut c_void;
}

#[cfg(feature = "driver")]
unsafe extern "C" {
    fn ExFreePoolWithTag(p: *mut c_void, tag: u32);
}
