//! `Spacer` component — renders empty lines
//! (port of `@dimi-agent/pi-tui` `src/components/spacer.ts`).

use crate::component::Component;

/// Renders `lines` empty lines.
pub struct Spacer {
    lines: usize,
}

impl Spacer {
    pub fn new(lines: usize) -> Self {
        Spacer { lines }
    }

    pub fn set_lines(&mut self, lines: usize) {
        self.lines = lines;
    }
}

impl Component for Spacer {
    fn render(&mut self, _width: usize) -> Vec<String> {
        vec![String::new(); self.lines]
    }

    fn invalidate(&mut self) {}
}
