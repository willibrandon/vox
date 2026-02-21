# API Contract: DictionaryCache

**Module**: `crates/vox_core/src/dictionary.rs`

## DictionaryEntry

A single dictionary entry stored in SQLite and loaded into memory.

```rust
#[derive(Clone, Debug)]
pub struct DictionaryEntry {
    /// Auto-increment primary key.
    pub id: i64,
    /// The term to match (case-insensitive). May be single-word or multi-word phrase.
    pub term: String,
    /// The replacement text.
    pub replacement: String,
    /// Usage frequency count (for ranking hints).
    pub frequency: u32,
    /// When this entry was created (ISO 8601).
    pub created_at: String,
}
```

## DictionaryCache

In-memory cache of user-defined vocabulary substitutions and LLM hints. Loaded from SQLite on startup. Clone is cheap (Arc-wrapped internals).

Supports both single-word terms (O(1) HashMap lookup) and multi-word phrases (longest-first string replacement). See research.md R-004 for the two-pass algorithm.

```rust
#[derive(Clone)]
pub struct DictionaryCache {
    /// Single-word substitutions. Lowercase key → replacement.
    word_substitutions: Arc<HashMap<String, String>>,
    /// Multi-word phrase substitutions. Sorted by descending word count
    /// (longest first) to prevent partial matches.
    phrase_substitutions: Arc<Vec<(String, String)>>,
    /// All entries sorted by frequency descending (for top_hints).
    hints: Arc<Vec<DictionaryEntry>>,
}

impl DictionaryCache {
    /// Load dictionary from SQLite database at the given path.
    /// Creates the table if it doesn't exist.
    /// Entries with whitespace in the term go into phrase_substitutions;
    /// single-word entries go into word_substitutions.
    pub fn load(db_path: &Path) -> Result<Self>;

    /// Create an empty dictionary cache (no substitutions, no hints).
    pub fn empty() -> Self;

    /// Apply substitutions to the given text using a two-pass algorithm:
    /// 1. Phrase pass: replace multi-word phrases (longest-first, case-insensitive)
    /// 2. Word pass: split on whitespace, replace single words (O(1) HashMap lookup)
    /// Returns the original text unchanged if no substitutions match.
    ///
    /// If a substitution produces an empty replacement (e.g., term "um" → replacement ""),
    /// the matched text is removed from the output. If ALL words are removed (entire result
    /// is empty or whitespace-only after substitution), the pipeline treats this the same
    /// as empty ASR output: skips LLM processing and injection, returns to Listening.
    pub fn apply_substitutions(&self, text: &str) -> String;

    /// Format the top N dictionary entries by frequency as a string
    /// for the LLM `dictionary_hints` parameter.
    /// Format: "term1 → replacement1, term2 → replacement2, ..."
    pub fn top_hints(&self, n: usize) -> String;

    /// Reload the cache from SQLite, replacing all in-memory data.
    /// Called when the user adds/edits/removes dictionary entries via the UI.
    /// The running pipeline holds a Clone of DictionaryCache (Arc internals),
    /// so a reload creates new Arc allocations — the pipeline's next call to
    /// apply_substitutions() will use the old snapshot until the pipeline
    /// re-clones the cache. The caller is responsible for providing the
    /// updated DictionaryCache to the pipeline (e.g., via a command channel
    /// extension or by swapping the shared Arc). For the initial implementation,
    /// reload is only called between pipeline sessions (not mid-dictation),
    /// so the pipeline always starts with a fresh cache on each activation.
    pub fn reload(&mut self, db_path: &Path) -> Result<()>;

    /// Number of entries in the cache (words + phrases).
    pub fn len(&self) -> usize;

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool;
}
```
