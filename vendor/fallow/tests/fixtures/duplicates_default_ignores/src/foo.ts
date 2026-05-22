export function calculateTotal(items: number[]): number {
  let total = 0;
  for (const item of items) {
    if (item > 0) {
      total += item;
    }
  }
  return total;
}

export function renderLabel(name: string): string {
  const trimmed = name.trim();
  if (trimmed.length === 0) {
    return "Untitled";
  }
  return trimmed.toUpperCase();
}
