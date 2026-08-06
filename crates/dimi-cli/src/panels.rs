//! App-level panel hosts: the queue pane host and the editor slot.
//!
//! - [`QueuePanelHost`] mounts a `dimi_tui::chrome::QueuePaneComponent` only
//!   while the send queue has messages (the pane always renders its top
//!   border, but the TS only mounts it when `messages.length > 0`, so the host
//!   hides it entirely — 0 lines — when the queue is empty).
//! - [`EditorSlotComponent`] renders the session picker while it is open and
//!   the editor otherwise — the Rust analogue of the TS `mountEditorReplacement`
//!   / `restoreEditor` pattern (the picker replaces the editor in the mount
//!   tree, and is removed to restore the editor).

use std::cell::RefCell;
use std::rc::Rc;

use dimi_tui::chrome::{QueuePaneComponent, QueuePaneOptions};
use dimi_tui::component::Component;
use dimi_tui::custom_editor::CustomEditor;
use dimi_tui::dialogs::session_picker::SessionPickerComponent;

/// Host for the queue pane: `Some(QueuePaneComponent)` while the queue has
/// messages, `None` (renders 0 lines) while empty.
pub struct QueuePanelHost {
    inner: Option<QueuePaneComponent>,
}

impl QueuePanelHost {
    pub fn new() -> Self {
        QueuePanelHost { inner: None }
    }

    /// Rebuild the pane from the current queue options (empty queue → hidden).
    pub fn update(&mut self, options: QueuePaneOptions) {
        self.inner = if options.messages.is_empty() {
            None
        } else {
            Some(QueuePaneComponent::new(options))
        };
    }

    /// `#[allow(dead_code)]`: read by tests; the app rebuilds the pane via
    /// [`QueuePanelHost::update`] and never queries visibility directly.
    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.inner.is_some()
    }
}

impl Default for QueuePanelHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for QueuePanelHost {
    fn render(&mut self, width: usize) -> Vec<String> {
        match &mut self.inner {
            Some(pane) => pane.render(width),
            None => Vec::new(),
        }
    }

    fn invalidate(&mut self) {
        if let Some(pane) = &mut self.inner {
            pane.invalidate();
        }
    }
}

/// The editor slot: delegates rendering + input to the session picker while it
/// is open, and to the editor otherwise. Mounted once at the editor child
/// index; the host swaps the picker in/out via the shared `Rc<RefCell<…>>`.
pub struct EditorSlotComponent {
    editor: Rc<RefCell<CustomEditor>>,
    picker: Rc<RefCell<Option<SessionPickerComponent>>>,
}

impl EditorSlotComponent {
    pub fn new(
        editor: Rc<RefCell<CustomEditor>>,
        picker: Rc<RefCell<Option<SessionPickerComponent>>>,
    ) -> Self {
        EditorSlotComponent { editor, picker }
    }
}

impl Component for EditorSlotComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        if self.picker.borrow().is_some() {
            self.picker.borrow_mut().as_mut().unwrap().render(width)
        } else {
            self.editor.borrow_mut().render(width)
        }
    }

    fn handle_input(&mut self, data: &str) {
        if self.picker.borrow().is_some() {
            self.picker
                .borrow_mut()
                .as_mut()
                .unwrap()
                .handle_input(data);
        } else {
            self.editor.borrow_mut().handle_input(data);
        }
    }

    fn invalidate(&mut self) {
        self.editor.borrow_mut().invalidate();
        if let Some(picker) = self.picker.borrow_mut().as_mut() {
            picker.invalidate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dimi_tui::ansi::strip_ansi;
    use dimi_tui::theme::{DARK_COLORS, set_palette};

    #[test]
    fn queue_host_hides_when_empty_and_shows_messages() {
        set_palette(DARK_COLORS);
        let mut host = QueuePanelHost::new();
        assert!(!host.is_visible());
        assert!(Component::render(&mut host, 80).is_empty());

        host.update(QueuePaneOptions {
            messages: vec![
                dimi_tui::chrome::QueuedMessage::new("first", None),
                dimi_tui::chrome::QueuedMessage::new("ls -la", Some("bash")),
            ],
            is_compacting: false,
            is_streaming: true,
            can_steer_immediately: true,
            enter_steers_by_default: false,
        });
        assert!(host.is_visible());
        let joined = strip_ansi(&Component::render(&mut host, 80).join("\n"));
        assert!(joined.contains("first"), "{joined}");
        assert!(joined.contains("ls -la"), "{joined}");

        host.update(QueuePaneOptions {
            messages: Vec::new(),
            is_compacting: false,
            is_streaming: false,
            can_steer_immediately: true,
            enter_steers_by_default: false,
        });
        assert!(!host.is_visible());
        assert!(Component::render(&mut host, 80).is_empty());
    }
}
