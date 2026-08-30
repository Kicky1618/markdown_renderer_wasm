const MAX_TOKENS = 256;
const MAX_DEPTH = 32;

function tokenize(source) {
  const tokens = [];
  let i = 0;
  while (i < source.length && tokens.length < MAX_TOKENS) {
    const c = source[i];
    if (/\s/.test(c)) { i += 1; continue; }
    const two = source.slice(i, i + 2);
    if (["&&", "||", "==", "!=", "<=", ">="].includes(two)) {
      tokens.push({ type: "op", value: two });
      i += 2;
      continue;
    }
    if ("+-*/%()!<>".includes(c)) {
      tokens.push({ type: c === "(" || c === ")" ? "paren" : "op", value: c });
      i += 1;
      continue;
    }
    if (c === '"' || c === "'") {
      const quote = c;
      let value = "";
      i += 1;
      let closed = false;
      while (i < source.length) {
        const ch = source[i++];
        if (ch === quote) { closed = true; break; }
        if (ch === "\\" && i < source.length) value += source[i++];
        else value += ch;
      }
      if (!closed) throw new Error("unterminated string");
      tokens.push({ type: "literal", value });
      continue;
    }
    const number = source.slice(i).match(/^(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/);
    if (number) {
      tokens.push({ type: "literal", value: Number(number[0]) });
      i += number[0].length;
      continue;
    }
    const identifier = source.slice(i).match(/^[A-Za-z_][A-Za-z0-9_.]*/);
    if (identifier) {
      const word = identifier[0];
      if (word === "true") tokens.push({ type: "literal", value: true });
      else if (word === "false") tokens.push({ type: "literal", value: false });
      else if (word === "null") tokens.push({ type: "literal", value: null });
      else tokens.push({ type: "identifier", value: word });
      i += word.length;
      continue;
    }
    throw new Error(`unsupported token at ${i}`);
  }
  if (i < source.length) throw new Error("expression too long");
  tokens.push({ type: "eof", value: "" });
  return tokens;
}

function numeric(value) {
  const result = Number(value);
  return Number.isFinite(result) ? result : 0;
}

export function evaluateExpression(source, state) {
  if (typeof source !== "string" || !source.trim()) return undefined;
  const tokens = tokenize(source);
  let index = 0;
  let depth = 0;
  const peek = () => tokens[index];
  const take = () => tokens[index++];
  const match = value => peek().value === value && (take(), true);
  const enter = fn => {
    if (++depth > MAX_DEPTH) throw new Error("expression nesting too deep");
    try { return fn(); } finally { depth -= 1; }
  };

  const primary = () => enter(() => {
    const token = take();
    if (token.type === "literal") return token.value;
    if (token.type === "identifier") return state.get(token.value);
    if (token.value === "(") {
      const value = logicalOr();
      if (!match(")")) throw new Error("missing )");
      return value;
    }
    throw new Error("expected value");
  });
  const unary = () => {
    if (match("!")) return !unary();
    if (match("-")) return -numeric(unary());
    if (match("+")) return numeric(unary());
    return primary();
  };
  const multiplicative = () => {
    let value = unary();
    while (["*", "/", "%"].includes(peek().value)) {
      const op = take().value;
      const rhs = numeric(unary());
      const lhs = numeric(value);
      value = op === "*" ? lhs * rhs : op === "/" ? lhs / rhs : lhs % rhs;
    }
    return value;
  };
  const additive = () => {
    let value = multiplicative();
    while (["+", "-"].includes(peek().value)) {
      const op = take().value;
      const rhs = multiplicative();
      if (op === "+" && (typeof value === "string" || typeof rhs === "string")) value = String(value ?? "") + String(rhs ?? "");
      else value = op === "+" ? numeric(value) + numeric(rhs) : numeric(value) - numeric(rhs);
    }
    return value;
  };
  const comparison = () => {
    let value = additive();
    while (["<", ">", "<=", ">="].includes(peek().value)) {
      const op = take().value;
      const rhs = additive();
      if (op === "<") value = value < rhs;
      else if (op === ">") value = value > rhs;
      else if (op === "<=") value = value <= rhs;
      else value = value >= rhs;
    }
    return value;
  };
  const equality = () => {
    let value = comparison();
    while (["==", "!="].includes(peek().value)) {
      const op = take().value;
      const rhs = comparison();
      value = op === "==" ? value === rhs : value !== rhs;
    }
    return value;
  };
  const logicalAnd = () => {
    let value = equality();
    while (match("&&")) {
      const rhs = equality();
      value = Boolean(value) && Boolean(rhs);
    }
    return value;
  };
  const logicalOr = () => {
    let value = logicalAnd();
    while (match("||")) {
      const rhs = logicalAnd();
      value = Boolean(value) || Boolean(rhs);
    }
    return value;
  };

  const result = logicalOr();
  if (peek().type !== "eof") throw new Error("trailing expression input");
  return result;
}

export function safeEvaluate(source, state, fallback = undefined) {
  try {
    const value = evaluateExpression(source, state);
    return typeof value === "number" && !Number.isFinite(value) ? fallback : value;
  } catch {
    return fallback;
  }
}
