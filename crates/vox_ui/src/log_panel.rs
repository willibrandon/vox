//! Live log viewer panel with severity filtering and auto-scroll.
//!
//! Provides [`LogPanel`] as an entity with `Render` impl, backed by a
//! [`LogStore`] entity that receives log entries from the tracing subscriber
//! via [`LogReceiver`]. Uses `uniform_list` for virtualized rendering.

use std::collections::VecDeque;

use gpui::{
    div, prelude::*, px, uniform_list, ClipboardItem, Context, Entity, EventEmitter, IntoElement,
    Render, ScrollStrategy, SharedString, Subscription, Task, UniformListScrollHandle, Window,
};

use vox_core::log_sink::{LogEntry, LogLevel, LogReceiver};

use crate::layout::{radius, spacing};
use crate::theme::{ThemeColors, VoxTheme};

/// Maximum number of log entries retained in memory.
const MAX_LOG_ENTRIES: usize = 10_000;

/// Events emitted by the log store when entries change.
pub enum LogStoreEvent {
    /// One or more new entries were added.
    NewEntries,
    /// All entries were cleared.
    Cleared,
}

/// GPUI global holding the persistent LogStore entity.
///
/// Created once during application startup in `run_app()` and shared
/// across settings window open/close cycles. Without this, the
/// take-once `LogReceiver` gets consumed by the first settings window
/// and subsequent windows see no log entries.
pub struct SharedLogStore(pub Entity<LogStore>);

impl gpui::Global for SharedLogStore {}

/// In-memory log entry buffer with capacity limit.
///
/// Receives entries from the tracing subscriber via a tokio channel and
/// emits events for the UI to refresh.
pub struct LogStore {
    /// Bounded ring buffer of log entries.
    entries: VecDeque<LogEntry>,
    /// Background task polling the log receiver channel.
    _poll_task: Task<()>,
}

impl EventEmitter<LogStoreEvent> for LogStore {}

impl LogStore {
    /// Create a new log store that polls the given receiver for entries.
    ///
    /// Batches all available entries per poll cycle to minimize re-renders
    /// under high throughput (100+ entries/sec).
    pub fn new(cx: &mut Context<Self>, mut receiver: LogReceiver) -> Self {
        let poll_task = cx.spawn(async move |this, cx| {
            while let Some(first) = receiver.rx.recv().await {
                // Drain all available entries in one batch
                let mut batch = vec![first];
                while let Ok(entry) = receiver.rx.try_recv() {
                    batch.push(entry);
                }
                let _ = this.update(cx, |store, cx| {
                    for entry in batch {
                        store.entries.push_back(entry);
                    }
                    while store.entries.len() > MAX_LOG_ENTRIES {
                        store.entries.pop_front();
                    }
                    cx.emit(LogStoreEvent::NewEntries);
                    cx.notify();
                });
            }
        });

        Self {
            entries: VecDeque::with_capacity(1024),
            _poll_task: poll_task,
        }
    }

    /// Number of entries currently stored.
    fn len(&self) -> usize {
        self.entries.len()
    }

    /// Get an entry by index.
    fn get(&self, index: usize) -> Option<&LogEntry> {
        self.entries.get(index)
    }

    /// Remove all entries.
    fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Log panel displaying live application log output.
///
/// Filters entries by severity level and supports auto-scrolling
/// to the latest entry.
pub struct LogPanel {
    /// The backing log store entity.
    log_store: Entity<LogStore>,
    /// Whether to auto-scroll to new entries.
    auto_scroll: bool,
    /// Minimum severity level to display (entries at or above this level shown).
    filter_level: LogLevel,
    /// Scroll handle for the uniform list.
    scroll_handle: UniformListScrollHandle,
    /// Subscription to log store events.
    _subscription: Subscription,
}

impl LogPanel {
    /// Create a new log panel backed by the shared global LogStore.
    ///
    /// The LogStore persists across settings window open/close cycles.
    /// If the global hasn't been set (shouldn't happen in normal startup),
    /// falls back to a dummy store.
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let log_store = if let Some(shared) = cx.try_global::<SharedLogStore>() {
            shared.0.clone()
        } else {
            // Fallback: no global set (shouldn't happen). Use dummy receiver.
            cx.new(|cx| {
                let (_, receiver) = vox_core::log_sink::LogSink::new();
                LogStore::new(cx, receiver)
            })
        };

        let scroll_handle = UniformListScrollHandle::new();
        let scroll_handle_for_sub = scroll_handle.clone();

        let subscription = cx.subscribe(&log_store, move |this, _store, event, cx| match event {
            LogStoreEvent::NewEntries => {
                if this.auto_scroll {
                    let count = this.log_store.read(cx).len();
                    if count > 0 {
                        scroll_handle_for_sub.scroll_to_item(count - 1, ScrollStrategy::Bottom);
                    }
                }
                cx.notify();
            }
            LogStoreEvent::Cleared => {
                cx.notify();
            }
        });

        Self {
            log_store,
            auto_scroll: true,
            filter_level: LogLevel::Info,
            scroll_handle,
            _subscription: subscription,
        }
    }

