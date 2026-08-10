Items that might be worth exploring or implementing in the future:

- Ability to configure a sysroot for isolated testing, e.g. via `--sysroot`
- Replace `rayon` with `tokio` for ebuild dependency resolving
- Pass debug mode to ebuild phase execution
- Parse eclass information and calculate digest for proper cache invalidation
