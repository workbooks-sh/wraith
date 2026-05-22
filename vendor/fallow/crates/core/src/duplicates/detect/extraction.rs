//! Step 5: Clone group extraction from LCP intervals.

use super::FileData;

/// A raw clone group before conversion to `CloneGroup`.
pub(super) struct RawGroup {
    /// List of (`file_id`, `token_offset`) instances.
    pub(super) instances: Vec<(usize, usize)>,
    /// Clone length in tokens.
    pub(super) length: usize,
}

/// Extract clone groups from the suffix array and LCP array.
///
/// Uses a stack-based approach to find all maximal LCP intervals where the
/// minimum LCP value is >= `min_tokens`, and the interval contains suffixes
/// from at least two different positions (cross-file or non-overlapping
/// same-file).
pub(super) fn extract_clone_groups(
    sa: &[usize],
    lcp: &[usize],
    file_of: &[usize],
    file_offsets: &[usize],
    min_tokens: usize,
    files: &[FileData],
    focus_file_ids: Option<&[bool]>,
) -> Vec<RawGroup> {
    let n = sa.len();
    if n < 2 {
        return vec![];
    }

    // Stack-based LCP interval extraction.
    // Each stack entry: (lcp_value, start_index_in_sa).
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut groups: Vec<RawGroup> = Vec::new();
    let focus_prefix = focus_file_ids.map(|ids| build_focus_prefix(sa, file_of, ids));

    #[expect(
        clippy::needless_range_loop,
        reason = "i is used as a value, not just as an index"
    )]
    for i in 1..=n {
        let cur_lcp = if i < n { lcp[i] } else { 0 };
        let mut start = i;

        while let Some(&(top_lcp, top_start)) = stack.last() {
            if top_lcp <= cur_lcp {
                break;
            }
            stack.pop();
            start = top_start;

            if top_lcp >= min_tokens {
                // The interval [start-1 .. i-1] shares a common prefix of
                // length `top_lcp`.
                let interval_begin = start - 1;
                let interval_end = i; // exclusive
                if let Some(prefix) = focus_prefix.as_deref()
                    && !interval_has_focus(prefix, interval_begin, interval_end)
                {
                    continue;
                }

                if let Some(group) = build_raw_group(
                    sa,
                    file_of,
                    file_offsets,
                    files,
                    interval_begin,
                    interval_end,
                    top_lcp,
                ) {
                    groups.push(group);
                }
            }
        }

        if i < n {
            stack.push((cur_lcp, start));
        }
    }

    groups
}

fn build_focus_prefix(sa: &[usize], file_of: &[usize], focus_file_ids: &[bool]) -> Vec<usize> {
    let mut prefix = Vec::with_capacity(sa.len() + 1);
    prefix.push(0);
    for &pos in sa {
        let focused = file_of
            .get(pos)
            .copied()
            .filter(|&file_id| file_id != usize::MAX)
            .and_then(|file_id| focus_file_ids.get(file_id))
            .copied()
            .unwrap_or(false);
        prefix.push(prefix.last().copied().unwrap_or(0) + usize::from(focused));
    }
    prefix
}

fn interval_has_focus(focus_prefix: &[usize], begin: usize, end: usize) -> bool {
    focus_prefix[end] > focus_prefix[begin]
}

/// Build a `RawGroup` from an LCP interval, filtering to non-overlapping
/// instances.
fn build_raw_group(
    sa: &[usize],
    file_of: &[usize],
    file_offsets: &[usize],
    files: &[FileData],
    interval_begin: usize,
    interval_end: usize,
    length: usize,
) -> Option<RawGroup> {
    let mut instances: Vec<(usize, usize)> = Vec::with_capacity(interval_end - interval_begin);

    for &pos in &sa[interval_begin..interval_end] {
        let fid = file_of[pos];
        if fid == usize::MAX {
            continue; // sentinel position
        }
        let offset_in_file = pos - file_offsets[fid];

        // Verify the clone doesn't extend beyond the file boundary.
        if offset_in_file + length > files[fid].hashed_tokens.len() {
            continue;
        }

        instances.push((fid, offset_in_file));
    }

    if instances.len() < 2 {
        return None;
    }

    // Remove overlapping instances within the same file.
    // Sort by (file_id, offset) and remove overlaps.
    instances.sort_unstable();
    let mut deduped: Vec<(usize, usize)> = Vec::with_capacity(instances.len());
    for &(fid, offset) in &instances {
        if let Some(&(last_fid, last_offset)) = deduped.last()
            && fid == last_fid
            && offset < last_offset + length
        {
            continue; // overlapping within the same file
        }
        deduped.push((fid, offset));
    }

    if deduped.len() < 2 {
        return None;
    }

    Some(RawGroup {
        instances: deduped,
        length,
    })
}
