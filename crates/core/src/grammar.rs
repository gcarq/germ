//! regex grammar fragments from the PMS.

/// PMS 3.1.1 category name syntax.
pub const CATEGORY: &str = r"[a-zA-Z0-9_][a-zA-Z0-9_+.-]*";

/// PMS 3.1.2 package name lexical syntax.
pub const PACKAGE: &str = r"[a-zA-Z0-9_][a-zA-Z0-9_+-]*";

/// PMS 3.1.3 slot name syntax.
pub const SLOT: &str = r"[a-zA-Z0-9_][a-zA-Z0-9_+.-]*";

/// PMS 3.1.4 USE flag name syntax.
pub const USE_FLAG: &str = r"[A-Za-z0-9][A-Za-z0-9+_@-]*";

/// PMS 3.1.5 repository name syntax.
pub const REPOSITORY: &str = r"[a-zA-Z0-9_][a-zA-Z0-9_-]*";

/// PMS 3.2 package version base syntax: numeric components and an optional letter.
pub const VERSION: &str = r"[0-9]+(?:\.[0-9]+)*[a-z]?";

/// PMS 3.2 package version suffix syntax.
pub const VERSION_SUFFIXES: &str = r"(?:_(?:alpha|beta|pre|rc|p)[0-9]*)*";

/// PMS 3.2 package revision digits, without `-r`.
pub const REVISION: &str = r"[0-9]+";
