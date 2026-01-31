// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use alloc::string::{FromUtf16Error, String};
use core::alloc::Layout;
use core::fmt;
use core::mem::ManuallyDrop;
use core::slice;

use crate::allocator::{Allocator, GlobalAllocator};
use crate::ntddk::UNICODE_STRING;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnicodeStringError {
    TooLong,
    AllocationFailed,
}

impl fmt::Display for UnicodeStringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => f.write_str("UNICODE_STRING exceeds u16 length"),
            Self::AllocationFailed => f.write_str("UNICODE_STRING allocation failed"),
        }
    }
}

#[macro_export]
/// Returns a reference to a compile-time `UNICODE_STRING` built from a string literal.
///
/// The backing buffer is UTF-16 with a trailing NUL. The returned `UNICODE_STRING`
/// references static storage and is safe to share across threads.
///
/// Requires the `kernel-unicode` feature.
macro_rules! kstr {
    ($lit:literal) => {{
        const BUF: &[u16] = &$crate::utf16_lit::utf16_null!($lit);
        static UNICODE: $crate::UNICODE_STRING = $crate::UNICODE_STRING {
            Length: ((BUF.len() - 1) * 2) as u16,
            MaximumLength: (BUF.len() * 2) as u16,
            Buffer: BUF.as_ptr() as *mut u16,
        };
        &UNICODE
    }};
}

/// An owned UNICODE_STRING backed by a UTF-16 buffer.
///
/// The buffer is constructed once, boxed, and never resized.
/// Mutable accessors are intentionally omitted to keep the backing storage
/// stable for the lifetime of the UNICODE_STRING.
///
/// The returned `UNICODE_STRING` references memory owned by this type. Callers
/// must not free it (e.g., via `RtlFreeUnicodeString`) or store it beyond the
/// lifetime of the `OwnedUnicodeString` instance.
pub struct OwnedUnicodeString<A: Allocator + Send + Sync = GlobalAllocator> {
    inner: UNICODE_STRING,
    buffer: *mut u16,
    buffer_len: usize,
    alloc: ManuallyDrop<A>,
}

impl<A: Allocator + Send + Sync> OwnedUnicodeString<A> {
    pub fn new_in(value: &str, alloc: A) -> Result<Self, UnicodeStringError> {
        let len = value.encode_utf16().count();
        let max_units = (u16::MAX as usize) / 2;
        if len + 1 > max_units {
            return Err(UnicodeStringError::TooLong);
        }
        let layout = Layout::array::<u16>(len + 1).map_err(|_| UnicodeStringError::TooLong)?;
        let buffer = unsafe { alloc.alloc(layout) } as *mut u16;
        if buffer.is_null() {
            return Err(UnicodeStringError::AllocationFailed);
        }
        for (idx, unit) in value.encode_utf16().enumerate() {
            unsafe {
                buffer.add(idx).write(unit);
            }
        }
        unsafe {
            buffer.add(len).write(0);
        }
        let inner = UNICODE_STRING {
            Length: (len * 2) as u16,
            MaximumLength: ((len + 1) * 2) as u16,
            Buffer: buffer,
        };
        Ok(Self {
            inner,
            buffer,
            buffer_len: len + 1,
            alloc: ManuallyDrop::new(alloc),
        })
    }

    /// Returns a borrowed UNICODE_STRING view.
    ///
    /// The underlying buffer remains owned by this instance and must not be freed
    /// by the caller.
    #[inline]
    pub fn as_unicode(&self) -> &UNICODE_STRING {
        &self.inner
    }

    #[inline]
    pub fn as_ptr(&self) -> *const UNICODE_STRING {
        &self.inner
    }

    #[inline]
    pub fn as_utf16(&self) -> &[u16] {
        let len = self.inner.Length as usize / 2;
        unsafe { slice::from_raw_parts(self.buffer, len) }
    }
}

impl OwnedUnicodeString<GlobalAllocator> {
    pub fn new(value: &str) -> Result<Self, UnicodeStringError> {
        Self::new_in(value, GlobalAllocator)
    }
}

impl<A: Allocator + Send + Sync> Drop for OwnedUnicodeString<A> {
    fn drop(&mut self) {
        if self.buffer.is_null() {
            return;
        }
        let layout = match Layout::array::<u16>(self.buffer_len) {
            Ok(layout) => layout,
            Err(_) => return,
        };
        let alloc = unsafe { core::ptr::read(&self.alloc) };
        let alloc = ManuallyDrop::into_inner(alloc);
        unsafe {
            alloc.dealloc(self.buffer as *mut u8, layout);
        }
        drop(alloc);
    }
}

/// Returns the UTF-16 slice referenced by a UNICODE_STRING.
///
/// # Safety
/// Caller must ensure the UNICODE_STRING buffer is valid for reads.
pub unsafe fn unicode_string_as_slice(unicode: &UNICODE_STRING) -> &[u16] {
    if unicode.Buffer.is_null() || unicode.Length == 0 {
        return &[];
    }
    let len = unicode.Length as usize / 2;
    unsafe { slice::from_raw_parts(unicode.Buffer, len) }
}

/// Converts a UNICODE_STRING into an owned Rust String.
///
/// # Safety
/// Caller must ensure the UNICODE_STRING buffer is valid for reads.
pub unsafe fn unicode_string_to_string(unicode: &UNICODE_STRING) -> Result<String, FromUtf16Error> {
    let slice = unsafe { unicode_string_as_slice(unicode) };
    String::from_utf16(slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_string_allows_max_len_minus_one() {
        let value: String = std::iter::repeat('a').take(32_766).collect();
        let owned = OwnedUnicodeString::new(&value).expect("expected string to fit");
        assert_eq!(owned.as_unicode().Length, (32_766 * 2) as u16);
        assert_eq!(owned.as_unicode().MaximumLength, ((32_766 + 1) * 2) as u16);
    }

    #[test]
    fn unicode_string_rejects_overflow() {
        let value: String = std::iter::repeat('a').take(32_767).collect();
        let err = OwnedUnicodeString::new(&value).unwrap_err();
        assert_eq!(err, UnicodeStringError::TooLong);
    }

    #[test]
    fn kstr_macro_builds_unicode_string() {
        let unicode = kstr!("Test");
        assert_eq!(unicode.Length, 8);
        assert_eq!(unicode.MaximumLength, 10);

        let expected: Vec<u16> = "Test".encode_utf16().collect();
        let slice = unsafe { unicode_string_as_slice(unicode) };
        assert_eq!(slice, expected.as_slice());

        let len = (unicode.Length / 2) as usize;
        unsafe {
            assert_eq!(*unicode.Buffer.add(len), 0);
        }
    }
}
