#![cfg_attr(test, allow(clippy::similar_names))]

pub mod conf;
pub mod consts;
mod eapi;
mod ebuild;
mod files;
mod keyword;
pub mod makenv;
pub mod package_set;
pub mod policy;
mod profile;
pub mod repository;
pub mod resolver;
#[cfg(test)]
pub(crate) mod test_support;
pub mod useflag;
pub use conf::system::SysConf;
mod types;
mod utils;
pub mod vdb;
