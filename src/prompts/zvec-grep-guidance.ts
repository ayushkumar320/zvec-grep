export const ZVEC_GREP_WORKSPACE_EVIDENCE_RULES = [
  "Treat the current workspace as an intended evidence source when the user asks to inspect, search, or ground the answer in local material; prior context established the workspace as the source; the user asks whether relevant local material exists; or the agent is operating inside a repository or project and the question asks how, where, or why its implementation, symbols, call chains, dependencies, lifecycle, data flow, architecture, or interactions work.",
  "For an implementation-specific question about the current checkout, do not require the user to explicitly say workspace, repository, project, codebase, index, or local files.",
  "A workspace may contain source code, documentation, books, research material, meeting notes, knowledge-base exports, manuals, configuration, data, or mixed content.",
  "Do not use workspace retrieval for unrelated open-world questions, current external facts, or web content that does not depend on local evidence.",
] as const;

export function formatPromptRules(
  heading: string,
  rules: readonly string[],
): string {
  return [heading, ...rules.map((rule) => `- ${rule}`)].join("\n");
}
