function contentTypeOf(response) {
  return String(response?.headers?.get?.("content-type") || "").toLowerCase();
}

export function detectStreamFormat(response, requested = "auto") {
  if (["plain", "sse", "ndjson"].includes(requested)) return requested;
  const type = contentTypeOf(response);
  if (type.includes("text/event-stream")) return "sse";
  if (type.includes("ndjson") || type.includes("jsonl") || type.includes("x-ndjson")) return "ndjson";
  return "plain";
}

function contentText(value) {
  if (typeof value === "string") return value;
  if (!Array.isArray(value)) return "";
  return value.map(item => {
    if (typeof item === "string") return item;
    if (typeof item?.text === "string") return item.text;
    if (typeof item?.content === "string") return item.content;
    return "";
  }).join("");
}

/** Extract a text delta from common LLM streaming JSON envelopes. */
export function extractDeltaText(value) {
  if (typeof value === "string") return value;
  if (!value || typeof value !== "object") return "";

  const choice = value.choices?.[0];
  const choiceDelta = choice?.delta;
  const candidates = [
    typeof choiceDelta === "string" ? choiceDelta : undefined,
    choiceDelta?.content,
    choiceDelta?.text,
    choice?.text,
    typeof value.delta === "string" ? value.delta : undefined,
    value.delta?.text,
    value.delta?.content,
    value.output_text,
    value.text,
    value.message?.content,
    value.content,
  ];
  for (const candidate of candidates) {
    const text = contentText(candidate);
    if (text) return text;
  }
  return "";
}

export function decodeSseEvent(event) {
  const data = String(event)
    .split(/\r?\n/)
    .filter(line => line.startsWith("data:"))
    .map(line => line.slice(5).trimStart())
    .join("\n");
  if (!data) return { text: "", done: false };
  if (data.trim() === "[DONE]") return { text: "", done: true };
  try {
    return { text: extractDeltaText(JSON.parse(data)), done: false };
  } catch {
    return { text: data, done: false };
  }
}

export function decodeNdjsonLine(line) {
  const text = String(line).trim();
  if (!text) return { text: "", done: false };
  if (text === "[DONE]") return { text: "", done: true };
  try {
    return { text: extractDeltaText(JSON.parse(text)), done: false };
  } catch {
    return { text, done: false };
  }
}

function throwIfAborted(signal) {
  if (!signal?.aborted) return;
  if (signal.reason !== undefined) throw signal.reason;
  const error = new Error("stream aborted");
  error.name = "AbortError";
  throw error;
}

/**
 * Consume a fetch Response and emit only Markdown/text payload chunks.
 * No credentials or headers are managed here; callers own the fetch policy.
 */
export async function consumeHttpResponse(response, {
  format = "auto",
  onText,
  signal,
} = {}) {
  if (!response?.ok) throw new Error(`stream request failed: HTTP ${response?.status ?? "?"}`);
  if (typeof onText !== "function") throw new TypeError("onText callback is required");
  const selected = detectStreamFormat(response, format);
  const body = response.body;
  if (!body?.getReader) {
    const text = await response.text();
    if (text) onText(text);
    return { format: selected, chunks: text ? 1 : 0, chars: text.length };
  }

  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let chunks = 0;
  let chars = 0;
  let doneEnvelope = false;

  const emit = text => {
    if (!text) return;
    onText(text);
    chunks += 1;
    chars += text.length;
  };
  const processSse = final => {
    buffer = buffer.replace(/\r\n/g, "\n");
    const events = buffer.split("\n\n");
    buffer = final ? "" : events.pop() ?? "";
    for (const event of events) {
      const decoded = decodeSseEvent(event);
      emit(decoded.text);
      if (decoded.done) doneEnvelope = true;
    }
    if (final && buffer) {
      const decoded = decodeSseEvent(buffer);
      emit(decoded.text);
      if (decoded.done) doneEnvelope = true;
      buffer = "";
    }
  };
  const processNdjson = final => {
    buffer = buffer.replace(/\r\n/g, "\n");
    const lines = buffer.split("\n");
    buffer = final ? "" : lines.pop() ?? "";
    for (const line of lines) {
      const decoded = decodeNdjsonLine(line);
      emit(decoded.text);
      if (decoded.done) doneEnvelope = true;
    }
    if (final && buffer) {
      const decoded = decodeNdjsonLine(buffer);
      emit(decoded.text);
      if (decoded.done) doneEnvelope = true;
      buffer = "";
    }
  };

  try {
    while (!doneEnvelope) {
      throwIfAborted(signal);
      const { done, value } = await reader.read();
      if (done) break;
      const text = decoder.decode(value, { stream: true });
      if (selected === "plain") emit(text);
      else {
        buffer += text;
        if (selected === "sse") processSse(false);
        else processNdjson(false);
      }
    }
    const tail = decoder.decode();
    if (selected === "plain") emit(tail);
    else {
      buffer += tail;
      if (selected === "sse") processSse(true);
      else processNdjson(true);
    }
  } finally {
    reader.releaseLock();
  }
  throwIfAborted(signal);
  return { format: selected, chunks, chars };
}
