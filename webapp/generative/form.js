function cleanText(value, fallback, max = 80) {
  const text = String(value ?? "").trim();
  return (text || fallback).slice(0, max);
}

export function formSpec(config = {}) {
  return {
    id: cleanText(config.id, "", 64),
    title: cleanText(config.title || config.label, "Generated form"),
    submit: cleanText(config.submit, "Apply", 48),
    action: cleanText(config.action, "", 160),
  };
}
