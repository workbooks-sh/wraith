(.fixes | map(select(.type == "remove_export")) | length) as $exports |
(.fixes | map(select(.type == "remove_dependency")) | length) as $deps |

if (.fixes | length) == 0 then
  "## Fallow — Auto-fix\n\nNo fixable issues found."
else
  "## Fallow — Auto-fix\n\n" +
  (if .dry_run then "**Dry run**: would apply" else "Applied" end) +
  " **\(.fixes | length) fixes**" +
  (if .dry_run then "" else " (\(.total_fixed) succeeded)" end) + "\n\n" +
  "| Type | Count |\n|------|-------|\n" +
  (if $exports > 0 then "| Export removals | \($exports) |\n" else "" end) +
  (if $deps > 0 then "| Dependency removals | \($deps) |\n" else "" end) +
  "\n<details>\n<summary>View details</summary>\n\n" +
  (if $exports > 0 then
    "**Export removals (\($exports))**\n" +
    ([.fixes | map(select(.type == "remove_export"))[:25][] |
      "- `\(.path):\(.line)` — `\(.name)`"] | join("\n")) +
    (if $exports > 25 then "\n- *... and \($exports - 25) more*" else "" end) +
    "\n\n"
  else "" end) +
  (if $deps > 0 then
    "**Dependency removals (\($deps))**\n" +
    ([.fixes | map(select(.type == "remove_dependency"))[:25][] |
      "- `\(.package)` from \(.location) in `\(.file)`"] | join("\n")) +
    (if $deps > 25 then "\n- *... and \($deps - 25) more*" else "" end) +
    "\n"
  else "" end) +
  "\n\n</details>"
end
