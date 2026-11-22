//! Compile-time selectable cross-border sources.

#[cfg(feature = "source-border")]
pub mod border;
#[cfg(feature = "source-coach")]
pub mod coach;
#[cfg(feature = "source-ferry")]
pub mod ferry;
#[cfg(feature = "source-hsr")]
pub mod hsr;
