use ctx_history_capture::ProviderImportSummary;

use super::{
    format_bytes, format_count, plural, ImportFailureScope, ImportFailureType, PlannedImportSource,
    SourceInfo, SourceStats, LARGE_IMPORT_SOURCE_BYTES_WARNING, LARGE_IMPORT_SOURCE_FILES_WARNING,
};

#[derive(Debug)]
pub(crate) struct ImportSourceOutcome {
    pub(crate) index: usize,
    pub(crate) source: SourceInfo,
    pub(crate) stats: SourceStats,
    pub(crate) summary: ProviderImportSummary,
}

#[derive(Debug)]
pub(crate) struct ImportSourceFailure {
    pub(crate) index: usize,
    pub(crate) source: SourceInfo,
    pub(crate) stats: SourceStats,
    pub(crate) error: String,
    pub(crate) failure_scope: ImportFailureScope,
    pub(crate) failure_type: ImportFailureType,
    pub(crate) rejected_summary: Option<ProviderImportSummary>,
    pub(crate) system_error: Option<anyhow::Error>,
}

#[derive(Debug)]
pub(super) enum ImportSourceRun {
    Imported(ImportSourceOutcome),
    Failed(ImportSourceFailure),
}

impl ImportSourceRun {
    pub(super) fn index(&self) -> usize {
        match self {
            Self::Imported(outcome) => outcome.index,
            Self::Failed(failure) => failure.index,
        }
    }
}

pub(crate) fn should_parallelize_import(planned_sources: &[PlannedImportSource]) -> bool {
    let _ = planned_sources;
    false
}

pub(crate) fn large_import_notice(
    planned_sources: &[PlannedImportSource],
    planned_total_bytes: u64,
) -> Option<String> {
    let planned_total_files = planned_sources
        .iter()
        .map(|plan| plan.stats.files)
        .sum::<usize>();
    if planned_total_files < LARGE_IMPORT_SOURCE_FILES_WARNING
        && planned_total_bytes < LARGE_IMPORT_SOURCE_BYTES_WARNING
    {
        return None;
    }
    Some(format!(
        "Large first import: scanning {} existing history {} ({}). This may take a while.",
        format_count(planned_total_files),
        plural(planned_total_files, "file", "files"),
        format_bytes(planned_total_bytes)
    ))
}
