#![feature(async_closure)]

mod runner;

pub use quickcheck::{Arbitrary, Gen};
pub use runner::{prop, NearProp, TestResult, Testable};

#[cfg(feature = "use_logging")]
fn env_logger_init() -> Result<(), log::SetLoggerError> {
    env_logger::try_init()
}
#[cfg(feature = "use_logging")]
macro_rules! info {
    ($($tt:tt)*) => {
        log::info!($($tt)*)
    };
}

pub(crate) use info;

#[cfg(not(feature = "use_logging"))]
fn env_logger_init() {}
#[cfg(not(feature = "use_logging"))]
macro_rules! info {
    ($($_ignore:tt)*) => {
        ()
    };
}
