//! Component trait — the pi-tui `Component` contract, ported.
//!
//! Every TUI component renders to a `Vec<String>` of ANSI-escaped lines for a
//! given viewport width. Differential rendering compares these lines between
//! frames, so components should keep their own render caches and return
//! identical strings for unchanged content (the same contract pi-tui relies
//! on for its processed-line fast path).

use std::cell::RefCell;
use std::rc::Rc;

/// Cursor position marker — APC (Application Program Command) sequence.
/// Terminals ignore it; a focused component emits it at the cursor position,
/// and the TUI finds, strips, and positions the hardware cursor there (IME
/// candidate window placement).
pub const CURSOR_MARKER: &str = "\x1b_pi:c\x07";

/// Component interface — all components must implement this.
pub trait Component {
    /// Render the component to lines for the given viewport width.
    /// Returns a string per line, each possibly containing ANSI escapes.
    fn render(&mut self, width: usize) -> Vec<String>;

    /// Optional handler for keyboard input when the component has focus.
    fn handle_input(&mut self, _data: &str) {}

    /// If true, the component receives key release events (Kitty protocol).
    /// Default is false — release events are filtered out.
    fn wants_key_release(&self) -> bool {
        false
    }

    /// Invalidate any cached rendering state. Called when the theme changes
    /// or the component needs to re-render from scratch.
    fn invalidate(&mut self) {}

    /// Downcast to a focusable component (default: not focusable).
    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        None
    }
}

/// Interface for components that can receive focus and display a hardware
/// cursor. When focused, the component should emit [`CURSOR_MARKER`] at the
/// cursor position in its render output.
pub trait Focusable {
    /// Set by the TUI when focus changes. The component should emit
    /// [`CURSOR_MARKER`] when true.
    fn focused(&self) -> bool;

    /// Called by the TUI on focus changes.
    fn set_focused(&mut self, focused: bool);
}

/// Downcast helper for `Box<dyn Component>` → `&mut dyn Focusable`.
/// Components that implement both traits can opt in via
/// [`Component::as_focusable_mut`].
pub fn component_as_focusable_mut(c: &mut dyn Component) -> Option<&mut dyn Focusable> {
    c.as_focusable_mut()
}

/// Type-level helper mirroring pi-tui's `isFocusable`: runtime detection is
/// not needed in Rust (a component that implements both traits is known at
/// compile time), kept as a convenience accessor.
pub fn is_focusable<C: Component + Focusable>(_c: &C) -> bool {
    true
}

/// Shared component wrapper — lets the coordinator hold a component by
/// `Rc<RefCell<…>>` while the same component is mounted in the render tree.
/// Mirrors the TS pattern where `DimiTUI` keeps `state.footer` and also adds
/// the footer to the layout tree.
pub struct SharedComponent<C> {
    inner: Rc<RefCell<C>>,
}

impl<C> SharedComponent<C> {
    pub fn new(inner: Rc<RefCell<C>>) -> Self {
        SharedComponent { inner }
    }

    pub fn inner(&self) -> &Rc<RefCell<C>> {
        &self.inner
    }
}

impl<C: Component> Component for SharedComponent<C> {
    fn render(&mut self, width: usize) -> Vec<String> {
        self.inner.borrow_mut().render(width)
    }

    fn handle_input(&mut self, data: &str) {
        self.inner.borrow_mut().handle_input(data);
    }

    fn wants_key_release(&self) -> bool {
        self.inner.borrow().wants_key_release()
    }

    fn invalidate(&mut self) {
        self.inner.borrow_mut().invalidate();
    }
}
