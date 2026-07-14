import { watch, type FSWatcher } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import { basename, isAbsolute, join, relative, sep } from "node:path";
import { ChangeSet, type ChangeSetSnapshot } from "./change-set.js";


export type WatchManagerOptions = {
  root: string;
  onChanges: (changes: ChangeSetSnapshot, reason: "watch" | "reconcile") => void | Promise<void>;
  debounceMs?: number;
  maxWaitMs?: number;
  reconcileIntervalMs?: number;
  maxChangedPaths?: number;
  watchFactory?: typeof watch;
  onPendingChange?: (pending: boolean) => void;
  resumeCheckIntervalMs?: number;
  resumeThresholdMs?: number;
};


export class WatchManager {
  private readonly watchers = new Set<FSWatcher>();
  private readonly watchedDirectories = new Set<string>();
  private readonly directoryWatchers = new Map<string, FSWatcher>();
  private readonly pendingRecords = new Set<Promise<void>>();
  private readonly inflightFlushes = new Set<Promise<void>>();
  private changes: ChangeSet;
  private debounceTimer?: ReturnType<typeof setTimeout>;
  private maxWaitTimer?: ReturnType<typeof setTimeout>;
  private reconcileTimer?: ReturnType<typeof setInterval>;
  private resumeTimer?: ReturnType<typeof setInterval>;
  private closed = false;
  private reconcileRequested = false;
  private lastResumeCheckAt = Date.now();


  constructor(private readonly options: WatchManagerOptions) {
    this.changes = this.newChangeSet();
  }


  start(): void {
    if (this.watchers.size > 0 || this.closed) {
      return;
    }
    const factory = this.options.watchFactory ?? watch;
    this.startPrimaryWatcher(factory);
    const intervalMs = this.options.reconcileIntervalMs ?? 60 * 60_000;
    if (intervalMs > 0) {
      this.reconcileTimer = setInterval(() => this.queueFullReconcile(), intervalMs);
      this.reconcileTimer.unref?.();
    }
    const resumeCheckIntervalMs = this.options.resumeCheckIntervalMs ?? 30_000;
    if (resumeCheckIntervalMs > 0) {
      this.lastResumeCheckAt = Date.now();
      this.resumeTimer = setInterval(() => this.checkForResume(), resumeCheckIntervalMs);
      this.resumeTimer.unref?.();
    }
  }


  async close(): Promise<void> {
    if (this.closed) {
      return;
    }
    this.closed = true;
    for (const watcher of this.watchers) watcher.close();
    this.watchers.clear();
    this.watchedDirectories.clear();
    this.directoryWatchers.clear();
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    if (this.maxWaitTimer) clearTimeout(this.maxWaitTimer);
    if (this.reconcileTimer) clearInterval(this.reconcileTimer);
    if (this.resumeTimer) clearInterval(this.resumeTimer);
    await Promise.allSettled([...this.pendingRecords]);
    await Promise.allSettled([...this.inflightFlushes]);
    this.options.onPendingChange?.(false);
  }


  async flushPending(): Promise<void> {
    await Promise.allSettled([...this.pendingRecords]);
    await Promise.allSettled([...this.inflightFlushes]);
    await this.startFlush();
  }


  checkForResume(now = Date.now()): void {
    const thresholdMs = this.options.resumeThresholdMs ?? 90_000;
    if (now - this.lastResumeCheckAt > thresholdMs) {
      this.queueFullReconcile();
    }
    this.lastResumeCheckAt = now;
  }


  private async recordPath(path: string, eventType: string, factory: typeof watch): Promise<void> {
    const pathFromRoot = relative(this.options.root, path);
    if (pathFromRoot.split(sep).some((segment) => segment === ".git" || segment === ".zvec-grep")) {
      return;
    }
    const info = await stat(path).catch(() => null);
    if (this.closed) {
      return;
    }
    if (info?.isDirectory() && eventType === "rename") {
      void this.watchDirectoryTree(path, factory);
    } else if (!info && eventType === "rename") {
      this.removeDirectoryWatchers(path);
    }
    this.changes.add(path, info ? (eventType === "rename" ? "created" : "changed") : "deleted", info?.isDirectory());
    this.scheduleFlush();
  }


  private queueRecord(path: string, eventType: string, factory: typeof watch): void {
    const pending = this.recordPath(path, eventType, factory).finally(() => {
      this.pendingRecords.delete(pending);
    });
    this.pendingRecords.add(pending);
  }


