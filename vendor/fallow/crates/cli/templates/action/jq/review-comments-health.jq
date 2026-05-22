def prefix: $ENV.PREFIX // "";
def root: $ENV.FALLOW_ROOT // ".";
def rel_path: if startswith("/") then (. as $p | root as $r | if ($p | test("/\($r)/")) then ($p | capture("/\($r)/(?<rest>.*)") | .rest) else ($p | split("/") | .[-3:] | join("/")) end) else . end;
def footer: "\n\n---\n<sub><a href=\"https://docs.fallow.tools/explanations/health\">Docs</a> \u00b7 Disagree? <a href=\"https://docs.fallow.tools/configuration/rules\">Configure thresholds</a></sub>";
def prod_footer: "\n\n---\n<sub><a href=\"https://docs.fallow.tools/explanations/health#runtime-coverage\">Docs</a></sub>";
(.summary.max_cyclomatic_threshold // 20) as $cyc_t |
(.summary.max_cognitive_threshold // 15) as $cog_t |
(.summary.max_crap_threshold // 30) as $crap_t |
[
  (.findings[]? | {
    type: "other",
    path: (prefix + (.path | rel_path)),
    line: .line,
    body: (
      (.severity // "moderate") as $sev |
      (if ((.exceeded // "") | test("cyclomatic|both|cyclomatic_crap|all")) then ":red_circle:" else ":white_check_mark:" end) as $cyc_status |
      (if ((.exceeded // "") | test("cognitive|both|cognitive_crap|all")) then ":red_circle:" else ":white_check_mark:" end) as $cog_status |
      (if ((.exceeded // "") | test("crap")) then ":red_circle:" else ":white_check_mark:" end) as $crap_status |
      (if .crap != null then "| [CRAP](https://docs.fallow.tools/explanations/health#crap-score) | **\(.crap)** | \($crap_t) | \($crap_status) |\n" else "" end) as $crap_row |
      (if .name == "<template>" then "Template" else "Function" end) as $subject |
      (if .name == "<template>" then
        "- **Cyclomatic complexity** \u2014 How many independent paths through this template (each control-flow block, bound expression branch, and logical operator adds one). High values mean more branches to test.\n- **Cognitive complexity** \u2014 How hard this template is to read top-to-bottom. Penalizes deeply nested control flow.\n- **CRAP score** \u2014 Change Risk Anti-Patterns: combines complexity with coverage. `CC^2 * (1 - cov/100)^3 + CC`. High CRAP means the template is complex AND poorly tested.\n</details>\n\n**Action:** Simplify the template control flow, extract smaller components, or move complex bindings into named helpers."
      else
        "- **Cyclomatic complexity** \u2014 How many independent paths through this function (each `if`, `switch` case, loop, and `&&`/`||` adds one). High values mean more branches to test.\n- **Cognitive complexity** \u2014 How hard this function is to read top-to-bottom. Penalizes deeply nested logic and jumps in control flow.\n- **CRAP score** \u2014 Change Risk Anti-Patterns: combines complexity with coverage. `CC^2 * (1 - cov/100)^3 + CC`. High CRAP means the function is complex AND poorly tested.\n</details>\n\n**Action:** Break this into smaller functions, each doing one thing. Look for independent blocks of logic that can be extracted with a descriptive name."
      end) as $guidance |
      ":warning: **High complexity** (\($sev))\n\n\($subject) `\(.name)` exceeds complexity thresholds:\n\n| Metric | Value | Threshold | Status |\n|:-------|------:|----------:|:------:|\n| Severity | **\($sev)** | | |\n| [Cyclomatic](https://docs.fallow.tools/explanations/health#cyclomatic-complexity) | **\(.cyclomatic)** | \($cyc_t) | \($cyc_status) |\n| [Cognitive](https://docs.fallow.tools/explanations/health#cognitive-complexity) | **\(.cognitive)** | \($cog_t) | \($cog_status) |\n\($crap_row)| Lines | \(.line_count) | | |\n\n<details>\n<summary>What these metrics mean</summary>\n\n\($guidance)\(footer)"
    )
  }),
  (.runtime_coverage.findings[]? | {
    type: "runtime-coverage",
    path: (prefix + (.path | rel_path)),
    line: .line,
    body: (
      (if .verdict == "coverage_unavailable" then ":information_source:" else ":warning:" end) as $icon |
      (if .invocations == null then "\u2014" else (.invocations | tostring) end) as $invocations |
      (if .evidence.untracked_reason then (.evidence.v8_tracking + " (" + .evidence.untracked_reason + ")") else .evidence.v8_tracking end) as $tracking |
      "\($icon) **Runtime coverage: `\(.verdict)`**\n\n| Metric | Value |\n|:-------|:------|\n| Function | `\(.function)` |\n| Verdict | `\(.verdict)` |\n| Invocations | \($invocations) |\n| Confidence | \(.confidence) |\n| Static | \(.evidence.static_status) |\n| Tests | \(.evidence.test_coverage) |\n| V8 | \($tracking) |\n\n\(if .actions | length > 0 then "**Action:** \(.actions[0].description)\n" else "" end)\(prod_footer)"
    )
  }),
  ((.targets // .refactoring_targets // [])[:5][]? |
    (if .evidence.complex_functions then .evidence.complex_functions[0].line
     else 1 end) as $target_line |
    {
    type: "refactoring-target",
    path: (prefix + (.path | rel_path)),
    line: $target_line,
    body: ":bulb: **Refactoring target**\n\n`\(.recommendation)`\n\n| Effort | Confidence |\n|:-------|:-----------|\n| \(.effort) | \(.confidence) |\n\n\(if .factors then "**Why:**\n\(.factors | map("- \(.detail // "\(.metric): \(.value)")") | join("\n"))\n" else "" end)\(if .evidence.complex_functions then "\n<details>\n<summary>Complex functions</summary>\n\n\(.evidence.complex_functions | map("- `\(.name)` \u2014 cognitive: \(.cognitive), line \(.line)") | join("\n"))\n</details>\n" elif .evidence.unused_exports then "\n<details>\n<summary>Unused exports</summary>\n\n\(.evidence.unused_exports | map("- `\(.)`") | join("\n"))\n</details>\n" else "" end)\(footer)"
  })
] | .[:($ENV.MAX | tonumber)]
