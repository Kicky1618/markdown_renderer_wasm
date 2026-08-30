import { SemanticJournal } from "./semantic-journal.mjs";

export class SemanticJournalVerificationError extends Error {
  constructor(verification) {
    super(`semantic journal verification failed: ${verification.errors.join("; ")}`);
    this.name = "SemanticJournalVerificationError";
    this.verification = verification;
  }
}

export function replaySemanticJournal(text, {
  verify = true,
  includeEntries = false,
} = {}) {
  if (typeof text !== "string") throw new TypeError("journal text must be a string");
  const journal = SemanticJournal.fromNDJSON(text);
  const verification = journal.verify();
  if (verify && !verification.ok) throw new SemanticJournalVerificationError(verification);
  const state = journal.replayState({ verify: false });
  return {
    verification,
    state,
    entries: includeEntries ? journal.snapshot() : undefined,
  };
}
