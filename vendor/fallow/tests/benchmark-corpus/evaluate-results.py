#!/usr/bin/env python3
"""
Duplication Accuracy Baseline — Precision/Recall Evaluator

Compares fallow dupes JSON output against ground-truth.json to compute:
- Per-mode precision, recall, F1
- Per-clone-type breakdown
- False positive analysis
"""

import json
import os
import sys
from pathlib import Path
from dataclasses import dataclass, field
from typing import Optional

SCRIPT_DIR = Path(__file__).parent
RESULTS_DIR = SCRIPT_DIR / "results"
GROUND_TRUTH = SCRIPT_DIR / "ground-truth.json"


@dataclass
class DetectedClone:
    """A clone group from fallow output."""
    group_id: int
    instances: list  # list of {file, start_line, end_line, ...}
    token_count: int
    line_count: int


@dataclass
class MatchResult:
    """Whether a ground-truth pair was matched by a detection."""
    pair_id: str
    pair_type: str
    matched: bool
    matching_group_ids: list = field(default_factory=list)
    overlap_lines: int = 0
    notes: str = ""


@dataclass
class EvalMetrics:
    """Evaluation metrics for a single mode."""
    mode: str
    true_positives: int = 0
    false_positives: int = 0
    false_negatives: int = 0
    negative_violations: int = 0  # detections between negative pairs
    total_groups: int = 0
    matched_groups: set = field(default_factory=set)
    pair_results: list = field(default_factory=list)

    @property
    def precision(self) -> float:
        denom = self.true_positives + self.false_positives
        return self.true_positives / denom if denom > 0 else 0.0

    @property
    def recall(self) -> float:
        denom = self.true_positives + self.false_negatives
        return self.true_positives / denom if denom > 0 else 0.0

    @property
    def f1(self) -> float:
        p, r = self.precision, self.recall
        return 2 * p * r / (p + r) if (p + r) > 0 else 0.0


def normalize_path(path: str, root: str) -> str:
    """Normalize absolute paths to relative for comparison."""
    path = path.replace("\\", "/")
    if root and path.startswith(root):
        path = path[len(root):]
    path = path.lstrip("/")
    return path


def load_results(mode: str) -> Optional[dict]:
    """Load fallow dupes JSON output for a mode."""
    path = RESULTS_DIR / f"dupes-{mode}.json"
    if not path.exists():
        return None
    with open(path) as f:
        return json.load(f)


def load_ground_truth() -> dict:
    with open(GROUND_TRUTH) as f:
        return json.load(f)


def files_overlap(instances: list, file_a: str, file_b: str, root: str) -> tuple:
    """Check if a clone group has instances covering both file_a and file_b.
    Returns (matched, overlap_lines_a, overlap_lines_b)."""
    norm_a = file_a.replace("\\", "/")
    norm_b = file_b.replace("\\", "/")

    inst_a = None
    inst_b = None

    for inst in instances:
        norm_file = normalize_path(inst["file"], root)
        if norm_file == norm_a:
            inst_a = inst
        elif norm_file == norm_b:
            inst_b = inst

    if inst_a and inst_b:
        lines_a = inst_a["end_line"] - inst_a["start_line"] + 1
        lines_b = inst_b["end_line"] - inst_b["start_line"] + 1
        return True, lines_a, lines_b

    return False, 0, 0


