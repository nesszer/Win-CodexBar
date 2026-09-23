use super::*;
#[cfg(test)]
use std::cell::Cell;
use std::collections::VecDeque;

#[cfg(test)]
thread_local! { static CODEX_LINEAGE_GRAPH_BUILDS: Cell<usize> = const { Cell::new(0) }; }

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum CodexLineageGate {
    #[default]
    Eligible,
    Unsafe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CodexLineageDecision {
    Root,
    ParentAbsent,
    ParentReady(crate::core::CodexTotals),
    Unsafe,
}

struct CodexLineageNode {
    path: String,
    session_id: Option<String>,
    parent_id: Option<String>,
    candidate_index: Option<usize>,
    may_infer_missing_parent: bool,
    may_author_parent: bool,
    initially_unsafe: bool,
}

struct CodexLineageGraph {
    nodes: Vec<CodexLineageNode>,
    session_owners: HashMap<String, Vec<usize>>,
    parent_indices: Vec<Option<usize>>,
    gates: Vec<CodexLineageGate>,
    candidate_node_indices: Vec<usize>,
    ordered_candidate_indices: Vec<usize>,
}

impl CodexLineageGraph {
    fn new(cache: &CostUsageCache, candidates: Option<&[CodexPreparedCandidate]>) -> Self {
        #[cfg(test)]
        CODEX_LINEAGE_GRAPH_BUILDS.with(|builds| builds.set(builds.get() + 1));
        let candidate_paths = candidates
            .into_iter()
            .flatten()
            .map(|candidate| candidate.path.to_string_lossy().to_string())
            .collect::<HashSet<_>>();
        let mut cached_paths = cache
            .files
            .keys()
            .filter(|path| !candidate_paths.contains(*path))
            .cloned()
            .collect::<Vec<_>>();
        cached_paths.sort();

        let mut nodes = Vec::with_capacity(cached_paths.len() + candidate_paths.len());
        for path in cached_paths {
            let usage = &cache.files[&path];
            let uses_parent = codex_usage_uses_parent(usage);
            let locally_inferred = codex_fork_uses_local_inference(usage);
            nodes.push(CodexLineageNode {
                path,
                session_id: usage.codex_session_id.clone(),
                parent_id: uses_parent
                    .then(|| usage.codex_forked_from_id.clone())
                    .flatten(),
                candidate_index: None,
                may_infer_missing_parent: locally_inferred,
                may_author_parent: !locally_inferred && !usage.codex_unresolved_fork_parent,
                initially_unsafe: usage.codex_unresolved_fork_parent,
            });
        }

        let mut candidate_node_indices = Vec::new();
        if let Some(candidates) = candidates {
            candidate_node_indices.reserve(candidates.len());
            for (candidate_index, candidate) in candidates.iter().enumerate() {
                let cached = cache
                    .files
                    .get(&candidate.path.to_string_lossy().to_string());
                let cached_identity_matches = cached.is_some_and(|usage| {
                    let Ok(metadata) = fs::metadata(&candidate.path) else {
                        return false;
                    };
                    let expected = usage.codex_file_identity.as_deref();
                    let actual = JsonlScanner::codex_file_identity(&candidate.path, &metadata);
                    codex_file_identity_matches(expected, actual.as_deref())
                });
                let metadata_owns_identity = candidate.session_metadata.session_id.is_some();
                let uses_parent = if metadata_owns_identity {
                    candidate.session_metadata.lineage.uses_parent_baseline()
                        || candidate.session_metadata.forked_from_id.is_some()
                } else if cached_identity_matches {
                    cached.is_some_and(super::codex_usage_uses_parent)
                } else {
                    candidate.session_metadata.lineage.uses_parent_baseline()
                        || candidate.session_metadata.forked_from_id.is_some()
                };
                let session_id = candidate.session_metadata.session_id.clone().or_else(|| {
                    cached_identity_matches
                        .then(|| cached.and_then(|usage| usage.codex_session_id.clone()))
                        .flatten()
                });
                let parent_id = if metadata_owns_identity {
                    candidate.session_metadata.forked_from_id.clone()
                } else {
                    candidate
                        .session_metadata
                        .forked_from_id
                        .clone()
                        .or_else(|| {
                            (cached_identity_matches && uses_parent)
                                .then(|| {
                                    cached.and_then(|usage| usage.codex_forked_from_id.clone())
                                })
                                .flatten()
                        })
                };
                // Freshly read identity-bearing metadata owns this candidate's
                // current lineage state. Retain cached lineage flags only when
                // the bounded metadata read could not establish an identity.
                let cached_fallback = if metadata_owns_identity {
                    None
                } else {
                    cached_identity_matches.then_some(cached).flatten()
                };
                nodes.push(CodexLineageNode {
                    path: candidate.path.to_string_lossy().to_string(),
                    session_id,
                    parent_id: uses_parent.then_some(parent_id).flatten(),
                    candidate_index: Some(candidate_index),
                    may_infer_missing_parent: candidate.session_metadata.is_subagent
                        || cached_fallback.is_some_and(super::codex_fork_uses_local_inference),
                    may_author_parent: cached_fallback.is_none_or(|usage| {
                        !super::codex_fork_uses_local_inference(usage)
                            && !usage.codex_unresolved_fork_parent
                    }),
                    initially_unsafe: cached_fallback
                        .is_some_and(|usage| usage.codex_unresolved_fork_parent),
                });
                candidate_node_indices.push(nodes.len() - 1);
            }
        }

        let mut session_owners = HashMap::<String, Vec<usize>>::new();
        for (index, node) in nodes.iter().enumerate() {
            if let Some(session_id) = node.session_id.as_ref() {
                session_owners
                    .entry(session_id.clone())
                    .or_default()
                    .push(index);
            }
        }

        let mut gates = nodes
            .iter()
            .map(|node| {
                if node.initially_unsafe {
                    CodexLineageGate::Unsafe
                } else {
                    CodexLineageGate::Eligible
                }
            })
            .collect::<Vec<_>>();
        for owners in session_owners.values().filter(|owners| owners.len() > 1) {
            for &index in owners {
                gates[index] = CodexLineageGate::Unsafe;
            }
        }

        let mut parent_indices = vec![None; nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            let Some(parent_id) = node.parent_id.as_ref() else {
                continue;
            };
            match session_owners.get(parent_id) {
                Some(owners) if owners.len() == 1 => parent_indices[index] = Some(owners[0]),
                Some(_) => {
                    gates[index] = CodexLineageGate::Unsafe;
                }
                None if !node.may_infer_missing_parent => {
                    gates[index] = CodexLineageGate::Unsafe;
                }
                None => {}
            }
        }

        // This single topological pass both rejects cycles/unsafe ancestry and
        // orders candidates. Cached-parent validation consumes the same gates.
        let mut completed = vec![false; nodes.len()];
        let mut ordered_candidate_indices = Vec::with_capacity(candidate_node_indices.len());
        let mut children = vec![Vec::new(); nodes.len()];
        let mut ready = VecDeque::new();
        for (index, parent) in parent_indices.iter().enumerate() {
            match parent {
                Some(parent_index) => children[*parent_index].push(index),
                None if gates[index] == CodexLineageGate::Eligible => ready.push_back(index),
                None => {}
            }
        }
        while let Some(index) = ready.pop_front() {
            if completed[index] || gates[index] == CodexLineageGate::Unsafe {
                continue;
            }
            completed[index] = true;
            if let Some(candidate_index) = nodes[index].candidate_index {
                ordered_candidate_indices.push(candidate_index);
            }
            if nodes[index].may_author_parent {
                for child in &children[index] {
                    if gates[*child] == CodexLineageGate::Eligible {
                        ready.push_back(*child);
                    }
                }
            }
        }

        for index in 0..nodes.len() {
            if !completed[index] {
                gates[index] = CodexLineageGate::Unsafe;
                if let Some(candidate_index) = nodes[index].candidate_index {
                    ordered_candidate_indices.push(candidate_index);
                }
            }
        }

        Self {
            nodes,
            session_owners,
            parent_indices,
            gates,
            candidate_node_indices,
            ordered_candidate_indices,
        }
    }

    fn unique_owner(&self, session_id: &str) -> Result<Option<usize>, ()> {
        match self.session_owners.get(session_id).map(Vec::as_slice) {
            None | Some([]) => Ok(None),
            Some([index]) => Ok(Some(*index)),
            Some(_) => Err(()),
        }
    }

    fn apply_candidate_plan(
        &self,
        candidates: &mut Vec<CodexPreparedCandidate>,
        sessions_dirs: &[PathBuf],
        range: &CostUsageDayRange,
    ) -> Vec<String> {
        if !candidates.is_empty() {
            for (candidate_index, candidate) in candidates.iter_mut().enumerate() {
                let node_index = self.candidate_node_indices[candidate_index];
                candidate.lineage_gate = self.gates[node_index];
                candidate.parent_owner_expected = self.parent_indices[node_index].is_some();
            }

            let mut remaining = candidates.drain(..).map(Some).collect::<Vec<_>>();
            for candidate_index in &self.ordered_candidate_indices {
                candidates.push(
                    remaining[*candidate_index]
                        .take()
                        .expect("candidate is ordered once"),
                );
            }
        }

        // Keep the complete graph for parent resolution, but invalidate only
        // unsafe cached nodes in the active range or in the ancestor closure
        // required to resolve an active node.
        let mut relevant = vec![false; self.nodes.len()];
        let mut pending = VecDeque::new();
        for (index, node) in self.nodes.iter().enumerate() {
            if super::is_codex_path_in_scan_window(Path::new(&node.path), sessions_dirs, range) {
                relevant[index] = true;
                pending.push_back(index);
            }
        }
        while let Some(index) = pending.pop_front() {
            let Some(parent_id) = self.nodes[index].parent_id.as_deref() else {
                continue;
            };
            if let Some(owners) = self.session_owners.get(parent_id) {
                for &owner in owners {
                    if !relevant[owner] {
                        relevant[owner] = true;
                        pending.push_back(owner);
                    }
                }
            }
        }

        self.nodes
            .iter()
            .zip(&self.gates)
            .enumerate()
            .filter(|(index, (node, gate))| {
                relevant[*index]
                    && node.candidate_index.is_none()
                    && **gate == CodexLineageGate::Unsafe
                    && !node.initially_unsafe
            })
            .map(|(_, (node, _))| node.path.clone())
            .collect()
    }
}

pub(super) struct CodexLineagePlanner {
    graph: Option<CodexLineageGraph>,
}

impl CodexLineagePlanner {
    pub(super) fn new(cache: &CostUsageCache) -> Self {
        Self {
            graph: Self::needs_graph(cache, None).then(|| CodexLineageGraph::new(cache, None)),
        }
    }

    pub(super) fn plan_candidates_by_lineage(
        cache: &CostUsageCache,
        candidates: &mut Vec<CodexPreparedCandidate>,
        sessions_dirs: &[PathBuf],
        range: &CostUsageDayRange,
    ) -> (Self, Vec<String>) {
        let graph = Self::needs_graph(cache, Some(candidates))
            .then(|| CodexLineageGraph::new(cache, Some(candidates)));
        let unsafe_paths = graph.as_ref().map_or_else(Vec::new, |graph| {
            graph.apply_candidate_plan(candidates, sessions_dirs, range)
        });
        (Self { graph }, unsafe_paths)
    }

    fn needs_graph(cache: &CostUsageCache, candidates: Option<&[CodexPreparedCandidate]>) -> bool {
        cache.files.values().any(super::codex_usage_uses_parent)
            || candidates.is_some_and(|items| {
                items.iter().any(|candidate| {
                    candidate.session_metadata.lineage.uses_parent_baseline()
                        || candidate.session_metadata.forked_from_id.is_some()
                })
            })
    }

    #[cfg(test)]
    pub(crate) fn reset_graph_build_count() {
        CODEX_LINEAGE_GRAPH_BUILDS.with(|count| count.set(0));
    }
    #[cfg(test)]
    pub(crate) fn graph_build_count() -> usize {
        CODEX_LINEAGE_GRAPH_BUILDS.with(Cell::get)
    }

    fn graph(&self) -> Option<&CodexLineageGraph> {
        self.graph.as_ref()
    }

    pub(super) fn cached_usage_is_safe(
        &self,
        cache: &CostUsageCache,
        usage: &CostUsageFileUsage,
    ) -> bool {
        let locally_resolved = super::codex_fork_uses_local_inference(usage);
        match self.decision_for_usage(cache, usage) {
            CodexLineageDecision::Root => true,
            CodexLineageDecision::ParentAbsent => locally_resolved,
            CodexLineageDecision::ParentReady(_) => !locally_resolved,
            CodexLineageDecision::Unsafe => false,
        }
    }

    pub(super) fn decision_for_scan(
        &self,
        cache: &CostUsageCache,
        uses_parent: bool,
        gate: CodexLineageGate,
        parent_id: Option<&str>,
        fork_timestamp: Option<&str>,
        parent_owner_expected: bool,
    ) -> CodexLineageDecision {
        if gate == CodexLineageGate::Unsafe {
            return CodexLineageDecision::Unsafe;
        }
        if !uses_parent {
            return CodexLineageDecision::Root;
        }
        parent_id.map_or(CodexLineageDecision::Unsafe, |parent_id| {
            self.resolve_parent(cache, parent_id, fork_timestamp, parent_owner_expected)
        })
    }

    pub(super) fn decision_for_usage(
        &self,
        cache: &CostUsageCache,
        usage: &CostUsageFileUsage,
    ) -> CodexLineageDecision {
        if usage.codex_unresolved_fork_parent {
            return CodexLineageDecision::Unsafe;
        }
        if !super::codex_usage_uses_parent(usage) {
            return CodexLineageDecision::Root;
        }
        usage
            .codex_forked_from_id
            .as_deref()
            .map_or(CodexLineageDecision::Unsafe, |parent_id| {
                self.resolve_parent(
                    cache,
                    parent_id,
                    usage.codex_fork_timestamp.as_deref(),
                    false,
                )
            })
    }

    /// Resolve one parent identity through the persisted graph. Absence is
    /// distinct from ambiguity and transitive unsafety so local inference is
    /// allowed only when no owner exists at all.
    fn resolve_parent(
        &self,
        cache: &CostUsageCache,
        parent_session_id: &str,
        child_fork_timestamp: Option<&str>,
        parent_owner_expected: bool,
    ) -> CodexLineageDecision {
        let Some(graph) = self.graph() else {
            return CodexLineageDecision::Unsafe;
        };
        let node_index = match graph.unique_owner(parent_session_id) {
            Ok(None) if !parent_owner_expected => return CodexLineageDecision::ParentAbsent,
            Ok(None) | Err(()) => return CodexLineageDecision::Unsafe,
            Ok(Some(index)) => index,
        };
        self.parent_owner_baseline(cache, node_index, child_fork_timestamp)
            .map_or(
                CodexLineageDecision::Unsafe,
                CodexLineageDecision::ParentReady,
            )
    }

    fn parent_owner_baseline(
        &self,
        cache: &CostUsageCache,
        node_index: usize,
        child_fork_timestamp: Option<&str>,
    ) -> Option<crate::core::CodexTotals> {
        let graph = self.graph()?;
        let node = graph.nodes.get(node_index)?;
        if graph.gates[node_index] == CodexLineageGate::Unsafe || !node.may_author_parent {
            return None;
        }
        let usage = cache.files.get(&node.path)?;
        if usage.codex_unresolved_fork_parent
            || usage.codex_token_timestamps_monotonic != Some(true)
            || super::codex_fork_uses_local_inference(usage)
        {
            return None;
        }

        if super::codex_usage_uses_parent(usage) {
            let inherited = usage
                .codex_fork_accounting_state
                .as_ref()?
                .inherited_totals
                .as_ref()?;
            let parent_index = graph.parent_indices[node_index]?;
            let baseline = self.parent_owner_baseline(
                cache,
                parent_index,
                usage.codex_fork_timestamp.as_deref(),
            )?;
            if &baseline != inherited {
                return None;
            }
        }

        let metadata = fs::metadata(&node.path).ok()?;
        let expected_identity = usage.codex_file_identity.as_ref()?;
        let actual_identity = JsonlScanner::codex_file_identity(Path::new(&node.path), &metadata)?;
        if expected_identity != &actual_identity {
            return None;
        }
        #[allow(clippy::cast_possible_wrap, reason = "session file sizes fit i64")]
        let size = metadata.len().min(i64::MAX as u64) as i64;
        if usage.mtime_unix_ms != system_time_to_unix_ms(metadata.modified().ok())
            || usage.size != size
            || usage.parsed_bytes.unwrap_or(0) < size
        {
            return None;
        }
        let last_totals = usage.last_totals.clone()?;
        let last_token_timestamp = usage.codex_last_token_timestamp.as_deref()?;
        let child_fork_timestamp = child_fork_timestamp?;
        JsonlScanner::codex_timestamp_at_or_before(last_token_timestamp, child_fork_timestamp)
            .then_some(last_totals)
    }
}

impl CodexLineageDecision {
    pub(super) fn accounting_mode(
        &self,
        matching_cached_state: Option<&CodexForkAccountingState>,
        metadata: &CodexSessionMetadata,
        paginated_continuation: bool,
    ) -> CodexAccountingMode {
        match self {
            Self::Root => CodexAccountingMode::Standard,
            Self::Unsafe => CodexAccountingMode::Unresolved,
            Self::ParentReady(baseline) => {
                let replaces_cached_state = matching_cached_state.is_some_and(|state| {
                    state.locally_resolved || state.inherited_totals.as_ref() != Some(baseline)
                });
                let cached_parent_state = matching_cached_state.filter(|state| {
                    !state.locally_resolved && state.inherited_totals.as_ref() == Some(baseline)
                });
                CodexAccountingMode::Baseline {
                    baseline: baseline.clone(),
                    paginated_continuation,
                    remaining_inherited_totals: cached_parent_state
                        .and_then(|state| state.remaining_inherited_totals.clone()),
                    provenance: CodexBaselineProvenance::ValidatedParent {
                        replaces_cached_state,
                    },
                }
            }
            Self::ParentAbsent => {
                if let Some(state) = matching_cached_state
                    && let Some(baseline) = state.inherited_totals.clone()
                {
                    return CodexAccountingMode::Baseline {
                        baseline,
                        paginated_continuation,
                        remaining_inherited_totals: state.remaining_inherited_totals.clone(),
                        provenance: if state.locally_resolved {
                            CodexBaselineProvenance::CachedLocalInference
                        } else {
                            CodexBaselineProvenance::CachedValidatedParent
                        },
                    };
                }
                if metadata.is_subagent {
                    CodexAccountingMode::InferSubagent {
                        start_ordinal: metadata.subagent_history_start_ordinal,
                    }
                } else {
                    CodexAccountingMode::Unresolved
                }
            }
        }
    }
}

pub(super) fn cached_codex_file_is_fresh(
    cache: &CostUsageCache,
    planner: &CodexLineagePlanner,
    entry: &CostUsageFileUsage,
    cache_covers_range: bool,
    mtime_unix_ms: i64,
    size: i64,
) -> bool {
    cache_covers_range
        && !entry.codex_unresolved_fork_parent
        && entry.mtime_unix_ms == mtime_unix_ms
        && entry.size == size
        && codex_scan_target_size(entry) == size
        && entry.parsed_bytes.unwrap_or(0) >= size
        && planner.cached_usage_is_safe(cache, entry)
}

pub(super) fn codex_file_identity_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    expected
        .zip(actual)
        .is_some_and(|(expected, actual)| expected == actual)
}

