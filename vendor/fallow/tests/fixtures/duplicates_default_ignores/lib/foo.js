export function calculateTotal(items) {
  let total = 0;
  for (const item of items) {
    if (item > 0) {
      total += item;
    }
  }
  return total;
}

export function renderLabel(name) {
  const trimmed = name.trim();
  if (trimmed.length === 0) {
    return "Untitled";
  }
  return trimmed.toUpperCase();
}
