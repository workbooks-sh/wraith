def count(obj; key): obj | if . then .[key] // 0 else 0 end;
def prod_failing_findings:
  (.health.runtime_coverage.findings // [])
  | map(select(.verdict == "safe_to_delete" or .verdict == "review_required" or .verdict == "low_traffic"));
def prod_advisory_findings:
  (.health.runtime_coverage.findings // [])
  | map(select(.verdict != "safe_to_delete" and .verdict != "review_required" and .verdict != "low_traffic"));
def prod_hot_paths: (.health.runtime_coverage.hot_paths // []);

(count(.check; "total_issues") // 0) as $check |
(count(.dupes.stats; "clone_groups") // 0) as $dupes |
(count(.health.summary; "functions_above_threshold") // 0) as $complex |
(($complex) + (prod_failing_findings | length)) as $health |
(prod_advisory_findings | length) as $prod_advisory |
(prod_hot_paths | length) as $hot_paths |
($check + $dupes + $health) as $total |
(.health.vital_signs // {}) as $vitals |
(($ENV.FILTERED_COUNT // "0") | tonumber) as $filtered |
(($ENV.INLINE_COUNT // "0") | tonumber) as $inline |

"## \ud83c\udf3f Fallow Review\n\n" +

(if $check > 0 then ":warning: **\($check)** code issues" else ":white_check_mark: No code issues" end) +
" \u00b7 " +
(if $dupes > 0 then ":warning: **\($dupes)** clone groups" else ":white_check_mark: No duplication" end) +
" \u00b7 " +
(if $health > 0 then ":warning: **\($health)** health findings" else ":white_check_mark: No blocking health findings" end) +
(if $prod_advisory > 0 then " \u00b7 :information_source: **\($prod_advisory)** coverage advisory finding\(if $prod_advisory == 1 then "" else "s" end)" else "" end) +
(if $hot_paths > 0 then " \u00b7 :eyes: **\($hot_paths)** hot path\(if $hot_paths == 1 then "" else "s" end)" else "" end) +

"\n\n" +

(if $vitals.maintainability_avg then
  "[Maintainability](https://docs.fallow.tools/explanations/health): **\($vitals.maintainability_avg | . * 10 | round / 10)** / 100" +
  (if $vitals.avg_cyclomatic then " \u00b7 Avg complexity: \($vitals.avg_cyclomatic | . * 10 | round / 10)" else "" end) +
  "\n\n"
else "" end) +

(if $prod_advisory > 0 or $hot_paths > 0 then
  "Runtime coverage: " +
  (if $prod_advisory > 0 then "**\($prod_advisory)** advisory finding\(if $prod_advisory == 1 then "" else "s" end)" else "" end) +
  (if $prod_advisory > 0 and $hot_paths > 0 then " \u00b7 " else "" end) +
  (if $hot_paths > 0 then "**\($hot_paths)** hot path\(if $hot_paths == 1 then "" else "s" end)" else "" end) +
  "\n\n"
else "" end) +

(if $filtered > 0 and $inline > 0 then
  "**\($inline)** inline comments on your changes \u00b7 \($filtered) findings in files not changed in this PR \u2014 run `fallow dead-code` locally to see them\n\n"
elif $filtered > 0 then
  "\($filtered) findings in changed files \u00b7 none are on lines changed in this PR\n\n"
elif $inline > 0 then
  "**\($inline)** inline comments on your changes.\n\n"
else
  ""
end) +
"<!-- fallow-review -->"
