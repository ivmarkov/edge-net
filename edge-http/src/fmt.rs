//! A module that re-exports the `log` macros if `defmt` is not enabled, or `defmt` macros if `defmt` is enabled.
//!
//! The module also defines:
//! - Custom versions of the core assert macros (`assert!`, `assert_eq!`, etc.) that use the `defmt` macros if the `defmt` feature is enabled.
//! - Custom versions of the `panic!`, `todo!`, and `unreachable!` macros that use the `defmt` macros if the `defmt` feature is enabled.
//! - A custom `unwrap!` macro that uses the `defmt` macros if the `defmt` feature is enabled, otherwise it uses the standard library's `unwrap` method.
//! - A custom `Bytes` struct that formats byte slices as hex in a way compatible with `defmt`.
#![macro_use]
#![allow(unused)]

use core::fmt::{Debug, Display, LowerHex};

#[collapse_debuginfo(yes)]
macro_rules! assert {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::assert!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::assert!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! assert_eq {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::assert_eq!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::assert_eq!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! assert_ne {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::assert_ne!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::assert_ne!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! debug_assert {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::debug_assert!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::debug_assert!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! debug_assert_eq {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::debug_assert_eq!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::debug_assert_eq!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! debug_assert_ne {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::debug_assert_ne!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::debug_assert_ne!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! todo {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::todo!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::todo!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! unreachable {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::unreachable!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::unreachable!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! panic {
    ($($x:tt)*) => {
        {
            #[cfg(not(feature = "defmt"))]
            ::core::panic!($($x)*);
            #[cfg(feature = "defmt")]
            ::defmt::panic!($($x)*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! trace {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "log")]
            ::log::trace!($s $(, $x)*);
            #[cfg(feature = "defmt")]
            ::defmt::trace!($s $(, $x)*);
            #[cfg(not(any(feature = "log", feature="defmt")))]
            let _ = ($( & $x ),*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! debug {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "log")]
            ::log::debug!($s $(, $x)*);
            #[cfg(feature = "defmt")]
            ::defmt::debug!($s $(, $x)*);
            #[cfg(not(any(feature = "log", feature="defmt")))]
            let _ = ($( & $x ),*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! info {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "log")]
            ::log::info!($s $(, $x)*);
            #[cfg(feature = "defmt")]
            ::defmt::info!($s $(, $x)*);
            #[cfg(not(any(feature = "log", feature="defmt")))]
            let _ = ($( & $x ),*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! warn {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "log")]
            ::log::warn!($s $(, $x)*);
            #[cfg(feature = "defmt")]
            ::defmt::warn!($s $(, $x)*);
            #[cfg(not(any(feature = "log", feature="defmt")))]
            let _ = ($( & $x ),*);
        }
    };
}

#[collapse_debuginfo(yes)]
macro_rules! error {
    ($s:literal $(, $x:expr)* $(,)?) => {
        {
            #[cfg(feature = "log")]
            ::log::error!($s $(, $x)*);
            #[cfg(feature = "defmt")]
            ::defmt::error!($s $(, $x)*);
            #[cfg(not(any(feature = "log", feature="defmt")))]
            let _ = ($( & $x ),*);
        }
    };
}

#[cfg(feature = "defmt")]
#[collapse_debuginfo(yes)]
macro_rules! unwrap {
    ($($x:tt)*) => {
        ::defmt::unwrap!($($x)*)
    };
}

#[cfg(not(feature = "defmt"))]
#[collapse_debuginfo(yes)]
macro_rules! unwrap {
    ($arg:expr) => {
        match $crate::fmt::Try::into_result($arg) {
            ::core::result::Result::Ok(t) => t,
            ::core::result::Result::Err(e) => {
                ::core::panic!("unwrap of `{}` failed: {:?}", ::core::stringify!($arg), e);
            }
        }
    };
    ($arg:expr, $($msg:expr),+ $(,)? ) => {
        match $crate::fmt::Try::into_result($arg) {
            ::core::result::Result::Ok(t) => t,
            ::core::result::Result::Err(e) => {
                ::core::panic!("unwrap of `{}` failed: {}: {:?}", ::core::stringify!($arg), ::core::format_args!($($msg,)*), e);
            }
        }
    }
}

#[cfg(feature = "defmt")]
#[collapse_debuginfo(yes)]
macro_rules! write_unwrap {
    ($f:expr, $s:literal $(, $x:expr)* $(,)?) => {
        {
            unwrap!(write!($f, $s $(, $x)*).map_err($crate::fmt::FmtError));
        }
    };
}

#[cfg(not(feature = "defmt"))]
#[collapse_debuginfo(yes)]
macro_rules! write_unwrap {
    ($f:expr, $s:literal $(, $x:expr)* $(,)?) => {
        {
            unwrap!(write!($f, $s $(, $x)*));
        }
    };
}

#[cfg(feature = "defmt")]
#[collapse_debuginfo(yes)]
macro_rules! display2format {
    ($arg:expr) => {
        ::defmt::Display2Format(&$arg)
    };
}

#[cfg(not(feature = "defmt"))]
#[collapse_debuginfo(yes)]
macro_rules! display2format {
    ($arg:expr) => {
        $arg
    };
}

#[cfg(feature = "defmt")]
#[collapse_debuginfo(yes)]
macro_rules! debug2format {
    ($arg:expr) => {
        ::defmt::Debug2Format(&$arg)
    };
}

#[cfg(not(feature = "defmt"))]
#[collapse_debuginfo(yes)]
macro_rules! debug2format {
    ($arg:expr) => {
        $arg
    };
}

/// A way to `{:x?}` format a byte slice which is compatible with `defmt`
pub struct Bytes<'a>(pub &'a [u8]);

impl Debug for Bytes<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#02x?}", self.0)
    }
}

impl Display for Bytes<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#02x?}", self.0)
    }
}

impl LowerHex for Bytes<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#02x?}", self.0)
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for Bytes<'_> {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{:02x}", self.0)
    }
}

/// Support for the `unwrap!` macro for `Option` and `Result`.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct NoneError;

/// Support for the `unwrap!` macro for `Option` and `Result`.
pub trait Try {
    type Ok;
    type Error;
    #[allow(unused)]
    fn into_result(self) -> Result<Self::Ok, Self::Error>;
}

impl<T> Try for Option<T> {
    type Ok = T;
    type Error = NoneError;

    #[inline]
    fn into_result(self) -> Result<T, NoneError> {
        self.ok_or(NoneError)
    }
}

impl<T, E> Try for Result<T, E> {
    type Ok = T;
    type Error = E;

    #[inline]
    fn into_result(self) -> Self {
        self
    }
}

#[cfg(feature = "defmt")]
pub(crate) struct FmtError(pub(crate) core::fmt::Error);

#[cfg(feature = "defmt")]
impl defmt::Format for FmtError {
    fn format(&self, f: defmt::Formatter<'_>) {
        defmt::write!(f, "{}", "FmtError")
    }
}