pub(super) fn cached_codex_file_is_complete_for_range(
    cache: &CostUsageCache,
    planner: &CodexLineagePlanner,
    path_key: &str,
    range: &CostUsageDayRange,
) -> bool {
    JsonlScanner::cache_covers_range(cache, range)
        && cache.files.get(path_key).is_some_and(|usage| {
            let Ok(metadata) = fs::metadata(path_key) else {
                return false;
            };
            let identity_matches = codex_file_identity_matches(
                usage.codex_file_identity.as_deref(),
                JsonlScanner::codex_file_identity(Path::new(path_key), &metadata).as_deref(),
            );
            #[allow(clippy::cast_possible_wrap, reason = "session file sizes fit i64")]
            let size = metadata.len().min(i64::MAX as u64) as i64;
            identity_matches
                && usage.mtime_unix_ms == system_time_to_unix_ms(metadata.modified().ok())
                && usage.size == size
                && codex_scan_target_size(usage) == size
                && usage.parsed_bytes.unwrap_or(0) >= size
                && !usage.codex_unresolved_fork_parent
                // Reconsider locally inferred children after this pass has
                // had a chance to discover and cache their parent.
                && !super::codex_fork_uses_local_inference(usage)
                && planner.cached_usage_is_safe(cache, usage)
        })
}

