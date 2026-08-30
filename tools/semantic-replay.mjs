#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { stdin } from "node:process";
import { replaySemanticJournal, SemanticJournalVerificationError } from "./semantic-replay-core.mjs";

function usage() {
  console.error(`Usage: node tools/semantic-replay.mjs [journal.ndjson] [options]

Options:
  --state-only   Print only the replayed state/revision snapshot
  --entries      Include parsed journal entries in JSON output
  --no-verify    Replay even when journal verification fails
  --help         Show this help

If the file is omitted, NDJSON is read from stdin.`);
}

function parseArgs(argv) {
  const options = { file: null, stateOnly: false, entries: false, verify: true };
  for (const arg of argv) {
    if (arg === "--help") {
      usage();
      process.exit(0);
    }
    if (arg === "--state-only") {
      options.stateOnly = true;
      continue;
    }
    if (arg === "--entries") {
      options.entries = true;
      continue;
    }
    if (arg === "--no-verify") {
      options.verify = false;
      continue;
    }
    if (arg.startsWith("--")) throw new Error(`unknown option: ${arg}`);
    if (options.file !== null) throw new Error("only one journal file may be specified");
    options.file = arg;
  }
  return options;
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of stdin) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}

try {
  const options = parseArgs(process.argv.slice(2));
  const text = options.file === null ? await readStdin() : await readFile(options.file, "utf8");
  const replay = replaySemanticJournal(text, {
    verify: options.verify,
    includeEntries: options.entries,
  });

  if (options.stateOnly) console.log(JSON.stringify(replay.state, null, 2));
  else console.log(JSON.stringify(replay, null, 2));
} catch (error) {
  console.error(`semantic replay: ${error.message}`);
  process.exitCode = error instanceof SemanticJournalVerificationError ? 3 : 2;
}
