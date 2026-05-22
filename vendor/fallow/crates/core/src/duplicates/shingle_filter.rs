//! Focused duplicate-analysis prefilter based on k-token shingles.

use rustc_hash::{FxHashSet, FxHasher};
use std::hash::Hasher;
use std::path::PathBuf;

use super::{TokenizedFile, normalize::HashedToken};

const DEFAULT_SHINGLE_TOKENS: usize = 7;

pub(super) fn filter_to_focus_candidates(
    files: &mut Vec<TokenizedFile>,
    focus_files: &FxHashSet<PathBuf>,
    min_tokens: usize,
) {
    let window = min_tokens.clamp(1, DEFAULT_SHINGLE_TOKENS);
    let mut focus_shingles = FxHashSet::default();
    for file in files.iter().filter(|file| focus_files.contains(&file.path)) {
        insert_shingles(&file.hashed_tokens, window, &mut focus_shingles);
    }
    if focus_shingles.is_empty() {
        return;
    }

    let mut candidates_kept = 0usize;
    let mut candidates_skipped = 0usize;
    files.retain(|file| {
        if focus_files.contains(&file.path) {
            return true;
        }
        let keep = has_matching_shingle(&file.hashed_tokens, window, &focus_shingles);
        if keep {
            candidates_kept += 1;
        } else {
            candidates_skipped += 1;
        }
        keep
    });
    tracing::debug!(
        candidates_kept,
        candidates_skipped,
        shingle_window = window,
        "focused duplication shingle prefilter"
    );
}

fn insert_shingles(tokens: &[HashedToken], window: usize, out: &mut FxHashSet<u64>) {
    if tokens.len() < window {
        return;
    }
    for shingle in tokens.windows(window) {
        out.insert(hash_shingle(shingle));
    }
}

fn has_matching_shingle(
    tokens: &[HashedToken],
    window: usize,
    focus_shingles: &FxHashSet<u64>,
) -> bool {
    tokens.len() >= window
        && tokens
            .windows(window)
            .any(|shingle| focus_shingles.contains(&hash_shingle(shingle)))
}

fn hash_shingle(tokens: &[HashedToken]) -> u64 {
    let mut hasher = FxHasher::default();
    for token in tokens {
        hasher.write_u64(token.hash);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::tokenize::FileTokens;
    use proptest::prelude::*;

    fn file(path: &str, hashes: &[u64]) -> TokenizedFile {
        TokenizedFile {
            path: PathBuf::from(path),
            hashed_tokens: hashes
                .iter()
                .enumerate()
                .map(|(original_index, &hash)| HashedToken {
                    hash,
                    original_index,
                })
                .collect(),
            file_tokens: FileTokens {
                tokens: Vec::new(),
                atomic_invocation_spans: Vec::new(),
                source: String::new(),
                line_count: 1,
            },
            metadata: None,
            cache_hit: false,
            suppressions: Vec::new(),
        }
    }

    #[test]
    fn keeps_focus_and_matching_candidates_only() {
        let mut files = vec![
            file("focus.ts", &[1, 2, 3, 4, 5]),
            file("candidate.ts", &[9, 1, 2, 3, 8]),
            file("unrelated.ts", &[10, 11, 12, 13, 14]),
        ];
        let focus = FxHashSet::from_iter([PathBuf::from("focus.ts")]);

        filter_to_focus_candidates(&mut files, &focus, 3);

        let paths = files
            .into_iter()
            .map(|file| file.path)
            .collect::<FxHashSet<_>>();
        assert!(paths.contains(&PathBuf::from("focus.ts")));
        assert!(paths.contains(&PathBuf::from("candidate.ts")));
        assert!(!paths.contains(&PathBuf::from("unrelated.ts")));
    }

    proptest! {
        #[test]
        fn keeps_files_that_share_a_focus_shingle(
            shared in prop::collection::vec(1_u64..1_000, 5..20),
            focus_prefix in prop::collection::vec(10_000_u64..20_000, 0..8),
            focus_suffix in prop::collection::vec(20_000_u64..30_000, 0..8),
            match_prefix in prop::collection::vec(30_000_u64..40_000, 0..8),
            match_suffix in prop::collection::vec(40_000_u64..50_000, 0..8),
            noise in prop::collection::vec(50_000_u64..60_000, 5..20),
        ) {
            let mut focus_hashes = focus_prefix;
            focus_hashes.extend(shared.iter().copied());
            focus_hashes.extend(focus_suffix);

            let mut match_hashes = match_prefix;
            match_hashes.extend(shared);
            match_hashes.extend(match_suffix);

            let mut files = vec![
                file("focus.ts", &focus_hashes),
                file("matching.ts", &match_hashes),
                file("noise.ts", &noise),
            ];
            let focus = FxHashSet::from_iter([PathBuf::from("focus.ts")]);

            filter_to_focus_candidates(&mut files, &focus, 5);

            let kept = files
                .into_iter()
                .map(|file| file.path)
                .collect::<FxHashSet<_>>();
            prop_assert!(kept.contains(&PathBuf::from("focus.ts")));
            prop_assert!(kept.contains(&PathBuf::from("matching.ts")));
        }
    }
}
