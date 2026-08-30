function words(value) {
  return String(value || "")
    .split(",")
    .map(item => item.trim())
    .filter(Boolean)
    .slice(0, 8);
}

function slug(value, fallback) {
  const slugged = String(value || "")
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40);
  return slugged || fallback;
}

export function tabsSpec(config) {
  const labels = words(config.labels || config.tabs);
  const explicit = words(config.values);
  const count = Math.min(8, Math.max(labels.length, explicit.length, 1));
  const items = Array.from({ length: count }, (_, index) => {
    const label = labels[index] || explicit[index] || `Tab ${index + 1}`;
    const value = explicit[index] || slug(label, `tab-${index + 1}`);
    return { label: label.slice(0, 48), value };
  });
  const values = new Set(items.map(item => item.value));
  const initial = values.has(config.value) ? config.value : items[0].value;
  return {
    id: slug(config.id, "tabs"),
    state: slug(config.state || `${config.id || "tabs"}.active`, "tabs-active"),
    title: String(config.title || "Generated views").slice(0, 80),
    items,
    initial,
  };
}

export function tabFor(config, spec) {
  const value = String(config?.tab || "").trim();
  return spec.items.some(item => item.value === value) ? value : spec.items[0].value;
}
