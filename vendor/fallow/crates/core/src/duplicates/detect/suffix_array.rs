//! Step 3: Suffix array construction using O(N log N) prefix-doubling with radix sort.

/// Build a suffix array using the O(N log N) prefix-doubling algorithm with
/// radix sort.
///
/// Returns `sa` where `sa[i]` is the starting position of the i-th
/// lexicographically smallest suffix in `text`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ranks are bounded by text length which fits in usize"
)]
pub(super) fn build_suffix_array(text: &[i64]) -> Vec<usize> {
    let n = text.len();
    if n == 0 {
        return vec![];
    }

    // Initial ranks based on raw values. Shift so sentinels (negative) sort
    // before all real tokens.
    let min_val = text.iter().copied().min().unwrap_or(0);
    let mut rank: Vec<i64> = text.iter().map(|&v| v - min_val).collect();
    let mut sa: Vec<usize> = (0..n).collect();
    let mut tmp: Vec<i64> = vec![0; n];
    let mut k: usize = 1;
    let mut iterations = 0u32;

    // Scratch buffers reused across all iterations to avoid per-iteration allocations.
    let mut sa_tmp: Vec<usize> = vec![0; n];
    let mut counts: Vec<usize> = Vec::new();

    // Track max rank across iterations: the rank update already computes it
    // as tmp[sa[n-1]], so we only need the initial scan once.
    let mut max_rank = rank.iter().copied().max().unwrap_or(0) as usize;

    while k < n {
        iterations += 1;

        // Two-pass radix sort: sort by secondary key (rank[i+k]) first,
        // then by primary key (rank[i]). Each pass is O(N + K) where
        // K = max_rank + 2 (including the -1 sentinel rank).
        let bucket_count = max_rank + 2; // ranks 0..=max_rank plus -1 mapped to 0

        // Pass 1: sort by secondary key (rank at offset k).
        counts.clear();
        counts.resize(bucket_count + 1, 0);
        for &i in &sa {
            let r2 = if i + k < n {
                rank[i + k] as usize + 1
            } else {
                0
            };
            counts[r2] += 1;
        }
        // Prefix sum.
        let mut sum = 0;
        for c in &mut counts {
            let v = *c;
            *c = sum;
            sum += v;
        }
        for &i in &sa {
            let r2 = if i + k < n {
                rank[i + k] as usize + 1
            } else {
                0
            };
            sa_tmp[counts[r2]] = i;
            counts[r2] += 1;
        }

        // Pass 2: sort by primary key (rank[i]), stable.
        // No +1 offset needed here: rank[i] is always >= 0 because the
        // initial ranks are shifted by min_val, and subsequent iterations
        // assign ranks starting from 0.
        counts.fill(0);
        counts.resize(bucket_count + 1, 0);
        for &i in &sa_tmp {
            let r1 = rank[i] as usize;
            counts[r1] += 1;
        }
        sum = 0;
        for c in &mut counts {
            let v = *c;
            *c = sum;
            sum += v;
        }
        for &i in &sa_tmp {
            let r1 = rank[i] as usize;
            sa[counts[r1]] = i;
            counts[r1] += 1;
        }

        // Compute new ranks.
        tmp[sa[0]] = 0;
        for i in 1..n {
            let prev = sa[i - 1];
            let curr = sa[i];
            let same = rank[prev] == rank[curr] && {
                let rp2 = if prev + k < n { rank[prev + k] } else { -1 };
                let rc2 = if curr + k < n { rank[curr + k] } else { -1 };
                rp2 == rc2
            };
            tmp[curr] = tmp[prev] + i64::from(!same);
        }

        // Early exit when all ranks are unique.
        let new_max_rank = tmp[sa[n - 1]];
        std::mem::swap(&mut rank, &mut tmp);

        if new_max_rank as usize == n - 1 {
            break;
        }

        // Carry forward max rank for next iteration (avoids O(n) rescan).
        max_rank = new_max_rank as usize;
        k *= 2;
    }

    tracing::trace!(n, iterations, "suffix array constructed");
    sa
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the suffix array property: for every adjacent pair SA[i], SA[i+1],
    /// the suffix starting at SA[i] is lexicographically <= the suffix at SA[i+1].
    fn assert_suffix_order(text: &[i64], sa: &[usize]) {
        assert_eq!(
            text.len(),
            sa.len(),
            "suffix array length must equal text length"
        );
        for i in 1..sa.len() {
            let suffix_a = &text[sa[i - 1]..];
            let suffix_b = &text[sa[i]..];
            assert!(
                suffix_a <= suffix_b,
                "suffix order violated at SA[{}]={} vs SA[{}]={}: {:?} > {:?}",
                i - 1,
                sa[i - 1],
                i,
                sa[i],
                suffix_a,
                suffix_b,
            );
        }
    }

    /// Verify the suffix array is a permutation of 0..n.
    fn assert_is_permutation(sa: &[usize], n: usize) {
        let mut seen = vec![false; n];
        for &idx in sa {
            assert!(idx < n, "suffix array index {idx} out of bounds (n={n})");
            assert!(!seen[idx], "duplicate index {idx} in suffix array");
            seen[idx] = true;
        }
    }

    #[test]
    fn empty_input() {
        let sa = build_suffix_array(&[]);
        assert!(sa.is_empty());
    }

    #[test]
    fn single_element() {
        let text = [42];
        let sa = build_suffix_array(&text);
        assert_eq!(sa, vec![0]);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn two_elements_already_sorted() {
        let text = [1, 2];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 2);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn two_elements_reverse_sorted() {
        let text = [2, 1];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 2);
        assert_suffix_order(&text, &sa);
        // Suffix at 1 is [1], suffix at 0 is [2, 1]. [1] < [2, 1].
        assert_eq!(sa[0], 1);
        assert_eq!(sa[1], 0);
    }

    #[test]
    fn already_sorted_input() {
        let text = [1, 2, 3, 4, 5];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 5);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn reverse_sorted_input() {
        let text = [5, 4, 3, 2, 1];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 5);
        assert_suffix_order(&text, &sa);
        // The shortest suffix [1] should come first.
        assert_eq!(sa[0], 4);
    }

    #[test]
    fn all_identical_elements() {
        let text = [7, 7, 7, 7];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 4);
        assert_suffix_order(&text, &sa);
        // With all identical values, shorter suffixes are "smaller":
        // [7] < [7,7] < [7,7,7] < [7,7,7,7]
        assert_eq!(sa, vec![3, 2, 1, 0]);
    }

    #[test]
    fn mixed_input_banana_like() {
        // Classic "banana" test adapted to i64: b=2, a=1, n=3 -> [2,1,3,1,3,1]
        let text = [2, 1, 3, 1, 3, 1];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 6);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn input_with_negative_sentinels() {
        // Sentinels are negative values used to separate file token sequences.
        // They should sort before all non-negative tokens.
        let text = [3, 1, 2, -1, 4, 5, -2, 6];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 8);
        assert_suffix_order(&text, &sa);
        // The most negative value (-2) starts the lexicographically smallest suffix.
        assert_eq!(sa[0], 6);
    }

    #[test]
    fn single_sentinel_only() {
        let text = [-1];
        let sa = build_suffix_array(&text);
        assert_eq!(sa, vec![0]);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn multiple_sentinels_decreasing() {
        // Simulates concatenation of three empty files: only sentinels.
        let text = [-1, -2];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 2);
        assert_suffix_order(&text, &sa);
        // [-2] < [-1, -2] because -2 < -1
        assert_eq!(sa[0], 1);
        assert_eq!(sa[1], 0);
    }

    #[test]
    fn realistic_concatenated_files() {
        // Two "files" [10, 20, 30] and [20, 30, 40] separated by sentinel -1.
        let text = [10, 20, 30, -1, 20, 30, 40];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 7);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn repeated_pattern() {
        // "abab" pattern: triggers the prefix-doubling loop multiple times.
        let text = [1, 2, 1, 2];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 4);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn large_input_stress() {
        // Verify correctness property on a larger input that exercises
        // multiple iterations of the prefix-doubling loop.
        let text: Vec<i64> = (0..256).map(|i| i64::from(i % 17)).collect();
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 256);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn large_identical_stress() {
        // All-identical larger input: worst case for rank convergence.
        let text = vec![42i64; 128];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 128);
        assert_suffix_order(&text, &sa);
        // Shorter suffixes must come first.
        for (i, &pos) in sa.iter().enumerate() {
            assert_eq!(pos, 127 - i);
        }
    }

    #[test]
    fn alternating_sentinels_and_tokens() {
        // token, sentinel, token, sentinel pattern.
        let text = [5, -1, 5, -2];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 4);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn all_same_with_trailing_sentinel() {
        // Common pattern: file of identical tokens followed by a sentinel.
        let text = [3, 3, 3, -1];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 4);
        assert_suffix_order(&text, &sa);
        // The sentinel is the smallest value, so its suffix comes first.
        assert_eq!(sa[0], 3);
    }

    #[test]
    fn suffix_array_is_inverse_of_rank() {
        // For any valid suffix array, rank[sa[i]] == i.
        let text = [4, 2, 3, 1, 5];
        let sa = build_suffix_array(&text);
        let n = text.len();
        let mut rank = vec![0usize; n];
        for i in 0..n {
            rank[sa[i]] = i;
        }
        for i in 0..n {
            assert_eq!(
                sa[rank[i]], i,
                "rank/sa inverse property violated at position {i}"
            );
        }
    }
}
