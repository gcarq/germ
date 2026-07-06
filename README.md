# pkgrove

pkgrove is an experimental package manager for Gentoo that aims to adhere to the [Package Manager Specification](https://wiki.gentoo.org/wiki/Project:Package_Manager_Specification).

At this stage it's a recreational project and might not be continued or finished.

## Usage
```
Package management tool for Gentoo-like systems

Usage: pkgrove [OPTIONS] <COMMAND>

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
