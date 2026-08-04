//! Container — a component that contains other components, ported from
//! pi-tui's `Container`.

use crate::component::Component;

/// A component that renders its children top-to-bottom.
///
/// The TS reference holds children by reference and shares component
/// instances between the coordinator and the tree. This first slice takes
/// ownership via `Box<dyn Component>`; the shared-ownership pattern
/// (`Rc<RefCell<…>>` or an arena) lands with the controllers slice where the
/// coordinator actually needs to mutate a mounted component.
pub struct Container {
    children: Vec<Box<dyn Component>>,
}

impl Container {
    pub fn new() -> Self {
        Container {
            children: Vec::new(),
        }
    }

    pub fn add_child(&mut self, component: Box<dyn Component>) {
        self.children.push(component);
    }

    pub fn remove_child(&mut self, index: usize) {
        if index < self.children.len() {
            self.children.remove(index);
        }
    }

    pub fn clear(&mut self) {
        self.children.clear();
    }

    pub fn children(&self) -> &[Box<dyn Component>] {
        &self.children
    }

    pub fn children_mut(&mut self) -> &mut Vec<Box<dyn Component>> {
        &mut self.children
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Container {
    fn render(&mut self, width: usize) -> Vec<String> {
        // Extremely narrow terminals can report tiny or even non-positive
        // column counts; never propagate a width below 1 into components.
        let width = width.max(1);
        let mut lines = Vec::new();
        for child in &mut self.children {
            lines.extend(child.render(width));
        }
        lines
    }

    fn handle_input(&mut self, data: &str) {
        for child in &mut self.children {
            child.handle_input(data);
        }
    }

    fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
    }
}
