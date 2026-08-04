//! Frame-level snapshot diff harness — replays the same component sequence
//! the TS capture produced (`testdata/frames-golden.jsonl`, captured from the
//! real pi-tui TUI + VirtualTerminal) through the Rust differential Tui and
//! compares the per-frame ANSI write sequences byte-for-byte.

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use dimi_tui::component::Component;
use dimi_tui::components::messages::assistant_message::AssistantMessageComponent;
use dimi_tui::components::messages::thinking::{ThinkingComponent, ThinkingMode};
use dimi_tui::components::messages::tool_call::ToolCallComponent;
use dimi_tui::components::messages::tool_renderers::{ToolCallData, ToolResultData};
use dimi_tui::components::messages::user_message::UserMessageComponent;
use dimi_tui::terminal::{RecordingTerminal, Terminal};
use dimi_tui::theme::{DARK_COLORS, set_palette};
use dimi_tui::tui::Tui;
use serde::Deserialize;

#[derive(Deserialize)]
struct FrameFixture {
    #[allow(dead_code)]
    frame: usize,
    writes: String,
}

/// Terminal wrapper sharing the recorder through Rc so the test can read
/// writes after each render.
struct SharedTerminal {
    inner: Rc<RefCell<RecordingTerminal>>,
}

impl Terminal for SharedTerminal {
    fn start(&mut self, _on_input: &mut dyn FnMut(&str), _on_resize: &mut dyn FnMut()) {}
    fn stop(&mut self) {}
    fn write(&mut self, data: &str) {
        self.inner.borrow_mut().write(data);
    }
    fn columns(&self) -> usize {
        self.inner.borrow().columns_value
    }
    fn rows(&self) -> usize {
        self.inner.borrow().rows_value
    }
    fn hide_cursor(&mut self) {}
    fn show_cursor(&mut self) {}
}

#[derive(Deserialize, Clone)]
struct Spec {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    args: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    result: Option<ResultSpec>,
}

#[derive(Deserialize, Clone)]
struct ResultSpec {
    output: String,
    #[serde(default)]
    is_error: bool,
}

const SEQUENCE: &str = r#"[
  {"type":"user","content":"Hello there!"},
  {"type":"thinking","content":"Let me think about this carefully."},
  {"type":"assistant","content":"Hello **world**!"},
  {"type":"tool","name":"Bash","args":{"command":"ls -la"},"result":{"output":"total 8\ndrwxr-xr-x  5 user staff  160 Aug  4 10:00 .\n-rw-r--r--  1 user staff  123 Aug  4 10:00 file.txt"}},
  {"type":"assistant","content":"Another assistant reply"},
  {"type":"tool","name":"WebSearch","args":{"query":"rust terminal ui"},"result":{"output":"Search results..."}}
]"#;

fn make_component(spec: &Spec, index: usize) -> Box<dyn Component> {
    match spec.kind.as_str() {
        "user" => Box::new(UserMessageComponent::new(&spec.content, None)),
        "thinking" => Box::new(ThinkingComponent::new(
            &spec.content,
            true,
            ThinkingMode::Finalized,
        )),
        "assistant" => {
            let mut c = AssistantMessageComponent::new(true);
            c.update_content(&spec.content);
            Box::new(c)
        }
        "tool" => {
            let id = format!("call_{}", index + 1);
            let result = spec.result.as_ref().map(|r| ToolResultData {
                tool_call_id: id.clone(),
                output: r.output.clone(),
                is_error: r.is_error,
            });
            Box::new(ToolCallComponent::new(
                ToolCallData {
                    id,
                    name: spec.name.clone(),
                    args: spec.args.clone(),
                    truncated: false,
                },
                result,
            ))
        }
        other => panic!("unknown spec kind {other}"),
    }
}

#[test]
fn frame_diff_matches_ts_capture() {
    set_palette(DARK_COLORS);
    let golden_path = format!(
        "{}/testdata/frames-golden.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let data = fs::read_to_string(golden_path).expect("frames golden file");
    let frames: Vec<FrameFixture> = data
        .lines()
        .map(|l| serde_json::from_str(l).expect("frame json"))
        .collect();
    assert_eq!(frames.len(), 7, "expected 7 frames (startup + 6 events)");

    let specs: Vec<Spec> = serde_json::from_str(SEQUENCE).expect("sequence json");

    let term = Rc::new(RefCell::new(RecordingTerminal::new(80, 24)));
    let mut tui = Tui::new(Box::new(SharedTerminal {
        inner: term.clone(),
    }));
    tui.start();

    let mut captured: Vec<String> = Vec::new();
    // Frame 0: startup empty render.
    term.borrow_mut().clear_writes();
    tui.request_render();
    captured.push(term.borrow().output());

    // Frames 1..=6: mount one component per frame.
    for (i, spec) in specs.iter().enumerate() {
        tui.add_child(make_component(spec, i));
        term.borrow_mut().clear_writes();
        tui.request_render();
        captured.push(term.borrow().output());
    }

    assert_eq!(captured.len(), frames.len());
    for (i, (got, want)) in captured.iter().zip(frames.iter()).enumerate() {
        assert_eq!(got, &want.writes, "frame {} write sequence mismatch", i);
    }
    tui.stop();
}
