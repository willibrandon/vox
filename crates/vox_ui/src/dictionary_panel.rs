//! Dictionary management panel for custom word mappings.
//!
//! Provides [`DictionaryPanel`] as an entity with `Render` impl. Displays
//! dictionary entries with search filtering, inline add/edit/delete, sort
//! by column, command phrase toggle, and JSON import/export.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    div, prelude::*, px, AnyElement, App, Entity, EntityId, IntoElement, PathPromptOptions, Render,
    ScrollHandle, SharedString, Window,
};

use vox_core::dictionary::DictionaryEntry;
use vox_core::state::VoxState;

use crate::layout::{radius, spacing};
use crate::scrollbar::{new_drag_state, Scrollbar, ScrollbarDragState};
use crate::text_input::TextInput;
use crate::theme::{ThemeColors, VoxTheme};

/// Sort field for dictionary entry ordering.
#[derive(Clone, Copy, PartialEq)]
enum SortField {
    /// Sort by the spoken form (alphabetical).
    Spoken,
    /// Sort by category.
    Category,
    /// Sort by use count (most used first when descending).
    UseCount,
}

/// Dictionary panel for viewing and managing custom word substitutions.
///
/// Uses the scrollbar-as-sibling pattern for proper scroll behavior.
/// Provides inline add form, inline edit, delete with confirmation,
/// search, sort, and command phrase toggle.
pub struct DictionaryPanel {
    /// Loaded dictionary entries (refreshed each render).
    entries: Vec<DictionaryEntry>,
    /// Current search filter text.
    search_query: String,
    /// Search input field.
    search_input: Entity<TextInput>,
    /// Input field for new entry's spoken form.
    new_spoken_input: Entity<TextInput>,
    /// Input field for new entry's written form.
    new_written_input: Entity<TextInput>,
    /// Input field for new entry's category.
    new_category_input: Entity<TextInput>,
    /// Entry ID currently being edited inline (None = no edit in progress).
    editing_id: Rc<Cell<Option<i64>>>,
    /// Reusable input field for editing an entry's spoken form.
    edit_spoken_input: Entity<TextInput>,
    /// Reusable input field for editing an entry's written form.
    edit_written_input: Entity<TextInput>,
    /// Reusable input field for editing an entry's category.
    edit_category_input: Entity<TextInput>,
    /// Current sort field.
    sort_field: SortField,
    /// Sort direction (true = ascending).
    sort_ascending: bool,
    /// Entry ID awaiting delete confirmation.
    confirming_delete: Rc<Cell<Option<i64>>>,
    /// Error message to display (e.g., duplicate spoken form).
    error_message: Rc<Cell<Option<String>>>,
    /// Scroll handle shared between scroll container and Scrollbar.
    scroll_handle: ScrollHandle,
    /// Drag state for scrollbar thumb.
    scrollbar_drag: ScrollbarDragState,
}

impl DictionaryPanel {
    /// Create a new dictionary panel, loading all entries.
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panel_id = cx.entity_id();

        let search_input = cx.new(|cx| {
            TextInput::new(cx, "Search entries...", move |_query, _window, cx| {
                cx.notify(panel_id);
            })
        });

        let new_spoken_input = cx.new(|cx| {
            TextInput::new(cx, "Spoken form", move |_, _window, _cx| {})
        });

        let new_written_input = cx.new(|cx| {
            TextInput::new(cx, "Written form", move |_, _window, _cx| {})
        });

        let new_category_input = cx.new(|cx| {
            TextInput::new(cx, "Category", move |_, _window, _cx| {})
        });

        let edit_spoken_input = cx.new(|cx| {
            TextInput::new(cx, "Spoken form", move |_, _window, _cx| {})
        });

        let edit_written_input = cx.new(|cx| {
            TextInput::new(cx, "Written form", move |_, _window, _cx| {})
        });

        let edit_category_input = cx.new(|cx| {
            TextInput::new(cx, "Category", move |_, _window, _cx| {})
        });

        let state = cx.global::<VoxState>();
        let entries = state.dictionary().list(None);