def evaluate_mode(mode: str, ground_truth: dict, root_hint: str) -> Optional[EvalMetrics]:
    """Evaluate a single mode against ground truth."""
    data = load_results(mode)
    if data is None:
        return None

    metrics = EvalMetrics(mode=mode)
    clone_groups = data.get("clone_groups", [])
    metrics.total_groups = len(clone_groups)

    # Determine root from first file path
    root = root_hint

    # For each expected clone pair, check if any group covers it
    for pair in ground_truth["clone_pairs"]:
        expected_modes = pair.get("expected_in_modes", [])
        min_overlap = pair.get("min_overlap_lines", 0)

        matched = False
        matching_groups = []
        best_overlap = 0

        for gid, group in enumerate(clone_groups):
            found, lines_a, lines_b = files_overlap(
                group["instances"], pair["file_a"], pair["file_b"], root
            )
            if found:
                overlap = min(lines_a, lines_b)
                if overlap >= min_overlap:
                    matched = True
                    matching_groups.append(gid)
                    best_overlap = max(best_overlap, overlap)

        result = MatchResult(
            pair_id=pair["id"],
            pair_type=pair["type"],
            matched=matched,
            matching_group_ids=matching_groups,
            overlap_lines=best_overlap,
        )

        if mode in expected_modes:
            if matched:
                metrics.true_positives += 1
                result.notes = "TP"
            else:
                metrics.false_negatives += 1
                result.notes = "FN (expected but not found)"
        else:
            if matched:
                # Detected but not expected in this mode — might be acceptable
                # For Type-4 it's clearly a false positive
                # For Type-2/3 detected in strict: might be partial overlap (acceptable)
                if pair["type"] == "type-4":
                    result.notes = "FP (Type-4 should not be detected by token matching)"
                else:
                    result.notes = "Bonus (not expected but acceptable overlap)"
                    # Count as TP since it's a real clone, just not expected in this mode
                    metrics.true_positives += 1
            else:
                result.notes = "TN (correctly not detected in this mode)"

        metrics.pair_results.append(result)
        metrics.matched_groups.update(matching_groups)

    # Check negative pairs — any detection between these is a false positive
    for neg in ground_truth.get("negative_pairs", []):
        for gid, group in enumerate(clone_groups):
            found, lines_a, lines_b = files_overlap(
                group["instances"], neg["file_a"], neg["file_b"], root
            )
            if found and min(lines_a, lines_b) >= 5:
                metrics.negative_violations += 1
                metrics.false_positives += 1

    # Groups that didn't match any ground-truth pair: count as potential FPs
    # But only if they involve files from different categories
    unmatched_groups = set(range(len(clone_groups))) - metrics.matched_groups
    # We don't automatically count all unmatched as FP — some may be legitimate
    # sub-clones within expected pairs. Only count cross-category as FP.
    for gid in unmatched_groups:
        group = clone_groups[gid]
        files_in_group = set()
        for inst in group["instances"]:
            norm = normalize_path(inst["file"], root)
            files_in_group.add(norm)

        # Check if this is a cross-category detection (potential FP)
        categories = set()
        for f in files_in_group:
            parts = f.split("/")
            if len(parts) >= 2:
                categories.add(parts[1])  # src/type1-exact/... -> type1-exact

        if len(categories) > 1:
            # Cross-category: check if it matches any known pair
            is_known = False
            for pair in ground_truth["clone_pairs"]:
                for inst in group["instances"]:
                    norm = normalize_path(inst["file"], root)
                    if norm == pair["file_a"] or norm == pair["file_b"]:
                        is_known = True
                        break
                if is_known:
                    break

            if not is_known:
                metrics.false_positives += 1

    return metrics


def detect_root(mode: str = "strict") -> str:
    """Detect the absolute path root from the first result file."""
    data = load_results(mode)
    if not data or not data.get("clone_groups"):
        return ""
    first_file = data["clone_groups"][0]["instances"][0]["file"]
    # Find where "src/" starts
    idx = first_file.find("/src/")
    if idx >= 0:
        return first_file[:idx + 1]
    return ""


