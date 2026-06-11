use lsp_types::{SemanticToken, SemanticTokensEdit};

#[derive(Debug)]
pub(crate) struct CachedSemanticTokens {
    pub(crate) result_id: String,
    pub(crate) data: Vec<u32>,
}

pub(crate) fn semantic_token_structs(data: &[u32]) -> Vec<SemanticToken> {
    data.chunks_exact(5)
        .map(|c| SemanticToken {
            delta_line: c[0],
            delta_start: c[1],
            length: c[2],
            token_type: c[3],
            token_modifiers_bitset: c[4],
        })
        .collect()
}

// Diffs whole tokens: lsp-types carries edit data as SemanticToken structs, so edits stay 5-u32 aligned.
pub(crate) fn semantic_token_edits(previous: &[u32], current: &[u32]) -> Vec<SemanticTokensEdit> {
    let previous_tokens = previous.len() / 5;
    let current_tokens = current.len() / 5;
    let prev_chunks = previous.chunks_exact(5);
    let cur_chunks = current.chunks_exact(5);
    let prefix = prev_chunks
        .clone()
        .zip(cur_chunks.clone())
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = previous_tokens.min(current_tokens) - prefix;
    let suffix = prev_chunks
        .rev()
        .zip(cur_chunks.rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    if previous_tokens == current_tokens && prefix == previous_tokens {
        return Vec::new();
    }
    let inserted = &current[prefix * 5..current.len() - suffix * 5];
    vec![SemanticTokensEdit {
        start: wire_u32(prefix * 5),
        delete_count: wire_u32((previous_tokens - prefix - suffix) * 5),
        data: Some(semantic_token_structs(inserted)),
    }]
}

/// Saturates: LSP wire offsets are u32; values past that clamp to `u32::MAX`.
fn wire_u32(n: usize) -> u32 {
    n.try_into().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use lsp_types::SemanticTokensEdit;
    use rstest::rstest;

    use super::semantic_token_edits;

    const T1: [u32; 5] = [0, 0, 5, 0, 0];
    const T2: [u32; 5] = [1, 0, 3, 1, 0];
    const T3: [u32; 5] = [1, 4, 2, 2, 0];
    const TX: [u32; 5] = [0, 6, 1, 3, 1];

    fn apply(previous: &[u32], edits: &[SemanticTokensEdit]) -> Vec<u32> {
        let mut result = previous.to_vec();
        for edit in edits {
            let start = edit.start as usize;
            let inserted: Vec<u32> = edit
                .data
                .iter()
                .flatten()
                .flat_map(|t| {
                    [
                        t.delta_line,
                        t.delta_start,
                        t.length,
                        t.token_type,
                        t.token_modifiers_bitset,
                    ]
                })
                .collect();
            result.splice(start..start + edit.delete_count as usize, inserted);
        }
        result
    }

    #[test]
    fn identical_arrays_produce_no_edits() {
        let tokens = [T1, T2].concat();
        let edits = semantic_token_edits(&tokens, &tokens);
        assert!(edits.is_empty(), "identical arrays must yield no edits");
    }

    #[rstest]
    #[case::append("append", &[T1], &[T1, T2])]
    #[case::prepend("prepend", &[T2], &[T1, T2])]
    #[case::middle_change("middle change", &[T1, T2, T3], &[T1, TX, T3])]
    #[case::shrink("shrink", &[T1, T2, T3], &[T1, T3])]
    #[case::repeated_tokens_shrink("repeated tokens shrink", &[T1, T1, T1], &[T1, T1])]
    #[case::from_empty("from empty", &[], &[T1, T2])]
    #[case::to_empty("to empty", &[T1, T2], &[])]
    #[case::full_replace("full replace", &[T1, T2], &[TX, T3])]
    fn single_edit_transforms_previous_into_current(
        #[case] name: &str,
        #[case] previous: &[[u32; 5]],
        #[case] current: &[[u32; 5]],
    ) {
        let previous = previous.concat();
        let current = current.concat();
        let edits = semantic_token_edits(&previous, &current);
        assert_eq!(edits.len(), 1, "case '{name}': expected exactly one edit");
        assert_eq!(
            apply(&previous, &edits),
            current,
            "case '{name}': applying the edit must reproduce the current array"
        );
    }
}
