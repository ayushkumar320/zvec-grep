import type { CliOptions } from "../cli/types.js";

export type ServerSearchPolicy = {
  freshness: "eventual" | "wait_for_fresh";
  autoUpdate: boolean;
};

export function resolveServerSearchPolicy(
  options: Pick<CliOptions, "fresh" | "noAutoUpdate">,
): ServerSearchPolicy {
  return {
    freshness: options.fresh ? "wait_for_fresh" : "eventual",
    autoUpdate: options.noAutoUpdate !== true,
  };
}
