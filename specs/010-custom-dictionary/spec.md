# Feature Specification: Custom Dictionary

**Feature Branch**: `010-custom-dictionary`
**Created**: 2026-02-21
**Status**: Draft
**Input**: User description: "Custom dictionary system for spoken-to-written word mappings"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Dictionary Entry Management (Priority: P1)

A user wants to define custom word mappings so that the dictation engine uses their preferred spellings, abbreviations, and expansions. For example, a user says "vox" and wants it transcribed as "Vox", or says "my email" and wants it replaced with "engineer@example.com". The user can add, edit, delete, and view all their dictionary entries.

**Why this priority**: Without the ability to create and persist dictionary entries, no other dictionary feature can function. This is the foundation that all other stories build on.

**Independent Test**: Create a fresh dictionary, add 3 entries (a name, a technical term, an email expansion), verify they persist across application restarts. Edit one entry, delete another, verify changes are reflected.

**Acceptance Scenarios**:

1. **Given** an empty dictionary, **When** the user adds an entry with spoken="vox" and written="Vox", **Then** the entry is persisted and appears in the dictionary list
2. **Given** a dictionary with entries, **When** the user edits an entry's written form, **Then** the updated value is persisted and used in future substitutions
3. **Given** a dictionary with entries, **When** the user deletes an entry, **Then** it is removed from both the in-memory cache and persistent storage
4. **Given** a dictionary with entries, **When** the user lists entries filtered by category, **Then** only entries in that category are returned
5. **Given** a dictionary with entries, **When** the user searches by partial text, **Then** entries matching spoken or written form are returned
6. **Given** a dictionary entry with a duplicate spoken form already exists, **When** the user tries to add another entry with the same spoken form, **Then** the system rejects it with a clear error

---

### User Story 2 - Text Substitution During Dictation (Priority: P1)

During dictation, the pipeline automatically applies dictionary substitutions to the raw transcript text before LLM processing. Substitutions are case-insensitive and match whole words only. Entries marked as command phrases are excluded from text substitution.

**Why this priority**: Text substitution is the primary value proposition of the dictionary — it directly improves transcription accuracy for user-specific vocabulary. Without it, the dictionary is just a data store.

**Independent Test**: Add entries "vox" → "Vox" and "postgres" → "PostgreSQL". Dictate "I use vox and postgres daily". Verify the substituted text reads "I use Vox and PostgreSQL daily". Then verify "equinox" does NOT get partially matched.

**Acceptance Scenarios**:

1. **Given** a dictionary entry spoken="vox" written="Vox", **When** the user dictates "I love vox", **Then** the output contains "I love Vox"
2. **Given** the same entry, **When** the user dictates "I love VOX", **Then** the output contains "I love Vox" (case-insensitive)
3. **Given** the same entry, **When** the user dictates "equinox is great", **Then** "equinox" is NOT modified (whole-word matching only)
4. **Given** a multi-word entry spoken="my email" written="engineer@example.com", **When** the user dictates "send to my email please", **Then** the output contains "send to engineer@example.com please"
5. **Given** an entry with is_command_phrase=true, **When** the user dictates text containing that phrase, **Then** no text substitution occurs for that entry

---

### User Story 3 - LLM Hint Integration (Priority: P2)

The most frequently used dictionary entries are injected into the LLM post-processor's system prompt as hints. This biases the LLM toward user-preferred spellings and terminology even when the dictionary substitution engine doesn't catch a variation.

**Why this priority**: LLM hints complement direct substitution by handling cases where the spoken form varies slightly or context matters. This multiplies the dictionary's effectiveness but requires the core dictionary (US1) and substitution (US2) to work first.

**Independent Test**: Add 5 entries with varying use counts. Request top 3 hints. Verify the output is formatted correctly and ordered by use count descending. Verify command phrases ARE included in hints (unlike substitution).

**Acceptance Scenarios**:

