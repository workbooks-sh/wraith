#!/usr/bin/env python3
"""Aggregate per-project conformance reports into a combined report.

Usage:
    python3 aggregate.py <reports_dir>

Reads all *-report.json files from the directory, combines them into a single
report with per-project breakdowns and overall summary.

Outputs aggregated JSON to stdout.
"""

import json
import sys
from pathlib import Path


def main():
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <reports_dir>", file=sys.stderr)
        sys.exit(1)

    reports_dir = Path(sys.argv[1])
    report_files = sorted(reports_dir.glob("*-report.json"))

    if not report_files:
        print("Error: no report files found", file=sys.stderr)
        sys.exit(1)

    projects = {}
    totals = {
        "fallow_total": 0,
        "knip_total": 0,
        "agreed": 0,
        "fallow_only": 0,
        "knip_only": 0,
    }

    # Aggregate by_type across all projects
    agg_by_type = {}

    for report_file in report_files:
        # Extract project name from filename: "name-report.json" → "name"
        name = report_file.stem.removesuffix("-report")

        with open(report_file) as f:
            report = json.load(f)

        projects[name] = report["summary"]

        for key in totals:
            totals[key] += report["summary"][key]

        for issue_type, data in report.get("by_type", {}).items():
            if issue_type not in agg_by_type:
                agg_by_type[issue_type] = {
                    "fallow_count": 0,
                    "knip_count": 0,
                    "agreed": 0,
                    "fallow_only": 0,
                    "knip_only": 0,
                }
            for field in ("fallow_count", "knip_count", "agreed", "fallow_only", "knip_only"):
                agg_by_type[issue_type][field] += data[field]

    # Calculate agreement percentages
    total_unique = totals["agreed"] + totals["fallow_only"] + totals["knip_only"]
    totals["agreement_pct"] = (
        round(totals["agreed"] / total_unique * 100, 1) if total_unique > 0 else 100.0
    )

    for data in agg_by_type.values():
        type_total = data["agreed"] + data["fallow_only"] + data["knip_only"]
        data["agreement_pct"] = (
            round(data["agreed"] / type_total * 100, 1) if type_total > 0 else 100.0
        )

    result = {
        "summary": totals,
        "projects": projects,
        "by_type": dict(sorted(agg_by_type.items())),
    }

    # Print human summary to stderr
    print(f"Overall agreement: {totals['agreement_pct']}%", file=sys.stderr)
    print(f"  Agreed: {totals['agreed']}", file=sys.stderr)
    print(f"  Fallow-only: {totals['fallow_only']}", file=sys.stderr)
    print(f"  Knip-only: {totals['knip_only']}", file=sys.stderr)
    print(file=sys.stderr)
    print("Per project:", file=sys.stderr)
    for name, summary in sorted(projects.items()):
        print(f"  {name}: {summary['agreement_pct']}%", file=sys.stderr)

    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