  private queueFullReconcile(): void {
    this.changes.requireFullReconcile();
    this.reconcileRequested = true;
    this.scheduleFlush();
  }


  private scheduleFlush(): void {
    if (this.closed) {
      return;
    }
    this.options.onPendingChange?.(true);
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
    }
    this.debounceTimer = setTimeout(() => void this.startFlush(), this.options.debounceMs ?? 750);
    this.debounceTimer.unref?.();
    if (!this.maxWaitTimer) {
      this.maxWaitTimer = setTimeout(() => void this.startFlush(), this.options.maxWaitMs ?? 5_000);
      this.maxWaitTimer.unref?.();
    }
  }


  private async flush(): Promise<void> {
    if (this.closed || this.changes.empty) {
      return;
    }
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    if (this.maxWaitTimer) clearTimeout(this.maxWaitTimer);
    this.debounceTimer = undefined;
    this.maxWaitTimer = undefined;
    const changes = this.changes.snapshot();
    const reason = this.reconcileRequested ? "reconcile" : "watch";
    this.changes = this.newChangeSet();
    this.reconcileRequested = false;
    try {
      await this.options.onChanges(changes, reason);
      this.options.onPendingChange?.(false);
    } catch {
      this.changes.merge({ ...changes, forceFullReconcile: true });
      this.reconcileRequested = true;
      this.scheduleFlush();
    }
  }


  private newChangeSet(): ChangeSet {
    return new ChangeSet({ maxChangedPaths: this.options.maxChangedPaths });
  }


  private startFlush(): Promise<void> {
    const flush = this.flush().finally(() => this.inflightFlushes.delete(flush));
    this.inflightFlushes.add(flush);
    return flush;
  }


  private addWatcher(watcher: FSWatcher, factory: typeof watch, directory?: string): void {
    this.watchers.add(watcher);
    if (directory) this.directoryWatchers.set(directory, watcher);
    watcher.on("error", () => {
      watcher.close();
      this.watchers.delete(watcher);
      if (directory) {
        this.directoryWatchers.delete(directory);
        this.watchedDirectories.delete(directory);
      }
      this.queueFullReconcile();
      if (!this.closed) {
        const retry = setTimeout(() => {
          if (directory) {
            void this.watchDirectoryTree(directory, factory);
          } else {
            this.startPrimaryWatcher(factory);
          }
        }, 100);
        retry.unref?.();
      }
    });
  }


  private startPrimaryWatcher(factory: typeof watch): void {
    if (this.closed) {
      return;
    }
    try {
      this.addWatcher(factory(this.options.root, { recursive: true }, (eventType, filename) => {
        if (!filename || this.closed) {
          this.queueFullReconcile();
          return;
        }
        this.queueRecord(join(this.options.root, filename.toString()), eventType, factory);
      }), factory);
    } catch {
      void this.watchDirectoryTree(this.options.root, factory);
    }
  }


  private async watchDirectoryTree(directory: string, factory: typeof watch): Promise<void> {
    if (
      this.closed
      || this.watchedDirectories.has(directory)
      || basename(directory) === ".git"
      || basename(directory) === ".zvec-grep"
    ) {
      return;
    }
    this.watchedDirectories.add(directory);
    try {
      this.addWatcher(factory(directory, { recursive: false }, (eventType, filename) => {
        if (!filename || this.closed) {
          this.queueFullReconcile();
          return;
        }
        this.queueRecord(join(directory, filename.toString()), eventType, factory);
      }), factory, directory);
    } catch {
      this.watchedDirectories.delete(directory);
      this.queueFullReconcile();
      return;
    }
    const entries = await readdir(directory, { withFileTypes: true }).catch(() => []);
    await Promise.all(entries
      .filter((entry) => entry.isDirectory())
      .map((entry) => this.watchDirectoryTree(join(directory, entry.name), factory)));
  }


  private removeDirectoryWatchers(path: string): void {
    for (const [directory, watcher] of this.directoryWatchers) {
      const pathFromDeleted = relative(path, directory);
      if (
        pathFromDeleted === ""
        || (!isAbsolute(pathFromDeleted)
          && !pathFromDeleted.startsWith(`..${sep}`)
          && pathFromDeleted !== "..")
      ) {
        watcher.close();
        this.watchers.delete(watcher);
        this.directoryWatchers.delete(directory);
        this.watchedDirectories.delete(directory);
      }
    }
  }
}
