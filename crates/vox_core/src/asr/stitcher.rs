//! Overlap stitching for force-segmented speech transcriptions.
//!
//! When the VAD force-segments long continuous speech at 30 seconds with a
//! 1-second overlap, consecutive transcriptions share words at the boundary.
//! [`stitch_segments`] deduplicates the overlap region using word-level
//! tail/head overlap matching.

/// Combine two transcription results, deduplicating the overlap region.
///
/// Splits both texts into word tokens, finds the longest exact overlap between
/// the tail of `previous` and the head of `next`, and removes duplicate words
/// from `next` before concatenating.
///
/// If one side has no word tokens, returns the other side. If both sides have
/// no word tokens, returns an empty string.
pub fn stitch_segments(previous: &str, next: &str) -> String {
    let previous_words: Vec<&str> = previous.split_whitespace().collect();
    let next_words: Vec<&str> = next.split_whitespace().collect();

    if previous_words.is_empty() && next_words.is_empty() {
        return String::new();
    }
    if previous_words.is_empty() {
        return next_words.join(" ");
    }
    if next_words.is_empty() {
        return previous_words.join(" ");
    }

    let max_overlap_length = previous_words.len().min(next_words.len());
    let overlap_length = (1..=max_overlap_length)
        .rev()
        .find(|&candidate_length| {
            previous_words[previous_words.len() - candidate_length..] == next_words[..candidate_length]
        })
        .unwrap_or(0);

    let mut stitched_words: Vec<&str> =
        Vec::with_capacity(previous_words.len() + next_words.len() - overlap_length);
    stitched_words.extend(previous_words.iter().copied());
    stitched_words.extend(next_words[overlap_length..].iter().copied());
    stitched_words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::stitch_segments;

    #[test]
    fn test_stitch_segments_both_empty() {
        assert_eq!(stitch_segments("", ""), "");
    }

    #[test]
    fn test_stitch_segments_previous_empty() {
        assert_eq!(stitch_segments("", "hello world"), "hello world");
    }

    #[test]
    fn test_stitch_segments_next_empty() {
        assert_eq!(stitch_segments("hello world", ""), "hello world");
    }

    #[test]
    fn test_stitch_segments_no_overlap() {
        assert_eq!(
            stitch_segments("the quick brown", "fox jumps over"),
            "the quick brown fox jumps over"
        );
    }

    #[test]
    fn test_stitch_segments_deduplicates_overlap() {
        assert_eq!(
            stitch_segments("the quick brown fox", "brown fox jumps over"),
            "the quick brown fox jumps over"
        );
    }

    #[test]
    fn test_stitch_segments_identical_text() {
        assert_eq!(
            stitch_segments("we hold these truths", "we hold these truths"),
            "we hold these truths"
        );
    }
}
