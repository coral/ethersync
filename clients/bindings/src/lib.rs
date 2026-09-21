//! Generated C, C++, Swift, and C# interfaces to one synchronous Rust facade.
#![allow(clippy::too_many_arguments)]
pub mod api;
#[cfg(feature = "c")]
mod c {
    include!(concat!(env!("OUT_DIR"), "/c.rs"));
}
#[cfg(feature = "swift")]
// swift-bridge 0.1.59 emits redundant casts in opaque Result conversions.
#[allow(clippy::unnecessary_cast)]
mod swift {
    include!(concat!(env!("OUT_DIR"), "/swift.rs"));
}
#[cfg(feature = "c")]
mod c_support;
