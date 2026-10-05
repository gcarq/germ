#![cfg_attr(test, allow(clippy::similar_names))]

pub mod conf;
pub mod consts;

mod eapi;
mod ebuild;
mod files;

mod keyword;
pub mod makenv;
pub mod package;
pub mod policy;
mod profile;
pub mod repository;
pub mod resolver;
pub mod useflag;
pub use conf::system::SysConf;
#[cfg(test)]
mod test_support;
mod types;
mod utils;
pub mod vdb;
