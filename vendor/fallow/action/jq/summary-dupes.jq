if .stats.clone_groups == 0 then
  "## Fallow — Code Duplication\n\nNo code duplication found.\n\n*Analyzed \(.stats.total_files) files in \(.elapsed_ms)ms*"
else
  "## Fallow — Code Duplication\n\nFound **\(.stats.clone_groups) clone groups** (\(.stats.clone_instances) instances) across \(.stats.files_with_clones) files in \(.elapsed_ms)ms\n\n" +
  "| Metric | Value |\n|--------|-------|\n" +
  "| Files analyzed | \(.stats.total_files) |\n" +
  "| Files with clones | \(.stats.files_with_clones) |\n" +
  "| Clone groups | \(.stats.clone_groups) |\n" +
  "| Clone instances | \(.stats.clone_instances) |\n" +
  "| Duplicated lines | \(.stats.duplicated_lines) / \(.stats.total_lines) (\((.stats.duplication_percentage // 0) | . * 10 | round / 10)%) |\n" +
  "\n<details>\n<summary>View details</summary>\n\n" +
  (if (.clone_families | length) > 0 then
    "**Clone Families (\(.clone_families | length))**\n\n" +
    ([.clone_families[:15][] |
      "- **\(.files[:3] | join(", "))\(if (.files | length) > 3 then " (+\((.files | length) - 3) more)" else "" end)** — \(.total_duplicated_lines) lines, \(.groups | length) groups" +
      (if (.groups | length) > 0 and ((.groups[0].instances // []) | length) > 0 then
        "\n  - " + ([(.groups[0].instances // [])[] | "`\(.file):\(.start_line)-\(.end_line)`"] | join(", "))
      else "" end) +
      (if (.suggestions | length) > 0 then
        "\n" + ([.suggestions[] | "  - \(.description) (~\(.estimated_savings) lines)"] | join("\n"))
      else "" end)
    ] | join("\n")) +
    (if (.clone_families | length) > 15 then "\n- *... and \((.clone_families | length) - 15) more families*" else "" end)
  else
    ((.clone_groups // []) | sort_by([(.line_count // 0), (.token_count // 0)]) | reverse) as $sorted |
    ([$sorted[:20][] |
      ([(.instances // [])[] | "`\(.file):\(.start_line)-\(.end_line)`"] | join(", ")) as $locs |
      "- **\(.line_count) lines, \(.token_count) tokens**, \($locs)"
    ] | join("\n")) +
    (if (.clone_groups | length) > 20 then "\n- *... and \((.clone_groups | length) - 20) more groups*" else "" end)
  end) +
  "\n\n</details>"
end