1. **Given** dictionary entries with different use counts, **When** the system requests top N hints, **Then** entries are returned sorted by use count (most used first)
2. **Given** 100 dictionary entries, **When** the system requests hints, **Then** at most 50 entries are included to stay within context window budget
3. **Given** entries including command phrases, **When** the system requests hints, **Then** command phrases ARE included (unlike text substitution where they are excluded)
4. **Given** dictionary entries, **When** hints are formatted for the LLM, **Then** each entry appears as "spoken" → "written" in a list format

---

### User Story 4 - Use Count Tracking (Priority: P2)

Each time a dictionary entry is applied during text substitution, its use count increments. Use count determines the priority order for LLM hints — frequently used entries appear first, maximizing the LLM's limited context window.

**Why this priority**: Use count tracking makes the hint system adaptive. Without it, hints are static and may waste context window space on rarely-used entries.

**Independent Test**: Add an entry with use_count=0. Apply substitution that matches it 3 times. Verify use_count=3. Verify this entry now ranks higher in hints than entries with use_count < 3.

**Acceptance Scenarios**:

1. **Given** an entry with use_count=0, **When** the entry is matched during text substitution, **Then** use_count increments to 1
2. **Given** multiple substitution matches in one text, **When** the same entry matches 3 times, **Then** use_count increments by 3
3. **Given** entries with varying use counts, **When** hints are requested, **Then** the ordering reflects current use counts

---

### User Story 5 - Import and Export (Priority: P3)

Users can export their entire dictionary as a portable file for backup purposes, and import entries from such a file to restore or merge dictionaries. Import handles duplicates gracefully by skipping entries whose spoken form already exists.

**Why this priority**: Import/export provides data portability and backup. Important for user confidence but not required for core dictation functionality.

**Independent Test**: Add 5 entries, export to file. Clear dictionary. Import from file. Verify all 5 entries restored with original data. Import again — verify 5 skipped as duplicates with 0 errors.

**Acceptance Scenarios**:

1. **Given** a dictionary with entries, **When** the user exports, **Then** a portable file is produced containing all entries with their metadata
2. **Given** an export file, **When** the user imports into an empty dictionary, **Then** all entries are created with original data
3. **Given** an export file with entries that already exist, **When** the user imports, **Then** duplicates are skipped and the import result reports how many were added vs skipped
4. **Given** an export file with malformed entries, **When** the user imports, **Then** valid entries are imported and errors are reported for invalid ones without aborting the entire import

---

### User Story 6 - Command Phrase Handling (Priority: P3)

