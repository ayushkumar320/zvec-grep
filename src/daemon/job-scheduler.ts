import { randomUUID } from "node:crypto";
import type { IndexProgress } from "../engine/types.js";
import { DaemonError } from "./errors.js";


export type JobState = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export type JobReason = "watch" | "reconcile" | "manual" | "fresh_query";

export type IndexJobSnapshot = {
  id: string;
  canonicalRoot: string;
  reason: JobReason;
  state: JobState;
  attempt: number;
  createdAt: number;
  startedAt?: number;
  finishedAt?: number;
  progress?: IndexProgress;
  error?: { code: string; message: string };
};

export type SubmitIndexJob = {
  canonicalRoot: string;
  reason: JobReason;
  run: (report: (progress: IndexProgress) => void) => Promise<void>;
  followupIfRunning?: boolean;
};

export type SubmitIndexJobResult = {
  job: IndexJobSnapshot;
  reused: boolean;
};

export type JobSchedulerOptions = {
  concurrency?: number;
  maxAttempts?: number;
  retryBaseDelayMs?: number;
};

type ScheduledJob = IndexJobSnapshot & {
  run: SubmitIndexJob["run"];
  completion: Promise<IndexJobSnapshot>;
  resolveCompletion: (snapshot: IndexJobSnapshot) => void;
  retryTimer?: ReturnType<typeof setTimeout>;
  followup?: ScheduledJob;
};


export class JobScheduler {
  private readonly jobs = new Map<string, ScheduledJob>();
  private readonly activeByRoot = new Map<string, ScheduledJob>();
  private readonly latestByRoot = new Map<string, ScheduledJob>();
  private readonly queue: ScheduledJob[] = [];
  private readonly concurrency: number;
  private readonly maxAttempts: number;
  private readonly retryBaseDelayMs: number;
  private running = 0;
  private closed = false;
  private closePromise?: Promise<void>;
  private closeResolve?: () => void;


  constructor(options: JobSchedulerOptions = {}) {
    this.concurrency = options.concurrency ?? 1;
    this.maxAttempts = options.maxAttempts ?? 3;
    this.retryBaseDelayMs = options.retryBaseDelayMs ?? 250;
  }


  submit(input: SubmitIndexJob): SubmitIndexJobResult {
    if (this.closed) {
      throw new DaemonError("DAEMON_SHUTTING_DOWN", "The daemon is shutting down.", true);
    }
    const active = this.activeByRoot.get(input.canonicalRoot);
    if (active) {
      if (active.state === "queued" && !active.retryTimer) {
        if (input.reason === "watch" || input.followupIfRunning) {
          active.run = combineRuns(active.run, input.run);
        }
        if (priority(input.reason) > priority(active.reason)) {
          active.reason = input.reason;
          this.sortQueue();
        }
      } else if (input.reason === "watch" || input.followupIfRunning) {
        if (active.followup) {
          mergeQueuedJob(active.followup, input);
        } else {
          active.followup = this.createJob(input);
        }
        return { job: snapshot(active.followup), reused: true };
      }
      return { job: snapshot(active), reused: true };
    }

    const job = this.enqueue(input);
    return { job: snapshot(job), reused: false };
  }


  getByRoot(canonicalRoot: string): IndexJobSnapshot | undefined {
    const job = this.latestByRoot.get(canonicalRoot);
    return job ? snapshot(job) : undefined;
  }


  hasActiveRoot(canonicalRoot: string): boolean {
    return this.activeByRoot.has(canonicalRoot);
  }


  get(jobId: string): IndexJobSnapshot | undefined {
    const job = this.jobs.get(jobId);
    return job ? snapshot(job) : undefined;
  }


  async wait(jobId: string): Promise<IndexJobSnapshot> {
    const job = this.jobs.get(jobId);
    if (!job) {
      throw new Error(`Unknown job: ${jobId}`);
    }
    return job.completion;
  }


  async waitForRootIdle(canonicalRoot: string): Promise<void> {
    while (true) {
      const active = this.activeByRoot.get(canonicalRoot);
      if (!active) {
        return;
      }
      await active.completion;
    }
  }


  snapshot(): { queued: number; running: number } {
    return {
      queued: [...this.jobs.values()].filter((job) => job.state === "queued").length,
      running: this.running,
    };
  }


  async close(): Promise<void> {
    if (this.closed) {
      return this.closePromise;
    }
    this.closed = true;
    for (const job of this.jobs.values()) {
      if (job.retryTimer) {
        clearTimeout(job.retryTimer);
        job.retryTimer = undefined;
      }
      if (job.state === "queued") {
        this.finish(job, "cancelled");
      }
    }
    this.queue.length = 0;
    if (this.running === 0) {
      return;
    }
    this.closePromise = new Promise<void>((resolve) => {
      this.closeResolve = resolve;
    });
    return this.closePromise;
  }


