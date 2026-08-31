# zg-host-native

Native filesystem scanning and resident watch sessions. Scanner/watcher request
types, cancellation control and traits belong to this crate; they are not
exported by `zg-engine`.

This crate is standalone: it exposes `HostError` and does not depend on
`zg-engine`. Engine composition maps native failures to `EngineError` at the
indexing boundary.

The crate provides metadata-first discovery, bounded source reads, normalized
change batches, ignore/filter compatibility and explicit scan/watch resource
limits. The engine indexing pipeline consumes the scanner directly; the
concrete storage implementation still needs to be composed into
`ZvecGrep::index`.
