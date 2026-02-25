//! Transcript history panel with search, copy, delete, and virtualized scrolling.
//!
//! Provides [`HistoryPanel`] as an entity with `Render` impl. Displays past
//! transcriptions using GPUI's `uniform_list` for performant rendering of
//! large lists. Supports search filtering, per-entry copy/delete with inline
//! confirmation, and "Clear All" with inline confirmation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    div, prelude::*, px, uniform_list, App, ClipboardItem, Entity, EntityId, IntoElement, Render,
    SharedString, UniformListScrollHandle, Window,
};

use vox_core::pipeline::transcript::TranscriptEntry;
use vox_core::state::VoxState;

use crate::layout::{radius, spacing};
use crate::text_input::TextInput;
use crate::theme::{ThemeColors, VoxTheme};

/// History panel displaying past transcription entries.
///
/// Uses `uniform_list` for virtualized rendering so scrolling stays smooth
/// even with thousands of entries. Reloads transcripts from VoxState on
/// every render to pick up external changes and reflect delete operations.
pub struct HistoryPanel {
    /// Loaded transcript entries (refreshed each render).
    transcripts: Vec<TranscriptEntry>,
    /// Current search filter text.
    search_query: String,
    /// Search input field entity.
    search_input: Entity<TextInput>,
    /// Scroll handle for the uniform list.
    scroll_handle: UniformListScrollHandle,
    /// Total transcript count in the database.
    total_count: usize,
    /// Entry ID currently awaiting delete confirmation (shared with click handlers).
    confirming_delete: Rc<RefCell<Option<String>>>,
    /// Whether "Clear All" confirmation is active.
    confirming_clear: Rc<Cell<bool>>,
}

impl HistoryPanel {
    /// Create a new history panel, loading initial transcript page.
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.global::<VoxState>();
        let transcripts = state.get_transcripts(100, 0).unwrap_or_default();
        let total_count = transcripts.len();
        let panel_id = cx.entity_id();

        let search_input = cx.new(|cx| {
            TextInput::new(cx, "Search transcripts...", move |_query, _window, cx| {
                cx.notify(panel_id);
            })
        });

