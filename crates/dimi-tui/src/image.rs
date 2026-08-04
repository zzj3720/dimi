//! Image thumbnail — transcript-side rendering of a pasted image (port of
//! `apps/dimi/src/tui/components/media/image-thumbnail.ts`).
//!
//! On terminals that speak the Kitty graphics protocol or iTerm2 inline image
//! protocol the TS side shows the actual image. **That path is future work in
//! Rust** (the kitty image protocol is not yet implemented in the render core
//! — see the review record); this slice ports the fallback: a one-line accent
//! placeholder matching what the user sees in the input box, keeping the
//! transcript readable on terminals without image support.

use crate::component::Component;
use crate::components::text::Text;
use crate::theme::{ColorToken, current_theme};

/// Height cap for the inline image (~12 rows) — kept for the future kitty
/// path (`MAX_IMAGE_ROWS`).
#[allow(dead_code)] // used by the future kitty path
const MAX_IMAGE_ROWS: usize = 12;
/// Width cap for the inline image (`MAX_IMAGE_WIDTH`).
#[allow(dead_code)] // used by the future kitty path
const MAX_IMAGE_WIDTH: usize = 40;

/// A pasted image attachment (subset of the TS `ImageAttachment`).
#[derive(Debug, Clone)]
pub struct ImageAttachment {
    pub placeholder: String,
    pub mime: String,
    pub width_px: u32,
    pub height_px: u32,
    pub bytes: Vec<u8>,
}

/// Fallback-only image thumbnail component.
///
/// TODO(future): when the render core gains kitty image support, render the
/// actual image via the kitty protocol for terminals whose capabilities
/// advertise `images === 'kitty' | 'iterm2'`; every other terminal falls back
/// to the accent placeholder line below.
pub struct ImageThumbnailComponent {
    attachment: ImageAttachment,
}

impl ImageThumbnailComponent {
    pub fn new(attachment: ImageAttachment) -> Self {
        ImageThumbnailComponent { attachment }
    }
}

impl Component for ImageThumbnailComponent {
    fn render(&mut self, width: usize) -> Vec<String> {
        let safe_width = width;
        // Fallback path only: `new Text(fg('accent', placeholder), 0, 0)`.
        let theme = current_theme();
        let line = theme.fg(ColorToken::Accent, &self.attachment.placeholder);
        Text::new(&line, 0, 0).render(safe_width)
    }

    fn invalidate(&mut self) {}
}