    /// Render a single log entry row with copy button.
    fn render_entry(
        entry: &LogEntry,
        colors: &ThemeColors,
        entry_index: usize,
    ) -> impl IntoElement {
        let level_color = match entry.level {
            LogLevel::Error => colors.log_error,
            LogLevel::Warn => colors.log_warn,
            LogLevel::Info => colors.log_info,
            LogLevel::Debug => colors.log_debug,
            LogLevel::Trace => colors.log_trace,
        };

        let level_text = SharedString::from(format!("{:5}", entry.level));
        let timestamp = SharedString::from(
            if entry.timestamp.len() >= 19 {
                &entry.timestamp[11..19]
            } else {
                &entry.timestamp
            }
            .to_string(),
        );
        let target = SharedString::from(entry.target.clone());
        let message = SharedString::from(entry.message.clone());

        // Pre-format for clipboard: "[timestamp] [LEVEL] target: message"
        let clipboard_text = format!(
            "[{}] [{}] {}: {}",
            entry.timestamp, entry.level, entry.target, entry.message
        );
        let copy_text_color = colors.text_muted;

        div()
            .flex()
            .items_center()
            .gap(spacing::SM)
            .px(spacing::SM)
            .py(px(2.0))
            .text_size(px(12.0))
            .child(
                div()
                    .text_color(colors.text_muted)
                    .min_w(px(60.0))
                    .child(timestamp),
            )
            .child(
                div()
                    .text_color(level_color)
                    .min_w(px(45.0))
                    .child(level_text),
            )
            .child(
                div()
                    .text_color(colors.text_muted)
                    .min_w(px(80.0))
                    .child(target),
            )
            .child(div().text_color(colors.text).flex_1().child(message))
            .child(
                div()
                    .id(SharedString::from(format!("copy-log-{entry_index}")))
                    .text_size(px(10.0))
                    .text_color(copy_text_color)
                    .cursor_pointer()
                    .px(spacing::XS)
                    .child(SharedString::from("Copy"))
                    .on_click(move |_, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(clipboard_text.clone()));
                    }),
            )
    }

    /// All available filter levels in display order.
    const ALL_LEVELS: [LogLevel; 5] = [
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ];
}

impl Render for LogPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let store = self.log_store.read(cx);
        let filter_level = self.filter_level;

        // Collect filtered entries for the uniform_list closure
        let entries: Vec<LogEntry> = (0..store.len())
            .filter_map(|i| {
                let entry = store.get(i)?;
                if entry.level <= filter_level {
                    Some(entry.clone())
                } else {
                    None
                }
            })
            .collect();

        let entry_count = entries.len();
        let colors = theme.colors.clone();

        // Release the read borrow before building the element tree
        let _ = store;

        // --- Header bar ---
        let title = div()
            .text_size(px(16.0))
            .text_color(theme.colors.text)
            .child(SharedString::from("Application Logs"));

        // Filter level buttons (T045)
        let mut filter_row = div().flex().items_center().gap(spacing::XS);
        for level in Self::ALL_LEVELS {
            let is_active = level == self.filter_level;
            let level_label = SharedString::from(format!("{level}"));
            let level_color = match level {
                LogLevel::Error => theme.colors.log_error,
                LogLevel::Warn => theme.colors.log_warn,
                LogLevel::Info => theme.colors.log_info,
                LogLevel::Debug => theme.colors.log_debug,
                LogLevel::Trace => theme.colors.log_trace,
            };

            filter_row = filter_row.child(
                div()
                    .id(SharedString::from(format!("filter-{level}")))
                    .text_size(px(11.0))
                    .px(spacing::SM)
                    .py(px(2.0))
                    .rounded(radius::SM)
                    .cursor_pointer()
                    .when(is_active, |d| {
                        d.bg(level_color)
                            .text_color(theme.colors.surface)
                    })
                    .when(!is_active, |d| {
                        d.text_color(level_color)
                            .border_1()
                            .border_color(theme.colors.border)
                    })
                    .child(level_label)
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.filter_level = level;
                        cx.notify();
                    })),
            );
        }

        // Auto-scroll toggle (T046)
        let auto_scroll_text = SharedString::from(if self.auto_scroll {
            "Auto-scroll: On"
        } else {
            "Auto-scroll: Off"
        });
        let auto_scroll_color = if self.auto_scroll {
            theme.colors.accent
        } else {
            theme.colors.text_muted
        };

        // Clear button (T046)
        let clear_color = theme.colors.text_muted;
        let clear_border = theme.colors.border;

        // Entry count
        let count_text = SharedString::from(format!("{entry_count} entries"));

        let header = div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::MD)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(title)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(spacing::SM)
                            .child(
                                div()
                                    .id("log-auto-scroll")
                                    .text_size(px(11.0))
                                    .text_color(auto_scroll_color)
                                    .px(spacing::SM)
                                    .py(px(2.0))
                                    .rounded(radius::SM)
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .cursor_pointer()
                                    .child(auto_scroll_text)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.auto_scroll = !this.auto_scroll;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("log-clear")
                                    .text_size(px(11.0))
                                    .text_color(clear_color)
                                    .px(spacing::SM)
                                    .py(px(2.0))
                                    .rounded(radius::SM)
                                    .border_1()
                                    .border_color(clear_border)
                                    .cursor_pointer()
                                    .child(SharedString::from("Clear"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.log_store.update(cx, |store, cx| {
                                            store.clear();
                                            cx.emit(LogStoreEvent::Cleared);
                                            cx.notify();
                                        });
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.colors.text_muted)
                                    .child(count_text),
                            ),
                    ),
            )
            .child(filter_row);

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                uniform_list("log-list", entry_count, move |range, _window, _cx| {
                    range
                        .map(|ix| {
                            Self::render_entry(&entries[ix], &colors, ix).into_any_element()
                        })
                        .collect()
                })
                .flex_1()
                .track_scroll(&self.scroll_handle),
            )
    }
}
