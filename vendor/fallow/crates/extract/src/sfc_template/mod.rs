//! Heuristic template scanners for Vue and Svelte single-file components.
//!
//! This module only handles markup-visible import usage that the JavaScript AST
//! cannot see. It is intentionally conservative: we support the common template
//! constructs that can be analyzed reliably with lightweight scanning, without
//! pretending to be a full framework compiler.

pub mod angular;
mod scanners;
mod shared;
mod svelte;
mod vue;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::template_usage::TemplateUsage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfcKind {
    /// Vue single-file components.
    Vue,
    /// Svelte single-file components.
    Svelte,
}

/// Collect template-visible import usage from Vue or Svelte markup.
#[cfg(test)]
pub fn collect_template_usage(
    kind: SfcKind,
    source: &str,
    imported_bindings: &FxHashSet<String>,
) -> TemplateUsage {
    match kind {
        SfcKind::Vue => vue::collect_template_usage(source, imported_bindings),
        SfcKind::Svelte => svelte::collect_template_usage(source, imported_bindings),
    }
}

/// Collect template-visible usage, including framework template references to
/// script-local instance bindings such as `const counter = new Counter()`.
pub fn collect_template_usage_with_bound_targets(
    kind: SfcKind,
    source: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
) -> TemplateUsage {
    match kind {
        SfcKind::Vue => {
            vue::collect_template_usage_with_bound_targets(source, imported_bindings, bound_targets)
        }
        SfcKind::Svelte => svelte::collect_template_usage_with_bound_targets(
            source,
            imported_bindings,
            bound_targets,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imported(names: &[&str]) -> FxHashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn svelte_template_usage_marks_named_imports_used() {
        let usage = collect_template_usage(
            SfcKind::Svelte,
            "<script>import { formatDate } from './utils';</script><p>{formatDate(value)}</p>",
            &imported(&["formatDate"]),
        );

        assert!(usage.used_bindings.contains("formatDate"));
    }

    #[test]
    fn svelte_template_usage_retains_namespace_members() {
        let usage = collect_template_usage(
            SfcKind::Svelte,
            "<script>import * as utils from './utils';</script><p>{utils.formatDate(value)}</p>",
            &imported(&["utils"]),
        );

        assert!(usage.used_bindings.contains("utils"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "utils");
        assert_eq!(usage.member_accesses[0].member, "formatDate");
    }

    #[test]
    fn vue_template_usage_marks_named_imports_used() {
        let usage = collect_template_usage(
            SfcKind::Vue,
            "<script setup>import { formatDate } from './utils';</script><template><p>{{ formatDate(value) }}</p></template>",
            &imported(&["formatDate"]),
        );

        assert!(usage.used_bindings.contains("formatDate"));
    }

    #[test]
    fn vue_template_usage_treats_event_handlers_as_statements() {
        let usage = collect_template_usage(
            SfcKind::Vue,
            "<script setup>import { increment } from './utils';</script><template><button @click=\"count += increment(step)\">Add</button></template>",
            &imported(&["increment"]),
        );

        assert!(usage.used_bindings.contains("increment"));
    }
}
