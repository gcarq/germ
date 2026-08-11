#![cfg_attr(test, allow(clippy::similar_names))]

pub mod conf;
pub mod consts;
pub mod deps;
mod eapi;
mod ebuild;
mod files;
pub mod makenv;
pub mod package;
mod profile;
mod regex;
pub mod repository;
pub use conf::system::SysConf;
#[cfg(test)]
pub(crate) mod test_support;
mod types;
mod utils;
pub mod vdb;
