# Quickstart Verification: Custom Dictionary

**Input**: spec.md acceptance scenarios, contracts/public-api.md
**Date**: 2026-02-21

## Prerequisites

- Feature 009 (Application State & Settings) complete and merged
- SQLite database (vox.db) initialized with dictionary table

## Verification Scenarios

### VS-001: Schema Migration (Fresh Install)

1. Use a temp directory with no existing database
2. Call `DictionaryCache::load(db_path)`
3. **Verify**: Table exists with columns: spoken, written, category, is_command_phrase, use_count, created_at
4. **Verify**: Indexes idx_dictionary_spoken and idx_dictionary_category exist
5. **Verify**: Cache is empty (len() == 0)

### VS-002: Schema Migration (Existing Database)

1. Create a database with old schema (term, replacement, frequency, created_at columns)
2. Insert a test entry: term="hello", replacement="Hello", frequency=5
3. Call `migrate_schema(db_path)`
4. Call `DictionaryCache::load(db_path)`
5. **Verify**: Old entry preserved — spoken="hello", written="Hello", use_count=5
6. **Verify**: New columns have defaults — category="general", is_command_phrase=false
7. **Verify**: Indexes exist

### VS-003: CRUD Operations

1. Load dictionary from a temp database
2. Add entry: spoken="vox", written="Vox", category="name", is_command_phrase=false
3. **Verify**: Returned ID > 0
4. **Verify**: list(None) returns 1 entry with correct fields
5. Update entry: change written to "VOX", category to "technical"
6. **Verify**: list(None) returns entry with updated fields
7. Delete entry by ID
8. **Verify**: list(None) returns empty
9. **Verify**: Reloading from same db_path also shows empty

### VS-004: Duplicate Rejection

1. Add entry: spoken="vox", written="Vox"
2. Try to add entry: spoken="VOX", written="Something"
3. **Verify**: Second add returns an error (case-insensitive uniqueness via COLLATE NOCASE)
4. **Verify**: Only 1 entry exists in the cache

### VS-005: Text Substitution

1. Add entries: "vox"→"Vox", "postgres"→"PostgreSQL", "my email"→"user@example.com"
2. Call apply_substitutions("I use vox and postgres daily")
3. **Verify**: result.text == "I use Vox and PostgreSQL daily"
4. **Verify**: result.matched_ids contains IDs for the "vox" and "postgres" entries
5. **Verify**: result.matched_ids length == 2

### VS-006: Whole-Word Matching

1. Add entry: "vox"→"Vox"
2. Call apply_substitutions("equinox is great")
3. **Verify**: result.text == "equinox is great" (unchanged)
4. **Verify**: result.matched_ids is empty

### VS-007: Command Phrase Exclusion

1. Add entry: spoken="delete last", written="", is_command_phrase=true
2. Add entry: spoken="vox", written="Vox", is_command_phrase=false
3. Call apply_substitutions("please delete last vox")
4. **Verify**: result.text contains "delete last" unchanged
5. **Verify**: result.text contains "Vox" (non-command entry substituted)
6. Call top_hints(50)
7. **Verify**: Output includes "delete last" (command phrases included in hints)
8. **Verify**: Output includes "vox → Vox"

### VS-008: Use Count Tracking

1. Add entry: spoken="vox", written="Vox" (use_count starts at 0)
2. Call apply_substitutions("vox is great vox")
3. **Verify**: result.matched_ids contains the entry ID twice
4. Call increment_use_counts(&result.matched_ids)
5. **Verify**: list(None) shows use_count == 2 for the "vox" entry
6. Reload from same database
7. **Verify**: use_count == 2 persisted in SQLite

### VS-009: Import/Export Round-Trip

1. Add 5 entries with varying categories and is_command_phrase values
2. Call export_json()
3. **Verify**: JSON is valid and contains 5 entries
4. **Verify**: JSON does NOT contain id, use_count, or created_at fields
5. Create a new empty dictionary (different temp db)
6. Call import_json(exported_json)
7. **Verify**: ImportResult.added == 5, skipped == 0, errors is empty
8. **Verify**: All 5 entries present with correct spoken, written, category, is_command_phrase
9. **Verify**: use_count == 0 for all imported entries (not carried over)
10. Call import_json(exported_json) again on the same dictionary
11. **Verify**: ImportResult.added == 0, skipped == 5, errors is empty

### VS-010: Search

1. Add entries: "postgresql"→"PostgreSQL" (category="technical"), "postgres"→"PostgreSQL" (category="technical"), "python"→"Python" (category="technical")
2. Call search("post")
3. **Verify**: Returns "postgresql" and "postgres" entries (partial match on spoken)
4. Call search("Python")
5. **Verify**: Returns "python" entry (match on written form, case-insensitive)
6. Call search("xyz")
7. **Verify**: Returns empty (no match)

### VS-011: Category Filtering

1. Add entries with categories: "vox" (name), "python" (technical), "hello" (general)
2. Call list(Some("technical"))
3. **Verify**: Returns only "python" entry
4. Call list(Some("name"))
5. **Verify**: Returns only "vox" entry
6. Call list(None)
7. **Verify**: Returns all 3 entries

### VS-012: LLM Hint Formatting

1. Add 3 entries, manually set use_counts: "vox" (10), "postgres" (5), "python" (1)
2. Call top_hints(2)
3. **Verify**: Output contains "vox → Vox" before "postgres → PostgreSQL"
4. **Verify**: "python" is NOT included (only top 2 requested)
5. Call top_hints(50) on dictionary with >50 entries
6. **Verify**: Output contains at most 50 entries

### VS-013: Empty Dictionary

1. Create empty dictionary
2. Call apply_substitutions("hello world")
3. **Verify**: result.text == "hello world", result.matched_ids is empty
4. Call top_hints(50)
5. **Verify**: Returns empty string
6. Call search("anything")
7. **Verify**: Returns empty list
8. Call list(None)
9. **Verify**: Returns empty list

### VS-014: Edge Cases

1. Add entries: "New York"→"NY", "New York City"→"NYC"
2. Call apply_substitutions("Visit New York City")
3. **Verify**: result.text == "Visit NYC" (longer phrase matched first)
4. Call apply_substitutions("Visit New York")
5. **Verify**: result.text == "Visit NY"

6. Add entry: "um"→"" (empty replacement)
7. Call apply_substitutions("um let's um meet")
8. **Verify**: result.text == "let's meet" (filler words removed)

9. Call apply_substitutions("um")
10. **Verify**: result.text == "" (all words removed)

### VS-015: Performance

1. Create 1000 entries programmatically (spoken="term_N", written="Term_N")
2. Time `DictionaryCache::load(db_path)`
3. **Verify**: Completes in <50ms
4. Time `apply_substitutions` on 100-word text containing 10 matches
5. **Verify**: Completes in <5ms
6. Time `search("term_5")` across 1000 entries
7. **Verify**: Completes in <10ms
8. Time `add()` operation
9. **Verify**: Completes in <10ms
