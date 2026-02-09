/// List of EAPIs that are considered valid and in various places
pub const VALID_EAPIS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// List of ebuild EAPIs that are currently supported
pub const SUPPORTED_EBUILD_EAPIS: [&str; 3] = ["7", "8", "9"];

pub const DEFAULT_PORTAGE_CONF_PATH: &str = "/usr/share/portage/config";

pub const DEFAULT_USE_PORTAGE_CONF_PATH: &str = "/etc/portage";

pub const BASH_BINARY_PATH: &str = "/bin/bash";
pub const SANDBOX_BINARY_PATH: &str = "/bin/sandbox";
pub const GIT_BINARY_PATH: &str = "/usr/bin/git";
