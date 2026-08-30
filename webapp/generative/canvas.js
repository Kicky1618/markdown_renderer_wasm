function finiteNumber(value, fallback = 0) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function coordinate(value) {
  return clamp(finiteNumber(value), -10000, 10000);
}

export function canvasSpec(config = {}) {
  return {
    width: Math.round(clamp(finiteNumber(config.width, 640), 160, 1200)),
    height: Math.round(clamp(finiteNumber(config.height, 280), 120, 700)),
    title: String(config.title || config.label || "Generated canvas"),
  };
}

/** Parse a tiny, non-executable scene language emitted by an LLM. */
export function parseCanvasScene(body, maxCommands = 512) {
  const commands = [];
  for (const rawLine of String(body || "").split(/\r?\n/)) {
    if (commands.length >= maxCommands) break;
    const line = rawLine.trim();
    if (!line || line.startsWith("#") || line.includes("=")) continue;
    const [verb, ...parts] = line.split(/\s+/);
    if (verb === "line" && parts.length >= 4) {
      commands.push({ type: "line", x1: coordinate(parts[0]), y1: coordinate(parts[1]), x2: coordinate(parts[2]), y2: coordinate(parts[3]) });
    } else if (verb === "circle" && parts.length >= 3) {
      commands.push({ type: "circle", x: coordinate(parts[0]), y: coordinate(parts[1]), r: clamp(Math.abs(finiteNumber(parts[2])), 0, 10000) });
    } else if (verb === "rect" && parts.length >= 4) {
      commands.push({ type: "rect", x: coordinate(parts[0]), y: coordinate(parts[1]), w: clamp(Math.abs(finiteNumber(parts[2])), 0, 10000), h: clamp(Math.abs(finiteNumber(parts[3])), 0, 10000) });
    } else if (verb === "text" && parts.length >= 3) {
      const x = coordinate(parts.shift());
      const y = coordinate(parts.shift());
      const text = parts.join(" ").slice(0, 240);
      if (text) commands.push({ type: "text", x, y, text });
    }
  }
  return commands;
}
