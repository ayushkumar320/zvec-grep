# zg-host-native

Native filesystem scanning and resident watch sessions. Scanner/watcher request
types, cancellation control and traits belong to this crate; they are not
exported by `zg-engine`.

The crate provides metadata-first discovery, bounded source reads, normalized
change batches, ignore/filter compatibility and explicit scan/watch resource
limits. It is not yet composed into `ZvecGrep::index`.
