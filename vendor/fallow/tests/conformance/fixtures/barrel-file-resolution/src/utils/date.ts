export const formatDate = (d: Date): string => d.toISOString();

export const formatRelative = (d: Date): string => {
  const diff = Date.now() - d.getTime();
  return `${Math.floor(diff / 1000)}s ago`;
};
