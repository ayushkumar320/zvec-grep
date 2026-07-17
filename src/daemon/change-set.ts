import { basename, dirname, isAbsolute, relative, sep } from "node:path";
import { normalizePath } from "../engine/utils/path.js";

export type ChangeKind = "created" | "changed" | "deleted";

export type ChangeSetSnapshot = {
  touchedFiles: string[];
  rescanDirectories: string[];
  deletedPrefixes: string[];
  forceFullReconcile: boolean;
};

export type ChangeSetOptions = {
  maxChangedPaths?: number;
};

export class ChangeSet {
  private readonly touchedFiles = new Set<string>();
  private readonly rescanDirectories = new Set<string>();
  private readonly deletedPrefixes = new Set<string>();
  private forceFullReconcile = false;
  private readonly maxChangedPaths: number;

  constructor(options: ChangeSetOptions = {}) {
    this.maxChangedPaths = options.maxChangedPaths ?? 1_000;
  }

  add(path: string, kind: ChangeKind, isDirectory = false): void {
    if (!isAbsolute(path)) {
      throw new Error("Changed paths must be absolute.");
    }
    const normalized = normalizePath(path);
    if (basename(normalized) === ".gitignore") {
      this.rescanDirectories.add(dirname(normalized));
    } else if (kind === "deleted") {
      this.deletedPrefixes.add(normalized);
    } else if (isDirectory) {
      this.rescanDirectories.add(normalized);
    } else {
      this.touchedFiles.add(normalized);
    }
    this.collapsePaths();
    if (this.size >= this.maxChangedPaths) {
      this.forceFullReconcile = true;
    }
  }

  requireFullReconcile(): void {
    this.forceFullReconcile = true;
  }

  merge(other: ChangeSetSnapshot): void {
    for (const path of other.touchedFiles) this.touchedFiles.add(path);
    for (const path of other.rescanDirectories)
      this.rescanDirectories.add(path);
    for (const path of other.deletedPrefixes) this.deletedPrefixes.add(path);
    this.forceFullReconcile ||= other.forceFullReconcile;
    this.collapsePaths();
  }

  snapshot(): ChangeSetSnapshot {
    return {
      touchedFiles: [...this.touchedFiles].sort(),
      rescanDirectories: [...this.rescanDirectories].sort(),
      deletedPrefixes: [...this.deletedPrefixes].sort(),
      forceFullReconcile: this.forceFullReconcile,
    };
  }

  get size(): number {
    return (
      this.touchedFiles.size +
      this.rescanDirectories.size +
      this.deletedPrefixes.size
    );
  }

  get empty(): boolean {
    return this.size === 0 && !this.forceFullReconcile;
  }

  private collapsePaths(): void {
    collapseSet(this.rescanDirectories);
    collapseSet(this.deletedPrefixes);
    for (const file of this.touchedFiles) {
      if (
        hasAncestor(this.rescanDirectories, file) ||
        hasAncestor(this.deletedPrefixes, file)
      ) {
        this.touchedFiles.delete(file);
      }
    }
    for (const directory of this.rescanDirectories) {
      if (hasAncestor(this.deletedPrefixes, directory)) {
        this.rescanDirectories.delete(directory);
      }
    }
  }
}

function collapseSet(paths: Set<string>): void {
  const sorted = [...paths].sort((left, right) => left.length - right.length);
  for (const path of sorted) {
    if (hasAncestor(paths, path, path)) {
      paths.delete(path);
    }
  }
}

function hasAncestor(
  paths: Set<string>,
  target: string,
  exclude?: string,
): boolean {
  for (const path of paths) {
    if (path === exclude || path === target) {
      continue;
    }
    const fromPath = relative(path, target);
    if (
      !isAbsolute(fromPath) &&
      fromPath !== ".." &&
      !fromPath.startsWith(`..${sep}`)
    ) {
      return true;
    }
  }
  return false;
}
