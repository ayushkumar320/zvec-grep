import assert from "node:assert/strict";
import test from "node:test";

import { loadReleaseManifest, selectPlatform } from "./npm-package-core.mjs";

const manifest = loadReleaseManifest();

test("maps supported native npm targets", () => {
  assert.deepEqual(selectPlatform(manifest, "darwin", "arm64"), {
    package: "@zvec/zvec-grep-darwin-arm64",
    os: "darwin",
    cpu: "arm64",
    binary: "zg",
    target: "darwin-arm64",
  });
  assert.deepEqual(selectPlatform(manifest, "linux", "x64", { glibcVersionRuntime: "2.39" }), {
    package: "@zvec/zvec-grep-linux-x64-gnu",
    os: "linux",
    cpu: "x64",
    libc: "glibc",
    binary: "zg",
    target: "linux-x64-gnu",
  });
  assert.deepEqual(selectPlatform(manifest, "win32", "x64"), {
    package: "@zvec/zvec-grep-win32-x64-msvc",
    os: "win32",
    cpu: "x64",
    binary: "zg.exe",
    target: "win32-x64-msvc",
  });
});

test("rejects platforms without a local release target", () => {
  assert.throws(
    () => selectPlatform(manifest, "freebsd", "x64"),
    /unsupported npm platform/,
  );
  assert.throws(
    () => selectPlatform(manifest, "win32", "arm64"),
    /unsupported npm platform/,
  );
  assert.throws(
    () => selectPlatform(manifest, "linux", "riscv64", { glibcVersionRuntime: "2.39" }),
    /unsupported npm platform/,
  );
  assert.throws(
    () => selectPlatform(manifest, "linux", "x64"),
    /unsupported npm platform: linux-x64-musl/,
  );
});