        Self {
            transcripts,
            search_query: String::new(),
            search_input,
            scroll_handle: UniformListScrollHandle::new(),
            total_count,
            confirming_delete: Rc::new(RefCell::new(None)),
            confirming_clear: Rc::new(Cell::new(false)),
        }
    }

    /// Reload transcripts from VoxState based on current search query.
    fn refresh_transcripts(&mut self, cx: &mut App) {
        let state = cx.global::<VoxState>();
        self.search_query = self.search_input.read(cx).content().to_string();

        if self.search_query.is_empty() {
            self.transcripts = state.get_transcripts(500, 0).unwrap_or_default();
        } else {
            self.transcripts = state
                .search_transcripts(&self.search_query)
                .unwrap_or_default();
        }
        self.total_count = self.transcripts.len();
    }

    /// Process any pending delete or clear actions from click handlers.
    fn process_pending_actions(&mut self, _cx: &mut App) {
        // Handle "Clear All" confirmation
        if self.confirming_clear.get() {
            // Don't auto-execute — wait for explicit confirm click
        }

        // Handle single-entry delete confirmation
        // (actual deletion happens in the confirm click handler)
    }

    /// Render a single transcript entry row with copy/delete buttons.
    fn render_entry(
        entry: &TranscriptEntry,
        colors: &ThemeColors,
        show_raw: bool,
        panel_id: EntityId,
        confirming_id: &Rc<RefCell<Option<String>>>,
    ) -> impl IntoElement {
        let timestamp = if entry.created_at.len() >= 16 {
            // Format "YYYY-MM-DDTHH:MM" as "YYYY-MM-DD HH:MM"
            SharedString::from(entry.created_at[..16].replace('T', " "))
        } else {
            SharedString::from(entry.created_at.clone())
        };

        let polished = SharedString::from(entry.polished_text.clone());
        let latency = SharedString::from(format!("{}ms", entry.latency_ms));
        let target = SharedString::from(entry.target_app.clone());

        // Check if this entry is in delete confirmation state
        let is_confirming = confirming_id
            .borrow()
            .as_ref()
            .map_or(false, |id| id == &entry.id);

        // Data for click handlers
        let text_for_copy = entry.polished_text.clone();
        let id_for_delete = entry.id.clone();
        let confirming_for_click = confirming_id.clone();
        let confirming_for_confirm = confirming_id.clone();
        let confirming_for_cancel = confirming_id.clone();
        let id_for_confirm = entry.id.clone();

        let mut row = div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .gap(spacing::XS)
            .px(spacing::MD)
            .py(spacing::SM)
            .mb(spacing::SM)
            .rounded(radius::SM)
            .bg(colors.elevated_surface)
            .border_1()
            .border_color(colors.border);

        // Header row: timestamp | target + latency
        row = row.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.text_muted)
                        .child(timestamp),
                )
                .child(
                    div()
                        .flex()
                        .gap(spacing::SM)
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(colors.text_muted)
                                .px(spacing::XS)
                                .rounded(radius::SM)
                                .bg(colors.surface)
                                .child(target),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(colors.text_muted)
                                .child(latency),
                        ),
                ),
        );

        // Polished text (single line with ellipsis — uniform_list requires fixed-height items)
        row = row.child(
            div()
                .text_size(px(13.0))
                .text_color(colors.text)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(polished),
        );

        // Raw text (optional, also single line)
        if show_raw && !entry.raw_text.is_empty() {
            let raw = SharedString::from(entry.raw_text.clone());
            row = row.child(
                div()
                    .text_size(px(11.0))
                    .text_color(colors.text_muted)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(raw),
            );
        }

        // Action buttons row
        let copy_button_text_color = colors.text_muted;
        let delete_button_color = colors.status_error;
        let accent = colors.accent;

        let action_row = if is_confirming {
            // Delete confirmation buttons
            div()
                .flex()
                .items_center()
                .gap(spacing::SM)
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(delete_button_color)
                        .child(SharedString::from("Delete?")),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("confirm-del-{}", id_for_confirm)))
                        .text_size(px(11.0))
                        .text_color(accent)
                        .cursor_pointer()
                        .child(SharedString::from("Yes"))
                        .on_click(move |_, _window, cx| {
                            if let Err(err) =
                                cx.global::<VoxState>().delete_transcript(&id_for_confirm)
                            {
                                tracing::warn!(%err, "failed to delete transcript");
                            }
                            *confirming_for_confirm.borrow_mut() = None;
                            cx.notify(panel_id);
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "cancel-del-{}",
                            id_for_delete.clone()
                        )))
                        .text_size(px(11.0))
                        .text_color(copy_button_text_color)
                        .cursor_pointer()
                        .child(SharedString::from("No"))
                        .on_click(move |_, _window, cx| {
                            *confirming_for_cancel.borrow_mut() = None;
                            cx.notify(panel_id);
                        }),
                )
        } else {
            // Normal action buttons: Copy | Delete
            div()
                .flex()
                .items_center()
                .gap(spacing::SM)
                .child(
                    div()
                        .id(SharedString::from(format!("copy-{}", id_for_delete)))
                        .text_size(px(11.0))
                        .text_color(copy_button_text_color)
                        .cursor_pointer()
                        .child(SharedString::from("Copy"))
                        .on_click(move |_, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                text_for_copy.clone(),
                            ));
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("del-{}", entry.id)))
                        .text_size(px(11.0))
                        .text_color(delete_button_color)
                        .cursor_pointer()
                        .child(SharedString::from("Delete"))
                        .on_click(move |_, _window, cx| {
                            *confirming_for_click.borrow_mut() = Some(id_for_delete.clone());
                            cx.notify(panel_id);
                        }),
                )
        };

        row = row.child(action_row);
        row
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel_id = cx.entity_id();

        // Refresh data from VoxState (mutable ops before theme borrow)
        self.refresh_transcripts(cx);
        self.process_pending_actions(cx);

        let theme = cx.global::<VoxTheme>();

        let show_raw = cx.global::<VoxState>().settings().show_raw_transcript;
        let confirming_clear = self.confirming_clear.clone();
        let confirming_clear_yes = self.confirming_clear.clone();
        let confirming_clear_no = self.confirming_clear.clone();
        let is_clearing = self.confirming_clear.get();

        // Header with search and Clear All
        let mut header = div()
            .p(spacing::LG)
            .pb(spacing::SM)
            .flex()
            .flex_col()
            .gap(spacing::SM);

        // Title row with Clear All button
        let title_row = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(16.0))
                    .text_color(theme.colors.text)
                    .child(SharedString::from("Transcript History")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.colors.text_muted)
                            .child(SharedString::from(format!(
                                "{} entries",
                                self.total_count
                            ))),
                    )
                    .when(!is_clearing && !self.transcripts.is_empty(), |d| {
                        d.child(
                            div()
                                .id("clear-all-history")
                                .text_size(px(11.0))
                                .text_color(theme.colors.status_error)
                                .cursor_pointer()
                                .px(spacing::SM)
                                .py(px(2.0))
                                .rounded(radius::SM)
                                .border_1()
                                .border_color(theme.colors.status_error)
                                .child(SharedString::from("Clear All"))
                                .on_click(move |_, _window, cx| {
                                    confirming_clear.set(true);
                                    cx.notify(panel_id);
                                }),
                        )
                    })
                    .when(is_clearing, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(spacing::XS)
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme.colors.status_error)
                                        .child(SharedString::from("Delete all?")),
                                )
                                .child(
                                    div()
                                        .id("confirm-clear-all")
                                        .text_size(px(11.0))
                                        .text_color(theme.colors.accent)
                                        .cursor_pointer()
                                        .child(SharedString::from("Yes"))
                                        .on_click(move |_, _window, cx| {
                                            if let Err(err) =
                                                cx.global::<VoxState>().clear_history()
                                            {
                                                tracing::warn!(
                                                    %err,
                                                    "failed to clear history"
                                                );
                                            }
                                            confirming_clear_yes.set(false);
                                            cx.notify(panel_id);
                                        }),
                                )
                                .child(
                                    div()
                                        .id("cancel-clear-all")
                                        .text_size(px(11.0))
                                        .text_color(theme.colors.text_muted)
                                        .cursor_pointer()
                                        .child(SharedString::from("No"))
                                        .on_click(move |_, _window, cx| {
                                            confirming_clear_no.set(false);
                                            cx.notify(panel_id);
                                        }),
                                ),
                        )
                    }),
            );

        header = header.child(title_row);

        // Search input
        header = header.child(self.search_input.clone());

        if self.transcripts.is_empty() {
            let empty_message = if self.search_query.is_empty() {
                "No transcript history"
            } else {
                "No transcripts match your search"
            };

            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(header)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .text_color(theme.colors.text_muted)
                                .child(SharedString::from(empty_message)),
                        ),
                )
                .into_any_element();
        }

        let entry_count = self.transcripts.len();
        let transcripts = self.transcripts.clone();
        let colors = theme.colors.clone();
        let confirming_delete = self.confirming_delete.clone();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                uniform_list(
                    "history-list",
                    entry_count,
                    move |range, _window, _cx| {
                        range
                            .map(|ix| {
                                Self::render_entry(
                                    &transcripts[ix],
                                    &colors,
                                    show_raw,
                                    panel_id,
                                    &confirming_delete,
                                )
                                .into_any_element()
                            })
                            .collect()
                    },
                )
                .flex_1()
                .px(spacing::LG)
                .track_scroll(&self.scroll_handle),
            )
            .into_any_element()
    }
}