Entries marked as "command phrase" behave differently from regular dictionary entries. They are excluded from text substitution (so they don't replace spoken text with written text) but are still included as LLM hints (so the LLM knows about them). This supports phrases like "delete last" or "new paragraph" that should trigger voice commands rather than text injection.

**Why this priority**: Command phrase exclusion enables the voice command system to coexist with dictionary substitution without interference. Important for the full pipeline but not needed for basic dictation.

**Independent Test**: Add entry spoken="delete last" written="" is_command_phrase=true. Dictate "please delete last sentence". Verify "delete last" is NOT substituted in the text. Verify "delete last" IS included in LLM hints.

**Acceptance Scenarios**:

1. **Given** an entry with is_command_phrase=true, **When** text substitution runs, **Then** that entry is skipped
2. **Given** an entry with is_command_phrase=true, **When** LLM hints are generated, **Then** that entry IS included
3. **Given** entries with mixed is_command_phrase values, **When** text substitution runs, **Then** only non-command entries are applied

---

### Edge Cases

- What happens when a dictionary entry's spoken form is a substring of another entry's spoken form? (e.g., "New York" and "New York City") — Longer phrases must take priority.
- What happens when substitution produces an empty string? (e.g., removing filler words like "um") — The word should be removed from the output.
- What happens when the dictionary has 0 entries? — Substitution returns the original text unchanged, hints return empty string.
- What happens when all words in input are removed by substitution? — Return an empty string.
- What happens when import file contains entries with empty spoken forms? — Reject those entries as errors.
- What happens when Unicode characters are involved in matching? (e.g., accented characters) — Case-insensitive matching must handle Unicode correctly.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST persist dictionary entries with spoken form, written form, category, command phrase flag, use count, and creation timestamp
- **FR-002**: System MUST enforce uniqueness on the spoken form (case-insensitive) — no two entries may have the same spoken text
- **FR-003**: System MUST support adding, updating, deleting, listing, and searching dictionary entries
- **FR-004**: System MUST keep an in-memory cache of dictionary entries synchronized with persistent storage for fast lookups during dictation
- **FR-005**: System MUST apply text substitutions case-insensitively using whole-word matching only
- **FR-006**: System MUST support multi-word phrase matching with longest-phrase-first priority to prevent partial matches
- **FR-007**: System MUST exclude entries marked as command phrases from text substitution
- **FR-008**: System MUST include command phrase entries in LLM hints alongside regular entries
- **FR-009**: System MUST format LLM hints as a list of spoken → written mappings, limited to the top 50 entries by use count
- **FR-010**: System MUST increment use count each time an entry is matched during text substitution
- **FR-011**: System MUST support exporting all dictionary entries to a portable format
- **FR-012**: System MUST support importing entries from a portable format, skipping duplicates and reporting results
- **FR-013**: System MUST support filtering entries by category
- **FR-014**: System MUST support searching entries by partial match on spoken or written text
- **FR-015**: System MUST support index-based lookups on spoken form and category for query performance

### Key Entities

- **Dictionary Entry**: A user-defined mapping from a spoken form to a written form. Attributes: unique identifier, spoken text (unique, case-insensitive), written text, category (general/name/technical/email/etc.), command phrase flag, use count, creation timestamp.
- **Import Result**: The outcome of a batch import operation. Attributes: count of entries added, count of entries skipped (duplicates), list of error descriptions for malformed entries.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Dictionary loads 1,000 entries into the in-memory cache in under 50 milliseconds
- **SC-002**: A single text substitution completes in under 1 millisecond
- **SC-003**: Full text substitution on a 100-word input completes in under 5 milliseconds
- **SC-004**: Add, update, or delete operations complete in under 10 milliseconds each
- **SC-005**: Searching 1,000 entries completes in under 10 milliseconds
- **SC-006**: Export and import of 1,000 entries produces identical data when round-tripped
- **SC-007**: Zero compiler warnings across all dictionary code
- **SC-008**: All 13 specified unit tests pass

## Assumptions

- The dictionary table already exists in the application database (created by Feature 009's init_database). This feature extends the schema with additional columns (category, is_command_phrase) and indexes.
- Multi-word phrase substitution uses a longest-first strategy: phrases with more words are matched before shorter ones.
- Category values are freeform strings — no predefined list is enforced. Common categories include "general", "name", "technical", "email".
- The existing dictionary cache (from the pipeline orchestration feature) will be refactored to align with the new schema and additional functionality.
- The LLM hint limit of 50 entries is a sensible default for the context window budget. This is not user-configurable.

## Dependencies

- **Feature 009 (Application State & Settings)**: Provides the SQLite database connection, VoxState global state, and data directory management.
- **Feature 007 (Pipeline Orchestration)**: The pipeline orchestrator currently uses `DictionaryCache` for substitution and hints. This feature refactors that existing code and extends it.

## Testing Requirements

### Unit Tests

| Test | Description |
|------|-------------|
| `test_add_entry` | Add entry, verify in cache and persistent storage |
| `test_update_entry` | Update entry, verify changes propagated to cache and storage |
| `test_delete_entry` | Delete entry, verify removed from cache and storage |
| `test_substitution_basic` | "vox" → "Vox" in text |
| `test_substitution_case_insensitive` | "VOX" also matches "vox" entry |
| `test_substitution_whole_word` | "equinox" does not match "vox" entry |
| `test_substitution_command_excluded` | Command phrases not substituted |
| `test_top_hints_format` | Correct formatting for LLM prompt |
| `test_top_hints_sorted_by_use` | Most-used entries first |
| `test_use_count_increment` | Count increments on substitution |
| `test_import_export_roundtrip` | Export then import preserves all data |
| `test_search_spoken` | Search finds by spoken form |
| `test_search_written` | Search finds by written form |
