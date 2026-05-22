#!/usr/bin/env python3
"""Parse cargo-modules DOT output and compute SIG module coupling metrics.

Reads DOT from stdin (one or more crates concatenated), extracts use-edges,
computes fan-in/fan-out per module, and outputs JSON benchmark data.

SIG 4-star thresholds:
  - Modules with >20 incoming deps: max 5.6%
  - Modules with >50 incoming deps: max 1.9%

Usage:
  cargo modules dependencies --lib -p <crate> --no-externs --no-fns \
    --no-sysroot --no-traits --no-types --no-owns \
    | python3 coupling-check.py [--max-fan-in N] [--max-fan-out N] [--warn-only]

Exit codes:
  0  All modules within thresholds (or --warn-only)
  1  Threshold exceeded
"""

import argparse
import json
import re
import sys
from collections import Counter

EDGE_RE = re.compile(r'"([^"]+)"\s*->\s*"([^"]+)"\s*\[label="uses"')

# Framework-inherent modules that naturally have high fan-in/fan-out.
# These are excluded from threshold violations but still reported.
FRAMEWORK_EXCEPTIONS = {
    # Plugin registry imports all plugins (high fan-out)
    "registry::builtin",
    # Plugin trait is imported by all plugins (high fan-in)
    "::plugins",
}


def is_framework_exception(module: str) -> bool:
    return any(exc in module for exc in FRAMEWORK_EXCEPTIONS)


def strip_crate_prefix(module: str) -> str:
    """Remove the crate name prefix for shorter display."""
    parts = module.split("::", 1)
    return parts[1] if len(parts) > 1 else module


def main() -> int:
    parser = argparse.ArgumentParser(description="SIG module coupling check")
    parser.add_argument("--max-fan-in", type=int, default=20,
                        help="Max incoming deps before flagging (SIG: 20)")
    parser.add_argument("--max-fan-out", type=int, default=20,
                        help="Max outgoing deps before flagging")
    parser.add_argument("--warn-only", action="store_true",
                        help="Report violations but exit 0")
    parser.add_argument("--json", type=str, default="",
                        help="Write benchmark JSON to this file")
    args = parser.parse_args()

    dot_input = sys.stdin.read()
    edges = EDGE_RE.findall(dot_input)

    if not edges:
        print("No use-edges found in input", file=sys.stderr)
        return 0

    fan_in: Counter[str] = Counter()
    fan_out: Counter[str] = Counter()
    all_modules: set[str] = set()

    for source, target in edges:
        fan_out[source] += 1
        fan_in[target] += 1
        all_modules.add(source)
        all_modules.add(target)

    total_modules = len(all_modules)

    # Compute SIG thresholds
    over_20_fan_in = [m for m, c in fan_in.items()
                      if c > 20 and not is_framework_exception(m)]
    over_50_fan_in = [m for m, c in fan_in.items()
                      if c > 50 and not is_framework_exception(m)]
    over_20_fan_out = [m for m, c in fan_out.items()
                       if c > args.max_fan_out and not is_framework_exception(m)]

    pct_over_20 = len(over_20_fan_in) / total_modules * 100 if total_modules else 0
    pct_over_50 = len(over_50_fan_in) / total_modules * 100 if total_modules else 0

    # Report
    print(f"\n{'=' * 60}")
    print(f"Module Coupling Report ({total_modules} modules, {len(edges)} edges)")
    print(f"{'=' * 60}")

    # Top fan-in (all, including exceptions)
    print(f"\nTop 10 modules by fan-in (incoming deps):")
    for module, count in fan_in.most_common(10):
        exc = " [framework]" if is_framework_exception(module) else ""
        flag = " **" if count > args.max_fan_in and not is_framework_exception(module) else ""
        print(f"  {count:>3}  {strip_crate_prefix(module)}{exc}{flag}")

    # Top fan-out
    print(f"\nTop 10 modules by fan-out (outgoing deps):")
    for module, count in fan_out.most_common(10):
        exc = " [framework]" if is_framework_exception(module) else ""
        flag = " **" if count > args.max_fan_out and not is_framework_exception(module) else ""
        print(f"  {count:>3}  {strip_crate_prefix(module)}{exc}{flag}")

    # SIG thresholds
    print(f"\nSIG 4-star thresholds (excluding framework exceptions):")
    sig_pass_20 = pct_over_20 <= 5.6
    sig_pass_50 = pct_over_50 <= 1.9
    print(f"  >20 fan-in: {len(over_20_fan_in)}/{total_modules} "
          f"({pct_over_20:.1f}%) {'PASS' if sig_pass_20 else 'FAIL'} (max 5.6%)")
    print(f"  >50 fan-in: {len(over_50_fan_in)}/{total_modules} "
          f"({pct_over_50:.1f}%) {'PASS' if sig_pass_50 else 'FAIL'} (max 1.9%)")

    if over_20_fan_in:
        print(f"\n  Violations (>20 fan-in, non-framework):")
        for m in over_20_fan_in:
            print(f"    {fan_in[m]:>3}  {strip_crate_prefix(m)}")

    if over_20_fan_out:
        print(f"\n  Violations (>{args.max_fan_out} fan-out, non-framework):")
        for m in over_20_fan_out:
            print(f"    {fan_out[m]:>3}  {strip_crate_prefix(m)}")

    # Write benchmark JSON for tracking
    if args.json:
        # Find max fan-in excluding framework exceptions
        non_fw_fan_in = {m: c for m, c in fan_in.items()
                         if not is_framework_exception(m)}
        max_fan_in = max(non_fw_fan_in.values()) if non_fw_fan_in else 0
        non_fw_fan_out = {m: c for m, c in fan_out.items()
                          if not is_framework_exception(m)}
        max_fan_out = max(non_fw_fan_out.values()) if non_fw_fan_out else 0

        bench_data = [
            {"name": "Max Fan-In (non-framework)", "unit": "deps",
             "value": max_fan_in},
            {"name": "Max Fan-Out (non-framework)", "unit": "deps",
             "value": max_fan_out},
            {"name": "Modules >20 Fan-In (%)", "unit": "%",
             "value": round(pct_over_20, 2)},
            {"name": "Total Modules", "unit": "count",
             "value": total_modules},
            {"name": "Total Edges", "unit": "count",
             "value": len(edges)},
        ]
        with open(args.json, "w") as f:
            json.dump(bench_data, f, indent=2)
        print(f"\nBenchmark data written to {args.json}")

    # Exit code
    violations = len(over_20_fan_in) + len(over_20_fan_out)
    if violations > 0 and not args.warn_only:
        print(f"\n{violations} coupling violation(s) found")
        return 1

    print(f"\nAll modules within thresholds")
    return 0


if __name__ == "__main__":
    sys.exit(main())
