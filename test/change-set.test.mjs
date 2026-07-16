import assert from "node:assert/strict";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { ChangeSet } from "../dist/daemon/change-set.js";

test("change set folds child paths and invalidates gitignore subtrees", () => {
  const root = join(tmpdir(), "change-set-repo");
  const changes = new ChangeSet();
  changes.add(join(root, "src", "a.ts"), "changed");
  changes.add(join(root, "src", "b.ts"), "changed");
  changes.add(join(root, "src"), "created", true);
  changes.add(join(root, "packages", ".gitignore"), "changed");
  assert.deepEqual(changes.snapshot(), {
    touchedFiles: [],
    rescanDirectories: [join(root, "packages"), join(root, "src")].sort(),
    deletedPrefixes: [],
    forceFullReconcile: false,
  });
});

test("change set collapses deleted prefixes and upgrades event storms", () => {
  const root = join(tmpdir(), "change-set-storm-repo");
  const changes = new ChangeSet({ maxChangedPaths: 3 });
  changes.add(join(root, "src", "a.ts"), "deleted");
  changes.add(join(root, "src"), "deleted", true);
  changes.add(join(root, "other.ts"), "changed");
  changes.add(join(root, "third.ts"), "changed");
  const snapshot = changes.snapshot();
  assert.deepEqual(snapshot.deletedPrefixes, [join(root, "src")]);
  assert.equal(snapshot.forceFullReconcile, true);
});
