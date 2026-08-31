//! Internal persistence contract for workspace indexes.

#![expect(
    dead_code,
    reason = "the storage SPI is intentionally defined before its implementation"
)]

pub(crate) mod spi;
