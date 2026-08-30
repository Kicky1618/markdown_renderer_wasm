const KEY = /^[A-Za-z_][A-Za-z0-9_.-]{0,63}$/;
const SENSITIVE = /(?:password|passwd|secret|token|api[-_.]?key|credential|auth)/i;
const PATCH_ALLOWED = new Set([
  "label", "title", "value", "unit", "trend", "min", "max", "step",
  "options", "values", "placeholder", "when", "height", "width",
]);
const STATE_META = new Set(["type", "id", "label", "title"]);

function issue(code, detail, id = "") {
  return { code, detail: String(detail || "").slice(0, 160), id: String(id || "").slice(0, 64) };
}

export function sensitiveStateKey(key) {
  return SENSITIVE.test(String(key || ""));
}

export function parseSafeAction(action) {
  const source = String(action || "").trim();
  if (!source) return { ok: false, issue: issue("action-empty", "empty action") };
  const parts = source.split(":");
  const verb = parts.shift() || "";
  if (verb === "llm") {
    const instruction = parts.join(":").trim().slice(0, 2000);
    return { ok: true, verb, instruction: instruction || "Continue the current application using the latest state." };
  }
  if (verb !== "set" && verb !== "increment" && verb !== "decrement") {
    return { ok: false, issue: issue("action-verb", `blocked action verb: ${verb || "(empty)"}`) };
  }
  const key = String(parts.shift() || "").trim();
  if (!KEY.test(key)) return { ok: false, issue: issue("action-key", `invalid state key: ${key || "(empty)"}`) };
  if (SENSITIVE.test(key)) return { ok: false, issue: issue("action-sensitive", `sensitive state key: ${key}`) };
  return { ok: true, verb, key, raw: parts.join(":").slice(0, 512) };
}

export function auditUiConfig(config = {}, { closed = true } = {}) {
  if (!closed || !config?.type) return [];
  const id = config.id || config.target || "";
  const out = [];

  if ((config.type === "button" || config.type === "form") && config.action) {
    const parsed = parseSafeAction(config.action);
    if (!parsed.ok) out.push({ ...parsed.issue, id });
  }

  if (config.type === "state") {
    let accepted = 0;
    for (const [rawKey, rawValue] of Object.entries(config)) {
      const key = String(rawKey);
      if (STATE_META.has(key)) continue;
      if (!KEY.test(key)) { out.push(issue("state-key", `invalid state key: ${key}`, id)); continue; }
      if (SENSITIVE.test(key)) { out.push(issue("state-sensitive", `blocked sensitive state key: ${key}`, id)); continue; }
      if (String(rawValue ?? "").trim() === "") { out.push(issue("state-empty", `ignored empty state value: ${key}`, id)); continue; }
      accepted += 1;
      if (accepted > 32) { out.push(issue("state-limit", "state patch exceeds 32 entries", id)); break; }
    }
  }

  if (config.type === "patch") {
    const target = String(config.target || "").trim();
    if (!KEY.test(target)) out.push(issue("patch-target", `invalid patch target: ${target || "(empty)"}`, id));
    for (const key of Object.keys(config)) {
      if (key === "type" || key === "target" || PATCH_ALLOWED.has(key)) continue;
      out.push(issue("patch-field", `blocked patch field: ${key}`, id));
    }
  }

  return out.slice(0, 64);
}

export function summarizePolicy(configs = [], maxIssues = 64) {
  const issues = [];
  for (const item of configs) {
    const config = item?.config ?? item;
    const closed = item?.closed ?? true;
    for (const value of auditUiConfig(config, { closed })) {
      issues.push(value);
      if (issues.length >= maxIssues) return issues;
    }
  }
  return issues;
}
