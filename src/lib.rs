pub mod catalog;
pub mod cli;
pub mod contract;
pub mod error;
pub mod learn;
pub mod manifest;
pub mod models;
pub mod provider;
pub mod reasoning;
pub mod registry;
pub mod resolver;
pub mod runtime;
pub mod search;
pub mod storage;
pub mod utils;
pub mod verification;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
