//! Ownership risk analysis for hotspot files.
//!
//! Computes per-file ownership signals from git author history:
//!
//! - **Bus factor** (Avelino truck factor): minimum number of contributors who
//!   together account for at least 50% of recency-weighted commits in the
//!   analysis window.
//! - **Contributor count**: distinct authors after bot filtering.
//! - **Top contributor**: the single highest-share author with their share,
//!   commit count, and "stale days" since their last touch.
//! - **Recent contributors**: up to three additional authors by share, useful
//!   for review-routing in agent workflows.
//! - **Declared owner**: the CODEOWNERS-resolved owner for the file path,
//!   when a CODEOWNERS file exists and a rule matches.
//! - **Unowned (tristate)**: `Some(true)` if a CODEOWNERS file exists but no
//!   rule matches; `Some(false)` if a rule matches; `None` if no CODEOWNERS
//!   file was discovered for the repository.
//! - **Drift**: true when the file's original author (earliest first commit
//!   in the window) differs substantially from the current top contributor,
//!   the file is at least 30 days old, and the original author's share is
//!   below 10%. Pairs with a human-readable `drift_reason` string.
//!
//! # Privacy
//!
//! Author emails are emitted in one of three modes per [`EmailMode`]:
//!
//! - `Raw` — full email as it appears in git history.
//! - `Handle` (default) — local-part only, with GitHub-style `12345+name`
//!   noreply addresses unwrapped to `name`.
//! - `Hash` — stable `xxh3:<16hex>` pseudonym derived from the raw email.
//!
//! Hashed mode is suitable for CI artifacts in regulated environments where
//! even local-parts are sensitive. The hash is non-cryptographic but stable
//! across runs.

use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use xxhash_rust::xxh3::xxh3_64;

use fallow_config::EmailMode;
use fallow_core::churn::{AuthorContribution, FileChurn};

use crate::codeowners::CodeOwners;
use crate::health_types::{ContributorEntry, ContributorIdentifierFormat, OwnershipMetrics};

/// Seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// Drift detection: the file must be at least this old (days) for drift to
/// be considered. Avoids flagging recently scaffolded files.
const DRIFT_MIN_FILE_AGE_DAYS: u64 = 30;

/// Drift detection: the original author's recency-weighted share must be
/// below this fraction for drift to fire. Tightens the boolean against
/// "scaffolded by one person, properly built by team" false positives.
const DRIFT_MAX_ORIGINAL_SHARE: f64 = 0.10;

/// Inputs needed to compute ownership for one file. Built once per analysis
/// run and reused for every hotspot.
pub struct OwnershipContext<'a> {
    /// Author email pool from [`fallow_core::churn::ChurnResult::author_pool`].
    pub author_pool: &'a [String],
    /// Compiled bot-author globs from the ownership config's `bot_patterns`.
    pub bot_globs: &'a GlobSet,
    /// CODEOWNERS lookup, when one was discovered.
    pub codeowners: Option<&'a CodeOwners>,
    /// Privacy mode for emitted author emails.
    pub email_mode: EmailMode,
    /// Current Unix epoch seconds; injectable so tests are deterministic.
    pub now_secs: u64,
}

/// Compile bot-author glob patterns from configuration.
///
/// Each pattern is matched (via `globset`) against the raw author email.
///
/// # Errors
/// Returns the first invalid glob pattern encountered.
pub fn compile_bot_globs(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let mut glob = GlobBuilder::new(p);
        glob.backslash_escape(true);
        builder.add(glob.build()?);
    }
    builder.build()
}

