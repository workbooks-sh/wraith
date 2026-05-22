//! Allocation tracking benchmark using dhat.
//!
//! This benchmark measures heap allocation statistics for the fallow analysis
//! pipeline. It cannot be a Criterion benchmark because dhat requires being
//! the global allocator.
//!
//! Run with: `cargo bench --bench allocations`
//!
//! Output is printed in a machine-parseable `key: value` format so that
//! `scripts/alloc-check.sh` can compare against a saved baseline.

#![expect(
    deprecated,
    reason = "ADR-008: benchmark exercises the workspace path-dep fallow_core::analyze surface"
)]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

mod helpers;

fn main() {
    // ── Set up a synthetic 100-file project ────────────────────────────
    let (temp_dir, config) = helpers::create_synthetic_project("alloc-bench", 100);

    // ── Profile the analysis pipeline ──────────────────────────────────
    let profiler = dhat::Profiler::builder().testing().build();

    let _ = fallow_core::analyze(&config);

    let stats = dhat::HeapStats::get();
    drop(profiler);

    // ── Clean up ───────────────────────────────────────────────────────
    let _ = std::fs::remove_dir_all(&temp_dir);

    // ── Print parseable stats ──────────────────────────────────────────
    // These keys are read by scripts/alloc-check.sh and stored in
    // alloc-baseline.json.
    #[expect(
        clippy::print_stdout,
        reason = "intentional bench output for alloc-check.sh"
    )]
    {
        println!("alloc_total_bytes: {}", stats.total_bytes);
        println!("alloc_total_blocks: {}", stats.total_blocks);
        println!("alloc_max_bytes: {}", stats.max_bytes);
        println!("alloc_max_blocks: {}", stats.max_blocks);
    }
}