/// Process cached local-inference children after all other candidates. A
/// parent discovered in this pass must enter the cache before its unchanged
/// child can decide whether the inferred baseline is still authoritative.
pub(super) fn defer_codex_locally_inferred_candidates(
    candidates: &mut Vec<CodexScanCandidate>,
    cache: &CostUsageCache,
) {
    if candidates.len() < 2 {
        return;
    }

    let mut other = Vec::with_capacity(candidates.len());
    let mut locally_inferred = Vec::new();
    for candidate in candidates.drain(..) {
        let path_key = candidate.path.to_string_lossy();
        if cache
            .files
            .get(path_key.as_ref())
            .is_some_and(super::codex_fork_uses_local_inference)
        {
            locally_inferred.push(candidate);
        } else {
            other.push(candidate);
        }
    }
    other.extend(locally_inferred);
    candidates.extend(other);
}

pub(super) fn invalidate_codex_unsafe_lineage(cache: &mut CostUsageCache, paths: &[String]) {
    for path in paths {
        let Some(usage) = cache.files.get_mut(path) else {
            continue;
        };
        usage.days.clear();
        usage.parsed_bytes = Some(0);
        usage.codex_scan_target_size = None;
        usage.last_model = None;
        usage.last_totals = None;
        usage.codex_token_timestamps_monotonic = None;
        usage.codex_last_token_timestamp = None;
        usage.codex_fork_accounting_state = None;
        usage.codex_unresolved_fork_parent = true;
    }
}

