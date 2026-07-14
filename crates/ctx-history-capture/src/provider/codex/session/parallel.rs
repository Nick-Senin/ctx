use super::*;

pub(super) fn codex_normalization_is_skipped_only(
    normalization: &ProviderNormalizationResult,
) -> bool {
    normalization.summary.failed == 0
        && normalization.summary.skipped > 0
        && normalization.captures.is_empty()
        && normalization.files_touched.is_empty()
}

pub(crate) fn normalize_codex_session_paths_parallel(
    paths: &[PathBuf],
    options: &CodexSessionImportOptions,
    parallelism: usize,
) -> Result<Vec<(usize, PathBuf, ProviderNormalizationResult)>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    if parallelism <= 1 || paths.len() == 1 {
        let mut normalized = Vec::with_capacity(paths.len());
        for (index, path) in paths.iter().enumerate() {
            normalized.push((
                index,
                path.clone(),
                normalize_codex_session_path(path, options)?,
            ));
        }
        return Ok(normalized);
    }

    let chunk_size = paths.len().div_ceil(parallelism).max(1);
    let mut batches = thread::scope(|scope| {
        let mut handles = Vec::new();
        for (chunk_index, chunk) in paths.chunks(chunk_size).enumerate() {
            let chunk = chunk.to_vec();
            handles.push(scope.spawn(move || {
                let mut normalized = Vec::with_capacity(chunk.len());
                let base_index = chunk_index * chunk_size;
                for (offset, path) in chunk.iter().enumerate() {
                    normalized.push((
                        base_index + offset,
                        path.clone(),
                        normalize_codex_session_path(path, options)?,
                    ));
                }
                Result::<Vec<_>>::Ok(normalized)
            }));
        }
        let mut batches = Vec::with_capacity(handles.len());
        for handle in handles {
            batches.push(join_codex_import_worker(handle)?);
        }
        Result::<Vec<_>>::Ok(batches)
    })?;
    let total = batches.iter().map(Vec::len).sum();
    let mut normalized = Vec::with_capacity(total);
    for batch in batches.drain(..) {
        normalized.extend(batch);
    }
    normalized.sort_by_key(|(index, _, _)| *index);
    Ok(normalized)
}

pub(crate) fn join_codex_import_worker<T>(
    handle: thread::ScopedJoinHandle<'_, Result<T>>,
) -> Result<T> {
    handle
        .join()
        .map_err(|_| CaptureError::WorkerPanicked("Codex import"))?
}

fn normalize_codex_session_path(
    path: &Path,
    options: &CodexSessionImportOptions,
) -> Result<ProviderNormalizationResult> {
    CodexSessionJsonlAdapter.normalize_path(
        path,
        &ProviderAdapterContext {
            machine_id: options.machine_id.clone(),
            source_path: Some(path.to_path_buf()),
            source_root: options.source_path.clone(),
            imported_at: options.imported_at,
        },
    )
}
pub(crate) fn import_parallelism(path_count: usize) -> usize {
    if path_count <= 1 {
        return 1;
    }
    thread::available_parallelism()
        .ok()
        .map(usize::from)
        .unwrap_or(1)
        .min(path_count)
        .min(8)
}
pub(crate) fn apply_codex_session_import_bounds(
    paths: &mut Vec<PathBuf>,
    max_files: Option<usize>,
    max_total_bytes: Option<u64>,
) -> Result<usize> {
    paths.sort();
    if max_files.is_none() && max_total_bytes.is_none() {
        return Ok(0);
    }

    let original_len = paths.len();
    let mut selected = Vec::new();
    let mut total_bytes = 0u64;
    for path in paths.iter().rev() {
        if max_files.is_some_and(|limit| selected.len() >= limit) {
            continue;
        }
        let len = fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if max_total_bytes.is_some_and(|limit| total_bytes.saturating_add(len) > limit) {
            continue;
        }
        total_bytes = total_bytes.saturating_add(len);
        selected.push(path.clone());
    }
    selected.sort();
    let skipped = original_len.saturating_sub(selected.len());
    *paths = selected;
    Ok(skipped)
}