        Self {
            entries,
            search_query: String::new(),
            search_input,
            new_spoken_input,
            new_written_input,
            new_category_input,
            editing_id: Rc::new(Cell::new(None)),
            edit_spoken_input,
            edit_written_input,
            edit_category_input,
            sort_field: SortField::Spoken,
            sort_ascending: true,
            confirming_delete: Rc::new(Cell::new(None)),
            error_message: Rc::new(Cell::new(None)),
            scroll_handle: ScrollHandle::new(),
            scrollbar_drag: new_drag_state(),
        }
    }

    /// Reload entries from VoxState based on current search and sort.
    fn refresh_entries(&mut self, cx: &mut App) {
        let state = cx.global::<VoxState>();
        self.search_query = self.search_input.read(cx).content().to_string();

        if self.search_query.is_empty() {
            self.entries = state.dictionary().list(None);
        } else {
            self.entries = state.dictionary().search(&self.search_query);
        }

        // Apply local sort
        let ascending = self.sort_ascending;
        match self.sort_field {
            SortField::Spoken => {
                self.entries.sort_by(|a, b| {
                    let ord = a.spoken.to_lowercase().cmp(&b.spoken.to_lowercase());
                    if ascending { ord } else { ord.reverse() }
                });
            }
            SortField::Category => {
                self.entries.sort_by(|a, b| {
                    let ord = a.category.to_lowercase().cmp(&b.category.to_lowercase());
                    if ascending { ord } else { ord.reverse() }
                });
            }
            SortField::UseCount => {
                self.entries.sort_by(|a, b| {
                    let ord = a.use_count.cmp(&b.use_count);
                    if ascending { ord } else { ord.reverse() }
                });
            }
        }
    }

    /// Render a single dictionary entry row with CMD toggle, Edit, and Delete.
    fn render_entry(
        entry: &DictionaryEntry,
        colors: &ThemeColors,
        panel_id: EntityId,
        confirming_delete: &Rc<Cell<Option<i64>>>,
        editing_id: &Rc<Cell<Option<i64>>>,
        edit_spoken: &Entity<TextInput>,
        edit_written: &Entity<TextInput>,
        edit_category: &Entity<TextInput>,
    ) -> impl IntoElement {
        let is_confirming = confirming_delete
            .get()
            .map_or(false, |id| id == entry.id);

        let confirming_for_click = confirming_delete.clone();
        let confirming_for_yes = confirming_delete.clone();
        let confirming_for_no = confirming_delete.clone();
        let editing_for_click = editing_id.clone();
        let edit_spoken_for_click = edit_spoken.clone();
        let edit_written_for_click = edit_written.clone();
        let edit_category_for_click = edit_category.clone();
        let spoken_for_edit = entry.spoken.clone();
        let written_for_edit = entry.written.clone();
        let category_for_edit = entry.category.clone();
        let entry_id = entry.id;
        let entry_id_for_toggle = entry.id;
        let spoken_for_toggle = entry.spoken.clone();
        let written_for_toggle = entry.written.clone();
        let category_for_toggle = entry.category.clone();
        let is_cmd = entry.is_command_phrase;

        let mut row = div()
            .flex()
            .items_center()
            .justify_between()
            .px(spacing::MD)
            .py(spacing::SM)
            .rounded(radius::SM)
            .bg(colors.elevated_surface)
            .border_1()
            .border_color(colors.border);

        // Left side: spoken → written with category badge
        row = row.child(
            div()
                .flex()
                .items_center()
                .gap(spacing::MD)
                .flex_1()
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(colors.text)
                        .min_w(px(100.0))
                        .child(SharedString::from(entry.spoken.clone())),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(colors.text_muted)
                        .child(SharedString::from("→")),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(colors.text)
                        .flex_1()
                        .child(SharedString::from(entry.written.clone())),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(colors.text_muted)
                        .px(spacing::XS)
                        .py(px(2.0))
                        .rounded(radius::SM)
                        .bg(colors.surface)
                        .child(SharedString::from(entry.category.clone())),
                )
                .when(entry.is_command_phrase, |d| {
                    d.child(
                        div()
                            .text_size(px(10.0))
                            .text_color(colors.accent)
                            .child(SharedString::from("CMD")),
                    )
                }),
        );

        // Right side: command toggle + delete button
        let accent = colors.accent;
        let text_muted = colors.text_muted;
        let border = colors.border;
        let error_color = colors.status_error;

        let action_buttons = if is_confirming {
            div()
                .flex()
                .items_center()
                .gap(spacing::XS)
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(error_color)
                        .child(SharedString::from("Delete?")),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("dict-confirm-{entry_id}")))
                        .text_size(px(11.0))
                        .text_color(accent)
                        .cursor_pointer()
                        .child(SharedString::from("Yes"))
                        .on_click(move |_, _window, cx| {
                            if let Err(err) =
                                cx.global::<VoxState>().dictionary().delete(entry_id)
                            {
                                tracing::warn!(%err, "failed to delete dictionary entry");
                            }
                            confirming_for_yes.set(None);
                            cx.notify(panel_id);
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("dict-cancel-{entry_id}")))
                        .text_size(px(11.0))
                        .text_color(text_muted)
                        .cursor_pointer()
                        .child(SharedString::from("No"))
                        .on_click(move |_, _window, cx| {
                            confirming_for_no.set(None);
                            cx.notify(panel_id);
                        }),
                )
        } else {
            div()
                .flex()
                .items_center()
                .gap(spacing::SM)
                .child(
                    div()
                        .id(SharedString::from(format!("dict-cmd-{entry_id}")))
                        .text_size(px(10.0))
                        .text_color(if is_cmd { accent } else { text_muted })
                        .cursor_pointer()
                        .px(spacing::XS)
                        .py(px(2.0))
                        .rounded(radius::SM)
                        .border_1()
                        .border_color(if is_cmd { accent } else { border })
                        .child(SharedString::from("CMD"))
                        .on_click(move |_, _window, cx| {
                            if let Err(err) = cx.global::<VoxState>().dictionary().update(
                                entry_id_for_toggle,
                                &spoken_for_toggle,
                                &written_for_toggle,
                                &category_for_toggle,
                                !is_cmd,
                            ) {
                                tracing::warn!(%err, "failed to toggle command phrase");
                            }
                            cx.notify(panel_id);
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("dict-edit-{entry_id}")))
                        .text_size(px(11.0))
                        .text_color(accent)
                        .cursor_pointer()
                        .child(SharedString::from("Edit"))
                        .on_click(move |_, _window, cx| {
                            // Populate edit inputs with current entry values
                            edit_spoken_for_click.update(cx, |input, _cx| {
                                input.set_content(spoken_for_edit.clone());
                            });
                            edit_written_for_click.update(cx, |input, _cx| {
                                input.set_content(written_for_edit.clone());
                            });
                            edit_category_for_click.update(cx, |input, _cx| {
                                input.set_content(category_for_edit.clone());
                            });
                            editing_for_click.set(Some(entry_id));
                            cx.notify(panel_id);
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("dict-del-{entry_id}")))
                        .text_size(px(11.0))
                        .text_color(error_color)
                        .cursor_pointer()
                        .child(SharedString::from("Delete"))
                        .on_click(move |_, _window, cx| {
                            confirming_for_click.set(Some(entry_id));
                            cx.notify(panel_id);
                        }),
                )
        };

        row = row.child(action_buttons);
        row
    }

    /// Render a dictionary entry row in edit mode with text inputs and
    /// Confirm/Cancel buttons. Returns `AnyElement` to avoid Rust 2024
    /// lifetime capture.
    fn render_edit_row(&self, entry: &DictionaryEntry, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<VoxTheme>();
        let button_bg = theme.colors.button_primary_bg;
        let button_text = theme.colors.button_primary_text;
        let text_muted = theme.colors.text_muted;
        let elevated = theme.colors.elevated_surface;
        let border_color = theme.colors.border;
        let accent = theme.colors.accent;

        let panel_id = cx.entity_id();
        let editing_id = self.editing_id.clone();
        let editing_id_cancel = self.editing_id.clone();
        let entry_id = entry.id;
        let is_cmd = entry.is_command_phrase;
        let edit_spoken = self.edit_spoken_input.clone();
        let edit_written = self.edit_written_input.clone();
        let edit_category = self.edit_category_input.clone();

        div()
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .px(spacing::MD)
            .py(spacing::SM)
            .rounded(radius::SM)
            .bg(elevated)
            .border_1()
            .border_color(accent)
            // Input fields row
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .child(self.edit_spoken_input.clone())
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(text_muted)
                            .child(SharedString::from("→")),
                    )
                    .child(self.edit_written_input.clone())
                    .child(self.edit_category_input.clone()),
            )
            // Action buttons row
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .id(SharedString::from(format!("dict-save-{entry_id}")))
                            .px(spacing::MD)
                            .py(px(3.0))
                            .rounded(radius::SM)
                            .bg(button_bg)
                            .text_color(button_text)
                            .text_size(px(11.0))
                            .cursor_pointer()
                            .child(SharedString::from("Confirm"))
                            .on_click(move |_, _window, cx| {
                                let spoken = edit_spoken.read(cx).content().to_string();
                                let written = edit_written.read(cx).content().to_string();
                                let category = edit_category.read(cx).content().to_string();

                                let category = if category.is_empty() {
                                    "general".to_string()
                                } else {
                                    category
                                };

                                if let Err(err) = cx.global::<VoxState>().dictionary().update(
                                    entry_id, &spoken, &written, &category, is_cmd,
                                ) {
                                    tracing::warn!(%err, "failed to update dictionary entry");
                                }
                                editing_id.set(None);
                                cx.notify(panel_id);
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("dict-cancel-edit-{entry_id}")))
                            .px(spacing::MD)
                            .py(px(3.0))
                            .rounded(radius::SM)
                            .border_1()
                            .border_color(border_color)
                            .text_color(text_muted)
                            .text_size(px(11.0))
                            .cursor_pointer()
                            .child(SharedString::from("Cancel"))
                            .on_click(move |_, _window, cx| {
                                editing_id_cancel.set(None);
                                cx.notify(panel_id);
                            }),
                    ),
            )
            .into_any_element()
    }

    /// Render Export and Import buttons for JSON dictionary round-trip.
    /// Returns `AnyElement` to avoid Rust 2024 lifetime capture.
    fn render_import_export(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<VoxTheme>();
        let border_color = theme.colors.border;
        let text_muted = theme.colors.text_muted;
        let accent = theme.colors.accent;
        let error_message = self.error_message.clone();
        let error_message_import = self.error_message.clone();
        let panel_id = cx.entity_id();

        div()
            .flex()
            .items_center()
            .gap(spacing::SM)
            .child(
                div()
                    .id("dict-export")
                    .px(spacing::MD)
                    .py(px(3.0))
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(border_color)
                    .text_size(px(11.0))
                    .text_color(text_muted)
                    .cursor_pointer()
                    .child(SharedString::from("Export JSON"))
                    .on_click(cx.listener(move |_this, _, _window, cx| {
                        let json = match cx.global::<VoxState>().dictionary().export_json() {
                            Ok(json) => json,
                            Err(err) => {
                                error_message.set(Some(format!("Export failed: {err}")));
                                cx.notify();
                                return;
                            }
                        };

                        let receiver = cx.prompt_for_new_path(
                            &std::path::PathBuf::default(),
                            Some("dictionary.json"),
                        );

                        cx.spawn(async move |_this, cx| {
                            let path = match receiver.await {
                                Ok(Ok(Some(path))) => path,
                                _ => return,
                            };
                            if let Err(err) = std::fs::write(&path, &json) {
                                cx.update(|cx| {
                                    tracing::warn!(%err, "failed to write dictionary export");
                                    cx.notify(panel_id);
                                });
                            }
                        })
                        .detach();
                    })),
            )
            .child(
                div()
                    .id("dict-import")
                    .px(spacing::MD)
                    .py(px(3.0))
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(border_color)
                    .text_size(px(11.0))
                    .text_color(accent)
                    .cursor_pointer()
                    .child(SharedString::from("Import JSON"))
                    .on_click(cx.listener(move |_this, _, _window, cx| {
                        let receiver = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Select dictionary JSON file".into()),
                        });
                        let error_msg = error_message_import.clone();

                        cx.spawn(async move |_this, cx| {
                            let paths = match receiver.await {
                                Ok(Ok(Some(paths))) => paths,
                                _ => return,
                            };
                            let Some(path) = paths.first() else {
                                return;
                            };
                            let json = match std::fs::read_to_string(path) {
                                Ok(json) => json,
                                Err(err) => {
                                    cx.update(|cx| {
                                        error_msg
                                            .set(Some(format!("Failed to read file: {err}")));
                                        cx.notify(panel_id);
                                    });
                                    return;
                                }
                            };
                            cx.update(|cx| {
                                match cx
                                    .global::<VoxState>()
                                    .dictionary()
                                    .import_json(&json)
                                {
                                    Ok(result) => {
                                        error_msg.set(None);
                                        tracing::info!(
                                            added = result.added,
                                            skipped = result.skipped,
                                            "dictionary import complete"
                                        );
                                    }
                                    Err(err) => {
                                        error_msg
                                            .set(Some(format!("Import failed: {err}")));
                                    }
                                }
                                cx.notify(panel_id);
                            });
                        })
                        .detach();
                    })),
            )
            .into_any_element()
    }

    /// Render the sort header row. Returns `AnyElement` to avoid Rust 2024
    /// lifetime capture on `impl IntoElement`.
    fn render_sort_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<VoxTheme>();
        let text_muted = theme.colors.text_muted;
        let sort_field = self.sort_field;
        let sort_asc = self.sort_ascending;

        let sort_indicator = |field: SortField| -> SharedString {
            if sort_field == field {
                if sort_asc {
                    SharedString::from(" ▲")
                } else {
                    SharedString::from(" ▼")
                }
            } else {
                SharedString::from("")
            }
        };

        div()
            .flex()
            .items_center()
            .gap(spacing::LG)
            .px(spacing::MD)
            .py(spacing::XS)
            .child(
                div()
                    .id("sort-spoken")
                    .text_size(px(11.0))
                    .text_color(text_muted)
                    .cursor_pointer()
                    .child(SharedString::from(format!(
                        "Spoken{}",
                        sort_indicator(SortField::Spoken)
                    )))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.sort_field == SortField::Spoken {
                            this.sort_ascending = !this.sort_ascending;
                        } else {
                            this.sort_field = SortField::Spoken;
                            this.sort_ascending = true;
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("sort-category")
                    .text_size(px(11.0))
                    .text_color(text_muted)
                    .cursor_pointer()
                    .child(SharedString::from(format!(
                        "Category{}",
                        sort_indicator(SortField::Category)
                    )))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.sort_field == SortField::Category {
                            this.sort_ascending = !this.sort_ascending;
                        } else {
                            this.sort_field = SortField::Category;
                            this.sort_ascending = true;
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("sort-uses")
                    .text_size(px(11.0))
                    .text_color(text_muted)
                    .cursor_pointer()
                    .child(SharedString::from(format!(
                        "Uses{}",
                        sort_indicator(SortField::UseCount)
                    )))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.sort_field == SortField::UseCount {
                            this.sort_ascending = !this.sort_ascending;
                        } else {
                            this.sort_field = SortField::UseCount;
                            this.sort_ascending = false; // Most-used first by default
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Render the add-entry form row. Returns `AnyElement` to avoid Rust 2024
    /// lifetime capture on `impl IntoElement`.
    fn render_add_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<VoxTheme>();
        let button_bg = theme.colors.button_primary_bg;
        let button_text = theme.colors.button_primary_text;
        let error_color = theme.colors.status_error;
        // NLL releases theme borrow after this line

        let panel_id = cx.entity_id();
        let error_msg = self.error_message.clone();
        let error_display = self.error_message.take();
        // Put the value back so the click handler can see/clear it
        if let Some(ref msg) = error_display {
            self.error_message.set(Some(msg.clone()));
        }

        let new_spoken = self.new_spoken_input.clone();
        let new_written = self.new_written_input.clone();
        let new_category = self.new_category_input.clone();

        let mut form = div().flex().flex_col().gap(spacing::XS);

        // Input row
        form = form.child(
            div()
                .flex()
                .items_center()
                .gap(spacing::SM)
                .child(self.new_spoken_input.clone())
                .child(self.new_written_input.clone())
                .child(self.new_category_input.clone())
                .child(
                    div()
                        .id("add-entry-btn")
                        .px(spacing::MD)
                        .py(spacing::SM)
                        .rounded(radius::SM)
                        .bg(button_bg)
                        .text_color(button_text)
                        .text_size(px(12.0))
                        .cursor_pointer()
                        .child(SharedString::from("Add"))
                        .on_click(move |_, _window, cx| {
                            let spoken = new_spoken.read(cx).content().to_string();
                            let written = new_written.read(cx).content().to_string();
                            let category = new_category.read(cx).content().to_string();

                            if spoken.is_empty() {
                                error_msg.set(Some("Spoken form is required".into()));
                                cx.notify(panel_id);
                                return;
                            }

                            let category = if category.is_empty() {
                                "general".to_string()
                            } else {
                                category
                            };

                            match cx
                                .global::<VoxState>()
                                .dictionary()
                                .add(&spoken, &written, &category, false)
                            {
                                Ok(_) => {
                                    // Clear inputs after successful add
                                    new_spoken.update(cx, |input, _cx| {
                                        input.set_content(String::new());
                                    });
                                    new_written.update(cx, |input, _cx| {
                                        input.set_content(String::new());
                                    });
                                    new_category.update(cx, |input, _cx| {
                                        input.set_content(String::new());
                                    });
                                    error_msg.set(None);
                                }
                                Err(err) => {
                                    error_msg.set(Some(format!("{err}")));
                                }
                            }
                            cx.notify(panel_id);
                        }),
                ),
        );

        // Error message
        if let Some(msg) = error_display {
            form = form.child(
                div()
                    .text_size(px(11.0))
                    .text_color(error_color)
                    .child(SharedString::from(msg)),
            );
        }

        form.into_any_element()
    }
}

impl Render for DictionaryPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Mutable operation first (before borrowing theme)
        self.refresh_entries(cx);

        let panel_id = cx.entity_id();

        // Extract owned theme values so the borrow is released by NLL
        let theme = cx.global::<VoxTheme>();
        let scrollbar_thumb = theme.colors.scrollbar_thumb;
        let scrollbar_track = theme.colors.scrollbar_track;
        let text_color = theme.colors.text;
        let muted_color = theme.colors.text_muted;
        let colors = theme.colors.clone();
        // theme borrow released by NLL — safe to use cx mutably below

        // Build sub-components (they access theme internally via cx)
        let add_form = self.render_add_form(cx);
        let sort_header = self.render_sort_header(cx);
        let import_export = self.render_import_export(cx);

        // Build edit rows for any entry being edited (must happen before the
        // immutable borrow of self.entries in the for-loop below)
        let current_editing_id = self.editing_id.get();
        let mut edit_row: Option<(i64, AnyElement)> = None;
        if let Some(eid) = current_editing_id {
            if let Some(entry) = self.entries.iter().find(|e| e.id == eid) {
                let entry_clone = entry.clone();
                edit_row = Some((eid, self.render_edit_row(&entry_clone, cx)));
            }
        }

        let mut scroll_content = div()
            .id("dictionary-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .flex()
            .flex_col()
            .p(spacing::LG)
            .gap(spacing::SM);

        // Header
        scroll_content = scroll_content.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb(spacing::SM)
                .child(
                    div()
                        .text_size(px(16.0))
                        .text_color(text_color)
                        .child(SharedString::from("Custom Dictionary")),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::SM)
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(muted_color)
                                .child(SharedString::from(format!(
                                    "{} entries",
                                    self.entries.len()
                                ))),
                        )
                        .child(import_export),
                ),
        );

        // Search input
        scroll_content = scroll_content.child(self.search_input.clone());

        // Add form and sort header (already built as AnyElement)
        scroll_content = scroll_content.child(add_form);
        scroll_content = scroll_content.child(sort_header);

        // Entry list or empty state
        if self.entries.is_empty() {
            let empty_msg = if self.search_query.is_empty() {
                "No dictionary entries. Add your first entry to get started."
            } else {
                "No entries match your search"
            };

            scroll_content = scroll_content.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .py(spacing::XL)
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(muted_color)
                            .child(SharedString::from(empty_msg)),
                    ),
            );
        } else {
            for entry in &self.entries {
                if edit_row.as_ref().map_or(false, |(eid, _)| *eid == entry.id) {
                    // Swap in the pre-built edit row for this entry
                    if let Some((_, element)) = edit_row.take() {
                        scroll_content = scroll_content.child(element);
                    }
                } else {
                    scroll_content = scroll_content.child(Self::render_entry(
                        entry,
                        &colors,
                        panel_id,
                        &self.confirming_delete,
                        &self.editing_id,
                        &self.edit_spoken_input,
                        &self.edit_written_input,
                        &self.edit_category_input,
                    ));
                }
            }
        }

        // Scrollbar as SIBLING of scroll container
        div()
            .size_full()
            .child(scroll_content)
            .child(Scrollbar::new(
                self.scroll_handle.clone(),
                cx.entity_id(),
                self.scrollbar_drag.clone(),
                scrollbar_thumb,
                scrollbar_track,
            ))
            .into_any_element()
    }
}
