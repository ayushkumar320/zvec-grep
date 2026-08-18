import type { FileScanDiagnostics, IndexProgress } from "../engine/types.js";
import { ChangeSet, type ChangeSetSnapshot } from "./change-set.js";
import type { JobScheduler, IndexJobSnapshot } from "./job-scheduler.js";
import type { RootRuntime } from "./root-runtime.js";

export type IndexCoordinatorOptions = {
  runtime: RootRuntime;
  scheduler: JobScheduler;
  run: (
    changes: ChangeSetSnapshot,
    report: (progress: IndexProgress) => void,
    signal: AbortSignal,
  ) => Promise<IndexReconciliationProof | void>;
  getIndexedFileCount?: () => number | undefined;
  fullReconcileRatio?: number;
  minRatioChangedPaths?: number;
};

export type IndexReconciliationProof = {
  reconciled: boolean;
  reconciliationEpoch: number;
  scanDiagnostics?: FileScanDiagnostics;
};

export class IndexCoordinator {
  private pending = new ChangeSet();
  private targetRevision = 0;

  constructor(private readonly options: IndexCoordinatorOptions) {}

  enqueue(
    changes: ChangeSetSnapshot,
    reason: "watch" | "reconcile" = "watch",
  ): IndexJobSnapshot {
    const indexedFiles = this.options.getIndexedFileCount?.();
    const changedPathCount =
      changes.touchedFiles.length +
      changes.rescanDirectories.length +
      changes.deletedPrefixes.length;
    if (
      indexedFiles &&
      changedPathCount >= (this.options.minRatioChangedPaths ?? 10) &&
      changedPathCount / indexedFiles > (this.options.fullReconcileRatio ?? 0.2)
    ) {
      changes = { ...changes, forceFullReconcile: true };
    }
    if (changes.forceFullReconcile) {
      this.options.runtime.requireFullReconciliation();
    }
    this.pending.merge(changes);
    this.targetRevision = this.options.runtime.markDirty();
    let jobChanges: ChangeSetSnapshot | undefined;
    let jobRevision = 0;
    const submitted = this.options.scheduler.submit({
      canonicalRoot: this.options.runtime.canonicalRoot,
      reason,
      followupIfRunning: true,
      run: async (report, signal) => {
        if (!jobChanges) {
          jobChanges = this.pending.snapshot();
          jobRevision = this.targetRevision;
          this.pending = new ChangeSet();
        }
        if (
          !jobChanges.forceFullReconcile &&
          jobChanges.touchedFiles.length === 0 &&
          jobChanges.rescanDirectories.length === 0 &&
          jobChanges.deletedPrefixes.length === 0
        ) {
          this.options.runtime.markIndexed(jobRevision);
          return;
        }
        const proof = await this.options.run(jobChanges, report, signal);
        if (jobChanges.forceFullReconcile) {
          if (proof?.reconciled === true) {
            this.options.runtime.markReconciled(
              jobRevision,
              proof.reconciliationEpoch,
            );
          } else {
            this.options.runtime.markIndexed(jobRevision);
          }
        } else {
          this.options.runtime.markIndexed(jobRevision);
        }
      },
    });
    return submitted.job;
  }
}