  private pump(): void {
    while (!this.closed && this.running < this.concurrency && this.queue.length > 0) {
      const job = this.queue.shift()!;
      if (job.state !== "queued" || job.retryTimer) {
        continue;
      }
      this.running += 1;
      job.state = "running";
      job.attempt += 1;
      job.error = undefined;
      job.startedAt ??= Date.now();
      void this.runJob(job);
    }
  }


  private async runJob(job: ScheduledJob): Promise<void> {
    try {
      await job.run((progress) => {
        job.progress = { ...progress };
      });
      this.finish(job, "succeeded");
    } catch (error) {
      if (!this.closed && isRetryable(error) && job.attempt < this.maxAttempts) {
        job.state = "queued";
        job.error = errorInfo(error);
        const delay = this.retryBaseDelayMs * (2 ** (job.attempt - 1));
        job.retryTimer = setTimeout(() => {
          job.retryTimer = undefined;
          this.queue.push(job);
          this.sortQueue();
          this.pump();
        }, delay);
        job.retryTimer.unref?.();
      } else {
        job.error = errorInfo(error);
        this.finish(job, this.closed ? "cancelled" : "failed");
      }
    } finally {
      this.running -= 1;
      if (this.closed && this.running === 0) {
        this.closeResolve?.();
      }
      this.pump();
    }
  }


  private finish(job: ScheduledJob, state: Extract<JobState, "succeeded" | "failed" | "cancelled">): void {
    job.state = state;
    job.finishedAt = Date.now();
    if (this.activeByRoot.get(job.canonicalRoot) === job) {
      this.activeByRoot.delete(job.canonicalRoot);
    }
    const followup = !this.closed && state !== "cancelled" && job.followup?.state === "queued"
      ? job.followup
      : undefined;
    job.followup = undefined;
    if (followup) {
      this.activate(followup);
    }
    job.resolveCompletion(snapshot(job));
  }


  private enqueue(input: SubmitIndexJob): ScheduledJob {
    const job = this.createJob(input);
    this.activate(job);
    return job;
  }


  private createJob(input: SubmitIndexJob): ScheduledJob {
    let resolveCompletion!: (value: IndexJobSnapshot) => void;
    const completion = new Promise<IndexJobSnapshot>((resolve) => {
      resolveCompletion = resolve;
    });
    const job: ScheduledJob = {
      id: randomUUID(),
      canonicalRoot: input.canonicalRoot,
      reason: input.reason,
      state: "queued",
      attempt: 0,
      createdAt: Date.now(),
      run: input.run,
      completion,
      resolveCompletion,
    };
    this.jobs.set(job.id, job);
    return job;
  }


  private activate(job: ScheduledJob): void {
    this.activeByRoot.set(job.canonicalRoot, job);
    this.latestByRoot.set(job.canonicalRoot, job);
    this.queue.push(job);
    this.sortQueue();
    this.pump();
  }


  private sortQueue(): void {
    this.queue.sort((left, right) =>
      priority(right.reason) - priority(left.reason) || left.createdAt - right.createdAt);
  }
}


function priority(reason: JobReason): number {
  switch (reason) {
    case "manual": return 4;
    case "fresh_query": return 3;
    case "watch": return 2;
    case "reconcile": return 1;
  }
}


function mergeQueuedJob(current: ScheduledJob, incoming: SubmitIndexJob): void {
  current.run = combineRuns(current.run, incoming.run);
  if (priority(incoming.reason) > priority(current.reason)) {
    current.reason = incoming.reason;
  }
}


function combineRuns(
  first: SubmitIndexJob["run"],
  second: SubmitIndexJob["run"],
): SubmitIndexJob["run"] {
  return async (report) => {
    await first(report);
    await second(report);
  };
}


function snapshot(job: ScheduledJob): IndexJobSnapshot {
  return {
    id: job.id,
    canonicalRoot: job.canonicalRoot,
    reason: job.reason,
    state: job.state,
    attempt: job.attempt,
    createdAt: job.createdAt,
    startedAt: job.startedAt,
    finishedAt: job.finishedAt,
    progress: job.progress ? { ...job.progress } : undefined,
    error: job.error ? { ...job.error } : undefined,
  };
}


function isRetryable(error: unknown): boolean {
  if (error instanceof DaemonError) {
    return error.retryable;
  }
  return Boolean(
    error
    && typeof error === "object"
    && "code" in error
    && error.code === "ZVEC_GREP.ENGINE.LOCK.BUSY",
  );
}


function errorInfo(error: unknown): { code: string; message: string } {
  if (error instanceof DaemonError) {
    return { code: error.code, message: error.message };
  }
  if (error && typeof error === "object" && "code" in error && typeof error.code === "string") {
    return { code: error.code, message: error instanceof Error ? error.message : String(error) };
  }
  return {
    code: "INDEX_FAILED",
    message: error instanceof Error ? error.message : String(error),
  };
}
