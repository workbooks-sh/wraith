// Remix co-located helper inside `app/routes/<segment>/`. NOT a route file.
// Verifies the route-convention skip is segment-scoped (a single `*` does not
// cross `/`); without `literal_separator(true)` on the globset this file would
// be silently swallowed by `**/routes/*.{ts,tsx,...}`.
type FormatOptions = {
  locale: string;
};

export function formatDate(opts: FormatOptions): string {
  return opts.locale;
}