def main():
    ground_truth = load_ground_truth()
    root = detect_root()

    print("=" * 72)
    print("  FALLOW DUPLICATION ACCURACY BASELINE")
    print("=" * 72)
    print(f"\nCorpus: {SCRIPT_DIR}")
    print(f"Root detected: {root}")
    print(f"Clone pairs in ground truth: {len(ground_truth['clone_pairs'])}")
    print(f"Negative pairs: {len(ground_truth.get('negative_pairs', []))}")
    print()

    modes = ["strict", "mild", "weak", "semantic", "defaults"]
    all_metrics = []

    for mode in modes:
        metrics = evaluate_mode(mode, ground_truth, root)
        if metrics is None:
            continue
        all_metrics.append(metrics)

        print("-" * 72)
        print(f"  MODE: {mode.upper()}")
        print("-" * 72)
        print(f"  Total clone groups detected: {metrics.total_groups}")
        print(f"  True positives:  {metrics.true_positives}")
        print(f"  False positives: {metrics.false_positives}")
        print(f"  False negatives: {metrics.false_negatives}")
        print(f"  Negative-pair violations: {metrics.negative_violations}")
        print()
        print(f"  Precision: {metrics.precision:.1%}")
        print(f"  Recall:    {metrics.recall:.1%}")
        print(f"  F1 Score:  {metrics.f1:.1%}")
        print()

        # Per-pair breakdown
        print("  Per-pair results:")
        for r in metrics.pair_results:
            status = "MATCH" if r.matched else "MISS "
            print(f"    [{status}] {r.pair_id:16s} ({r.pair_type:8s}) "
                  f"overlap={r.overlap_lines:3d} lines  {r.notes}")
        print()

    # Summary table
    print("=" * 72)
    print("  SUMMARY")
    print("=" * 72)
    print(f"  {'Mode':<12s} {'Groups':>6s} {'TP':>4s} {'FP':>4s} {'FN':>4s} "
          f"{'Prec':>7s} {'Recall':>7s} {'F1':>7s}")
    print("  " + "-" * 58)
    for m in all_metrics:
        print(f"  {m.mode:<12s} {m.total_groups:>6d} {m.true_positives:>4d} "
              f"{m.false_positives:>4d} {m.false_negatives:>4d} "
              f"{m.precision:>6.1%} {m.recall:>6.1%} {m.f1:>6.1%}")
    print()

    # Per-type summary
    print("  Per clone type (best mode):")
    for clone_type in ["type-1", "type-2", "type-3", "type-4"]:
        pairs = [p for p in ground_truth["clone_pairs"] if p["type"] == clone_type]
        if not pairs:
            continue
        best_recall = 0
        best_mode = ""
        for m in all_metrics:
            matched = sum(1 for r in m.pair_results
                         if r.pair_type == clone_type and r.matched)
            recall = matched / len(pairs) if pairs else 0
            if recall > best_recall:
                best_recall = recall
                best_mode = m.mode
        print(f"    {clone_type}: {len(pairs)} pairs, best recall={best_recall:.0%} ({best_mode})")

    # Write machine-readable summary
    summary_path = RESULTS_DIR / "accuracy-baseline.json"
    summary = {
        "corpus": str(SCRIPT_DIR),
        "ground_truth_pairs": len(ground_truth["clone_pairs"]),
        "negative_pairs": len(ground_truth.get("negative_pairs", [])),
        "modes": {}
    }
    for m in all_metrics:
        summary["modes"][m.mode] = {
            "total_groups": m.total_groups,
            "true_positives": m.true_positives,
            "false_positives": m.false_positives,
            "false_negatives": m.false_negatives,
            "negative_violations": m.negative_violations,
            "precision": round(m.precision, 4),
            "recall": round(m.recall, 4),
            "f1": round(m.f1, 4),
            "pair_results": [
                {
                    "id": r.pair_id,
                    "type": r.pair_type,
                    "matched": r.matched,
                    "overlap_lines": r.overlap_lines,
                    "notes": r.notes,
                }
                for r in m.pair_results
            ],
        }
    with open(summary_path, "w") as f:
        json.dump(summary, f, indent=2)
    print(f"\n  Machine-readable summary: {summary_path}")


if __name__ == "__main__":
    main()
