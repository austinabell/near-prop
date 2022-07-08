#![feature(async_closure)]

mod runner;

pub use quickcheck::{Arbitrary, Gen};
pub use runner::{TestResult, Testable};
