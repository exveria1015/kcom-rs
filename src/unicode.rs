// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

use alloc::boxed::Box;
use alloc::string::{FromUtf16Error, String};
use alloc::vec::Vec;
use core::fmt;
use core::slice;

use wdk_sys::ntddk::UNICODE_STRING;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnicodeStringError {
    TooLong,
}

impl fmt::Display for UnicodeStringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(f, "UNICODE_STRING exceeds u16 length"),
        }
    }
}

/// An owned UNICODE_STRING backed by a UTF-16 buffer.
///
/// The buffer is constructed once, boxed, and never resized.
/// Mutable accessors are intentionally omitted to keep the backing storage
/// stable for the lifetime of the UNICODE_STRING.
pub struct OwnedUnicodeString {
    inner: UNICODE_STRING,
    buffer: Box<[u16]>,
}

impl OwnedUnicodeString {
    pub fn new(value: &str) -> Result<Self, UnicodeStringError> {
        let mut buffer: Vec<u16> = value.encode_utf16().collect();
        let len = buffer.len();
        let max_units = (u16::MAX as usize) / 2;
        if len + 1 > max_units {
            return Err(UnicodeStringError::TooLong);
        }
        buffer.push(0);
        let length = (len * 2) as u16;
        let maximum_length = (buffer.len() * 2) as u16;
        let mut buffer = buffer.into_boxed_slice();
        let inner = UNICODE_STRING {
            Length: length,
            MaximumLength: maximum_length,
            Buffer: buffer.as_mut_ptr(),
        };
        Ok(Self { inner, buffer })
    }

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
        &self.buffer[..self.inner.Length as usize / 2]
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
}