/// Give paths already in the durable queue their saved turn before newly
/// discovered dirty paths. The scanner appends unfinished paths after this
/// pass, making the queue a round-robin cursor instead of a newest-first loop.
pub(super) fn prioritize_codex_pending_candidates(
    candidates: &mut Vec<CodexScanCandidate>,
    pending_paths: &[String],
) {
    if pending_paths.is_empty() || candidates.len() < 2 {
        return;
    }

    let mut pending = Vec::with_capacity(candidates.len());
    let mut fresh = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        let key = candidate.path.to_string_lossy();
        if pending_paths
            .iter()
            .any(|path| path.as_str() == key.as_ref())
        {
            pending.push(candidate);
        } else {
            fresh.push(candidate);
        }
    }
    pending.extend(fresh);
    candidates.extend(pending);
}

/// Return the persisted logical end of a Codex parse. Older cache entries did
/// not have a frozen target, so their physical size remains the safe fallback.
pub(super) fn codex_scan_target_size(usage: &CostUsageFileUsage) -> i64 {
    usage.codex_scan_target_size.unwrap_or(usage.size).max(0)
}

/// A cached prefix is resumable toward its original target when the target is
/// still present in the current file. The caller separately validates the
/// byte-boundary/parser-state invariants before using the cursor.
pub(super) fn codex_resumable_scan_target_size(
    metadata_size: i64,
    usage: &CostUsageFileUsage,
) -> Option<i64> {
    let parsed_bytes = usage.parsed_bytes.unwrap_or(usage.size).max(0);
    let target_size = codex_scan_target_size(usage);
    (parsed_bytes < target_size && target_size <= metadata_size).then_some(target_size)
}

/// Whether a logically complete prefix still has physical bytes that must be
/// revisited. This is the catch-up cursor for a growing rollout or a retained
/// incomplete tail.
pub(super) fn codex_logical_target_has_unconsumed_tail(
    metadata_size: i64,
    usage: &CostUsageFileUsage,
) -> bool {
    let parsed_bytes = usage.parsed_bytes.unwrap_or(usage.size).max(0);
    let target_size = codex_scan_target_size(usage);
    parsed_bytes < metadata_size || target_size < metadata_size
}

/// Whether a completed empty fragment has no parser state worth resuming.
pub(super) fn codex_cached_entry_is_complete_empty_fragment(usage: &CostUsageFileUsage) -> bool {
    usage.days.is_empty()
        && usage.parsed_bytes == Some(usage.size)
        && usage.codex_scan_target_size == Some(usage.size)
        && usage.last_model.is_none()
        && usage.last_totals.is_none()
        && usage.codex_last_token_timestamp.is_none()
        && usage.codex_token_timestamps_monotonic != Some(false)
}