/// Compute ownership signals for one file. Returns `None` when the file has
/// no recorded authors or all authors are filtered out as bots.
#[expect(
    clippy::cast_possible_truncation,
    reason = "contributor counts and bus factor are bounded by author pool size"
)]
pub fn compute_ownership(
    churn: &FileChurn,
    relative_path: &Path,
    ctx: &OwnershipContext<'_>,
) -> Option<OwnershipMetrics> {
    if churn.authors.is_empty() {
        return None;
    }

    // Resolve, filter, and rank contributions.
    let mut filtered: Vec<RankedAuthor<'_>> = churn
        .authors
        .iter()
        .filter_map(|(idx, contribution)| {
            let raw_email = ctx.author_pool.get(*idx as usize)?;
            if is_bot(raw_email, ctx.bot_globs) {
                return None;
            }
            Some(RankedAuthor {
                idx: *idx,
                contribution,
                rendered: render_email(raw_email, ctx.email_mode),
            })
        })
        .collect();

    if filtered.is_empty() {
        return None;
    }

    filtered.sort_by(|a, b| {
        b.contribution
            .weighted_commits
            .partial_cmp(&a.contribution.weighted_commits)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_weighted: f64 = filtered
        .iter()
        .map(|a| a.contribution.weighted_commits)
        .sum();
    if total_weighted <= 0.0 {
        return None;
    }

    let bus_factor = compute_bus_factor(&filtered, total_weighted);
    let format = identifier_format(ctx.email_mode);

    let top = &filtered[0];
    let top_contributor = ContributorEntry {
        identifier: top.rendered.clone(),
        format,
        share: round3(top.contribution.weighted_commits / total_weighted),
        stale_days: stale_days(top.contribution.last_commit_ts, ctx.now_secs),
        commits: top.contribution.commits,
    };

    let recent_contributors: Vec<ContributorEntry> = filtered
        .iter()
        .skip(1)
        .take(3)
        .map(|a| ContributorEntry {
            identifier: a.rendered.clone(),
            format,
            share: round3(a.contribution.weighted_commits / total_weighted),
            stale_days: stale_days(a.contribution.last_commit_ts, ctx.now_secs),
            commits: a.contribution.commits,
        })
        .collect();

    // suggested_reviewers = recent_contributors filtered by stale_days < 90.
    // AI agents can paste this directly into "Request review from @X, @Y"
    // PR comments without re-filtering.
    let suggested_reviewers: Vec<ContributorEntry> = recent_contributors
        .iter()
        .filter(|c| c.stale_days < 90)
        .cloned()
        .collect();

    let (drift, drift_reason) = compute_drift(&filtered, total_weighted, ctx.now_secs);

    let (declared_owner, unowned) = ctx.codeowners.map_or((None, None), |co| {
        co.owner_of(relative_path)
            .map_or((None, Some(true)), |owner| {
                (Some(owner.to_string()), Some(false))
            })
    });

    Some(OwnershipMetrics {
        bus_factor,
        contributor_count: filtered.len() as u32,
        top_contributor,
        recent_contributors,
        suggested_reviewers,
        declared_owner,
        unowned,
        drift,
        drift_reason,
    })
}

/// Per-author working entry used during ranking.
struct RankedAuthor<'a> {
    /// Interned author pool index. Stable identifier for equality checks
    /// across `min_by_key` / sorted-position comparisons in drift detection.
    idx: u32,
    contribution: &'a AuthorContribution,
    rendered: String,
}

/// Avelino truck factor. Sort by weighted commits descending (already done
/// by the caller), accumulate until we cross 50% of total weighted commits.
fn compute_bus_factor(ranked: &[RankedAuthor<'_>], total_weighted: f64) -> u32 {
    let mut acc = 0.0;
    let mut count: u32 = 0;
    for entry in ranked {
        acc += entry.contribution.weighted_commits;
        count += 1;
        if acc / total_weighted >= 0.5 {
            break;
        }
    }
    count
}

/// Drift detection. Original author = earliest first-commit-ts among
/// non-bot contributors. Drift fires only when:
///
/// 1. Original author differs from current top contributor.
/// 2. File age >= [`DRIFT_MIN_FILE_AGE_DAYS`] (avoids flagging fresh files).
/// 3. Original author's recent share < [`DRIFT_MAX_ORIGINAL_SHARE`] (avoids
///    flagging files where the author still actively maintains).
fn compute_drift(
    ranked: &[RankedAuthor<'_>],
    total_weighted: f64,
    now_secs: u64,
) -> (bool, Option<String>) {
    let Some(original) = ranked.iter().min_by_key(|a| a.contribution.first_commit_ts) else {
        return (false, None);
    };
    let top = &ranked[0];

    // Compare by interned author idx, not pointer identity. Pointer equality
    // works today because both `RankedAuthor` entries borrow from the same
    // `FxHashMap`, but it would silently break if `filtered` were ever
    // rebuilt from copies. The idx field is a stable identifier.
    if original.idx == top.idx {
        return (false, None);
    }

    let file_age_days = stale_days(original.contribution.first_commit_ts, now_secs);
    let original_share = original.contribution.weighted_commits / total_weighted;
    let top_share = top.contribution.weighted_commits / total_weighted;

    if file_age_days < DRIFT_MIN_FILE_AGE_DAYS || original_share >= DRIFT_MAX_ORIGINAL_SHARE {
        return (false, None);
    }

    let reason = format!(
        "original author {} now has {:.0}% share; current top is {} ({:.0}%)",
        original.rendered,
        original_share * 100.0,
        top.rendered,
        top_share * 100.0,
    );
    (true, Some(reason))
}

fn stale_days(commit_ts: u64, now_secs: u64) -> u64 {
    now_secs.saturating_sub(commit_ts) / SECS_PER_DAY
}

/// Map the configured [`EmailMode`] to the public format discriminator
/// emitted in [`ContributorEntry::format`].
const fn identifier_format(mode: EmailMode) -> ContributorIdentifierFormat {
    match mode {
        EmailMode::Raw => ContributorIdentifierFormat::Raw,
        EmailMode::Handle => ContributorIdentifierFormat::Handle,
        EmailMode::Hash => ContributorIdentifierFormat::Hash,
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn is_bot(email: &str, bot_globs: &GlobSet) -> bool {
    bot_globs.is_match(email)
}

/// Render an author email per the configured privacy mode.
fn render_email(email: &str, mode: EmailMode) -> String {
    match mode {
        EmailMode::Raw => email.to_string(),
        EmailMode::Handle => extract_handle(email),
        EmailMode::Hash => hash_email(email),
    }
}

/// Extract a display handle from an email address.
///
/// Strips the domain and unwraps GitHub-style numeric noreply prefixes
/// (`12345+alice@users.noreply.github.com` → `alice`).
fn extract_handle(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    if let Some(plus_idx) = local.find('+') {
        let after_plus = &local[plus_idx + 1..];
        if !after_plus.is_empty() {
            return after_plus.to_string();
        }
    }
    if local.is_empty() {
        return email.to_string();
    }
    local.to_string()
}

/// Stable non-cryptographic pseudonym for an email address.
///
/// Uses xxh3 (already a workspace dep) prefixed with `xxh3:` so consumers
/// can recognize the pseudonym format. Not suitable as a security primitive
/// — given a known list of org emails, rebuilding the rainbow table is
/// trivial. The intent is to avoid emitting raw PII into CI artifacts.
fn hash_email(email: &str) -> String {
    let h = xxh3_64(email.as_bytes());
    format!("xxh3:{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashMap;

    const NOW: u64 = 1_750_000_000;

    fn ts_days_ago(days: u64) -> u64 {
        NOW.saturating_sub(days * SECS_PER_DAY)
    }

    fn churn_with_authors(path: &str, authors: &[(u32, u32, f64, u64, u64)]) -> FileChurn {
        let mut map: FxHashMap<u32, AuthorContribution> = FxHashMap::default();
        for &(idx, commits, weighted, first_ts, last_ts) in authors {
            map.insert(
                idx,
                AuthorContribution {
                    commits,
                    weighted_commits: weighted,
                    first_commit_ts: first_ts,
                    last_commit_ts: last_ts,
                },
            );
        }
        FileChurn {
            path: std::path::PathBuf::from(path),
            commits: authors.iter().map(|a| a.1).sum(),
            weighted_commits: authors.iter().map(|a| a.2).sum(),
            lines_added: 0,
            lines_deleted: 0,
            trend: fallow_core::churn::ChurnTrend::Stable,
            authors: map,
        }
    }

    fn empty_globs() -> GlobSet {
        GlobSet::empty()
    }

    fn ctx_with<'a>(
        pool: &'a [String],
        globs: &'a GlobSet,
        codeowners: Option<&'a CodeOwners>,
    ) -> OwnershipContext<'a> {
        OwnershipContext {
            author_pool: pool,
            bot_globs: globs,
            codeowners,
            email_mode: EmailMode::Raw,
            now_secs: NOW,
        }
    }

    // ── extract_handle ────────────────────────────────────────────

    #[test]
    fn extract_handle_strips_domain() {
        assert_eq!(extract_handle("alice@example.com"), "alice");
    }

    #[test]
    fn extract_handle_unwraps_github_noreply() {
        assert_eq!(
            extract_handle("12345+alice@users.noreply.github.com"),
            "alice"
        );
    }

    #[test]
    fn extract_handle_keeps_plus_suffix_when_present() {
        // user+tag@example.com -> "tag" (matches GH noreply pattern)
        assert_eq!(extract_handle("user+tag@example.com"), "tag");
    }

    #[test]
    fn extract_handle_falls_back_for_no_at() {
        assert_eq!(extract_handle("alice"), "alice");
    }

    #[test]
    fn extract_handle_empty_local_falls_back() {
        // "@example.com" -> empty local, return original
        assert_eq!(extract_handle("@example.com"), "@example.com");
    }

    // ── hash_email ────────────────────────────────────────────────

    #[test]
    fn hash_email_is_stable() {
        let a = hash_email("alice@example.com");
        let b = hash_email("alice@example.com");
        assert_eq!(a, b);
        assert!(a.starts_with("xxh3:"));
        assert_eq!(a.len(), "xxh3:".len() + 16);
    }

    #[test]
    fn hash_email_differs_per_input() {
        assert_ne!(hash_email("alice@x"), hash_email("bob@x"));
    }

    // ── render_email ──────────────────────────────────────────────

    #[test]
    fn render_email_raw_passes_through() {
        assert_eq!(
            render_email("alice@example.com", EmailMode::Raw),
            "alice@example.com"
        );
    }

    #[test]
    fn render_email_handle_strips_domain() {
        assert_eq!(
            render_email("alice@example.com", EmailMode::Handle),
            "alice"
        );
    }

    #[test]
    fn render_email_hash_obfuscates() {
        let r = render_email("alice@example.com", EmailMode::Hash);
        assert!(r.starts_with("xxh3:"));
        assert!(!r.contains("alice"));
    }

    // ── compile_bot_globs / is_bot ────────────────────────────────

    #[test]
    fn bot_globs_match_default_patterns() {
        let globs = compile_bot_globs(&[
            r"*\[bot\]*".to_string(),
            "dependabot*".to_string(),
            "github-actions*".to_string(),
        ])
        .unwrap();
        assert!(is_bot("dependabot[bot]@users.noreply.github.com", &globs));
        assert!(is_bot("dependabot@github.com", &globs));
        assert!(is_bot("github-actions@users.noreply.github.com", &globs));
        assert!(!is_bot("alice@example.com", &globs));
    }

    #[test]
    fn human_github_noreply_is_not_a_bot() {
        // Regression: `*noreply*` was once a default bot pattern but it
        // matched the GitHub privacy-default address format used by the
        // majority of real human contributors. Ensure default patterns
        // don't fire on a typical human noreply address.
        let globs =
            compile_bot_globs(&fallow_config::OwnershipConfig::default().bot_patterns).unwrap();
        // Real example from a vite contributor.
        assert!(!is_bot(
            "49056869+sapphi-red@users.noreply.github.com",
            &globs
        ));
        assert!(!is_bot("12345+alice@users.noreply.github.com", &globs));
        // The actual github-actions[bot] still gets caught via `\[bot\]`.
        assert!(is_bot(
            "41898282+github-actions[bot]@users.noreply.github.com",
            &globs
        ));
    }

    // ── compute_bus_factor ────────────────────────────────────────

    #[test]
    fn bus_factor_single_dominant_author_is_one() {
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 9, 9.0, ts_days_ago(60), ts_days_ago(1)),
                (1, 1, 1.0, ts_days_ago(30), ts_days_ago(20)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.bus_factor, 1);
        assert_eq!(m.contributor_count, 2);
        assert_eq!(m.top_contributor.identifier, "alice@x");
    }

    #[test]
    fn bus_factor_even_split_three_authors_is_two() {
        // Three authors at 4/3/3 weighted. Top one is 40%, top two = 70% (>= 50%).
        let pool = vec!["a@x".to_string(), "b@x".to_string(), "c@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 4, 4.0, ts_days_ago(50), ts_days_ago(1)),
                (1, 3, 3.0, ts_days_ago(40), ts_days_ago(2)),
                (2, 3, 3.0, ts_days_ago(30), ts_days_ago(3)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.bus_factor, 2);
        assert_eq!(m.contributor_count, 3);
    }

    #[test]
    fn bus_factor_excludes_bots() {
        let pool = vec![
            "alice@x".to_string(),
            "dependabot[bot]@users.noreply.github.com".to_string(),
        ];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 1.0, ts_days_ago(60), ts_days_ago(10)),
                (1, 100, 100.0, ts_days_ago(30), ts_days_ago(1)),
            ],
        );
        let globs = compile_bot_globs(&[r"*\[bot\]*".to_string()]).unwrap();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.bus_factor, 1);
        assert_eq!(m.contributor_count, 1);
        assert_eq!(m.top_contributor.identifier, "alice@x");
    }

    // ── recent_contributors ───────────────────────────────────────

    #[test]
    fn recent_contributors_takes_top_three_excluding_top() {
        let pool = (0..6).map(|i| format!("u{i}@x")).collect::<Vec<_>>();
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 10, 10.0, ts_days_ago(60), ts_days_ago(1)),
                (1, 5, 5.0, ts_days_ago(50), ts_days_ago(2)),
                (2, 4, 4.0, ts_days_ago(40), ts_days_ago(3)),
                (3, 3, 3.0, ts_days_ago(30), ts_days_ago(4)),
                (4, 2, 2.0, ts_days_ago(20), ts_days_ago(5)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert_eq!(m.recent_contributors.len(), 3);
        assert_eq!(m.recent_contributors[0].identifier, "u1@x");
        assert_eq!(m.recent_contributors[1].identifier, "u2@x");
        assert_eq!(m.recent_contributors[2].identifier, "u3@x");
    }

    // ── drift detection ───────────────────────────────────────────

    #[test]
    fn drift_fires_when_original_author_inactive_old_file() {
        // alice scaffolded 200 days ago with 1 commit, bob has built it out
        // with 20 weighted commits. alice's share is well below 10%.
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 0.5, ts_days_ago(200), ts_days_ago(200)),
                (1, 20, 20.0, ts_days_ago(60), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(m.drift);
        let reason = m.drift_reason.expect("drift_reason should be set");
        assert!(reason.contains("alice@x"));
        assert!(reason.contains("bob@x"));
    }

    #[test]
    fn drift_does_not_fire_for_recently_scaffolded_file() {
        // File is only 10 days old. Drift heuristic requires 30+ day age.
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 0.5, ts_days_ago(10), ts_days_ago(10)),
                (1, 20, 20.0, ts_days_ago(8), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(!m.drift);
        assert!(m.drift_reason.is_none());
    }

    #[test]
    fn drift_does_not_fire_when_original_still_active() {
        // alice still has 30% share — no drift even though bob is on top.
        let pool = vec!["alice@x".to_string(), "bob@x".to_string()];
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 6, 6.0, ts_days_ago(200), ts_days_ago(2)),
                (1, 14, 14.0, ts_days_ago(60), ts_days_ago(1)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(!m.drift);
    }

    #[test]
    fn drift_does_not_fire_when_original_is_top_contributor() {
        let pool = vec!["alice@x".to_string()];
        let churn = churn_with_authors("f.ts", &[(0, 10, 10.0, ts_days_ago(200), ts_days_ago(1))]);
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!(!m.drift);
    }

    // ── CODEOWNERS cross-reference ───────────────────────────────

    #[test]
    fn unowned_tristate_some_true_when_no_rule_matches() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let pool = vec!["alice@x".to_string()];
        let churn =
            churn_with_authors("README.md", &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))]);
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, Some(&co));
        let m = compute_ownership(&churn, Path::new("README.md"), &ctx).unwrap();
        assert_eq!(m.unowned, Some(true));
        assert!(m.declared_owner.is_none());
    }

    #[test]
    fn unowned_tristate_some_false_when_rule_matches() {
        let co = CodeOwners::parse("/src/ @frontend\n").unwrap();
        let pool = vec!["alice@x".to_string()];
        let churn = churn_with_authors(
            "src/app.ts",
            &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, Some(&co));
        let m = compute_ownership(&churn, Path::new("src/app.ts"), &ctx).unwrap();
        assert_eq!(m.unowned, Some(false));
        assert_eq!(m.declared_owner.as_deref(), Some("@frontend"));
    }

    #[test]
    fn unowned_tristate_none_when_no_codeowners_file() {
        let pool = vec!["alice@x".to_string()];
        let churn = churn_with_authors(
            "src/app.ts",
            &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("src/app.ts"), &ctx).unwrap();
        assert_eq!(m.unowned, None);
        assert!(m.declared_owner.is_none());
    }

    // ── empty / bot-only inputs ───────────────────────────────────

    #[test]
    fn returns_none_when_no_authors() {
        let pool: Vec<String> = vec![];
        let churn = churn_with_authors("f.ts", &[]);
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        assert!(compute_ownership(&churn, Path::new("f.ts"), &ctx).is_none());
    }

    #[test]
    fn returns_none_when_only_bot_authors() {
        let pool = vec!["dependabot[bot]@users.noreply.github.com".to_string()];
        let churn = churn_with_authors("f.ts", &[(0, 5, 5.0, ts_days_ago(60), ts_days_ago(1))]);
        let globs = compile_bot_globs(&[r"*\[bot\]*".to_string()]).unwrap();
        let ctx = ctx_with(&pool, &globs, None);
        assert!(compute_ownership(&churn, Path::new("f.ts"), &ctx).is_none());
    }

    // ── stale_days ────────────────────────────────────────────────

    #[test]
    fn stale_days_clamps_at_zero_for_future_timestamps() {
        // Future timestamp shouldn't underflow.
        assert_eq!(stale_days(NOW + 1000, NOW), 0);
    }

    #[test]
    fn stale_days_basic() {
        assert_eq!(stale_days(ts_days_ago(7), NOW), 7);
        assert_eq!(stale_days(ts_days_ago(0), NOW), 0);
    }

    // ── share rounding ────────────────────────────────────────────

    #[test]
    fn shares_are_rounded_to_three_decimals() {
        let pool = vec!["a@x".to_string(), "b@x".to_string(), "c@x".to_string()];
        // Ratios: 1/3, 1/3, 1/3 = 0.333... → 0.333 after round3.
        let churn = churn_with_authors(
            "f.ts",
            &[
                (0, 1, 1.0, ts_days_ago(50), ts_days_ago(1)),
                (1, 1, 1.0, ts_days_ago(40), ts_days_ago(2)),
                (2, 1, 1.0, ts_days_ago(30), ts_days_ago(3)),
            ],
        );
        let globs = empty_globs();
        let ctx = ctx_with(&pool, &globs, None);
        let m = compute_ownership(&churn, Path::new("f.ts"), &ctx).unwrap();
        assert!((m.top_contributor.share - 0.333).abs() < f64::EPSILON);
    }
}
