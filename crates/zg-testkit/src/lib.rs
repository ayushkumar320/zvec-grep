//! Shared fakes, fixture readers and contract suites for Core adapters.

pub mod contracts;
pub mod fakes;
mod fixture;

pub use fixture::{CliCompatibilityCase, FixtureError, load_cli_case};
