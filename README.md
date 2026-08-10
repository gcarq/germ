# germ

germ (Gentoo Ebuild Repository Manager) is an experimental package manager for Gentoo that aims to implement the [Package Manager Specification](https://wiki.gentoo.org/wiki/Project:Package_Manager_Specification).

This is a recreational project and might not be continued or finished.

At this point it uses the existing portage config (`/etc/portage` and `/usr/share/portage/config`) and doesn't rely on additional configuration.

## Who is this for?

This project takes care of package indexing for [pkgindex](https://pkgindex.patchyard.org).
Other than that, it is mainly for curious Gentoo users and Rust developers who enjoy experimenting with package mangager internals. If you need a reliable package manager, use Portage.

## Known issues

- `germ-core` currently expects to find `./bin/ebuild.sh` relative to the working directory.
- `germ-core` currently uses `anyhow` for error handling, that means there is no way to distinguish between errors for users.
- Metadata is currently written to the hardcoded `.cache/metadata` path.
- A valid Portage configuration and system paths are assumed and are currently hardcoded.
- Only Git-based repository synchronization is supported.
- The `install` command is just a placeholder and doesn't install anything.

## Capabilities

### Implemented

- repository and profile handling
- repository synchronization (git only)
- ebuild metadata generation and caching
- dependency expression parsing
- package matching based on atoms
- reading installed package database (VDB)

All of this is experimental and might only work for common configurations. There are still many bugs and unsupported edge cases, especially due to the flexibility Portage offers.

### Planned

Smaller improvements and ideas are collected in [TODO.md](TODO.md).

- proper `USE_EXPAND` and keyword handling
- dependency resolution
- download and build package sources
- binary package handling
- package installation and removal
- PMS 9 support

## Quick Start

Clone this repository, build and run it with Cargo:

```sh
git clone https://github.com/gcarq/germ
cd germ/
cargo run --release -- info dev-lang/python
```

## Usage

```sh
Package management tool for Gentoo-like systems

Usage: germ [OPTIONS] <COMMAND>

Commands:
  info      Provides information about the system, useful for troubleshooting
  install   Install a package
  gencache  Generate metadata cache for ebuild repositories
  sync      Sync repositories
  help      Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose...  Increase verbosity
  -h, --help        Print help
  -V, --version     Print version
```

## Testing

```sh
cargo test --workspace
```

## Contributing

Contributions and bug reports are welcome. Since the project is still small and unfinished, keep changes focused and explain the problem they solve. LLM assisted PRs might be considered, as long as the code matches the expected code quality.

The code must pass the test suite `./scripts/test.sh` and should follow the [Rust Style Guide](https://doc.rust-lang.org/stable/style-guide/).

## Dependencies

- app-shells/bash
- dev-lang/rust
- dev-vcs/git
- sys-apps/sandbox

## Resources

- [Package Manager Specification](https://wiki.gentoo.org/wiki/Project:Package_Manager_Specification)
- [Gentoo devmanual](https://devmanual.gentoo.org/index.html)
- [portage](https://github.com/gentoo/portage)
