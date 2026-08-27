# Native scanner and watcher adapter

`zg-host-native` implements the Core-owned host ports without exposing `notify`,
`globset` or filesystem-native types outside this crate.

## Public adapters

- `NativeScanner` implements `WorkspaceScannerPort`. Discovery reads metadata,
  file headers for binary detection, ignore files and directory entries; full
  source bytes are read only by `read_batch`.
- `NativeWatcherFactory` implements `WorkspaceWatcherFactoryPort`. Each session
  yields root-relative `Upsert`, `Delete`, `RescanDirectory`, `DeletePrefix` or
  `Rescan` changes and shuts down with the daemon-owned session.
- `NativeWatcherConfig` controls debounce, maximum wait, reconciliation,
  suspend detection, queue capacities, path compaction and the watcher backend.

Scanner and watcher construct the same `RootPolicy`, so include/exclude paths,
ripgrep globs and file types, hidden files, ignore files, depth, symlinks and
nested Git repositories cannot drift between the initial scan and incremental
updates. Direct and Server integration must inject this same scanner adapter;
the daemon owns watcher sessions but translates their batches into ordinary
Core Index operations.

## Resource behavior

`NativeScanner` allows one concurrent blocking scan by default. The composition
root may call `with_max_concurrent_scans`, while the process-wide Tokio blocking
thread limit remains owned by the Core runtime. Watch callbacks use a bounded
raw-event queue; overflow becomes one full `Rescan`. The normalized batch queue
is also bounded, and session shutdown interrupts a blocked send.

The default watcher uses the platform-native `notify` backend. Set
`poll_interval` only for filesystems or environments where native notifications
are unavailable. Polling hashes file contents to preserve rapid-change
correctness and therefore costs more I/O and CPU.

## Compatibility

The implementation matches the TypeScript scanner defaults and watcher timing
defaults. Intentional fixes are registered in
`compat/allowed-differences.toml`. In particular, conflicting events for the
same path use last-event-wins semantics, so delete followed by recreate produces
an upsert, while create followed by delete produces a deletion.

Run the adapter and shared fake/production contracts with:

```sh
cargo test -p zg-host-native
cargo clippy -p zg-host-native --all-targets -- -D warnings
```
