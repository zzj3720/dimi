//! Golden tests for the tool-call component against TS-captured byte-exact
//! output (`testdata/components-tools-golden.jsonl`).

#[cfg(test)]
mod tests {
    use crate::component::Component;
    use crate::components::messages::tool_call::ToolCallComponent;
    use crate::components::messages::tool_renderers::{ToolCallData, ToolResultData};
    use crate::theme::{DARK_COLORS, set_palette};
    use serde::Deserialize;
    use serde_json::json;
    use std::fs;

    #[derive(Deserialize)]
    struct Fixture {
        name: String,
        #[serde(default)]
        width: usize,
        #[serde(default)]
        lines: Vec<String>,
    }

    fn golden_path() -> String {
        format!(
            "{}/testdata/components-tools-golden.jsonl",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    fn args(obj: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        obj.as_object().cloned().unwrap_or_default()
    }

    fn build(name: &str) -> Box<dyn Component> {
        match name {
            "tool_bash_finished" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_1".into(),
                    name: "Bash".into(),
                    args: args(json!({"command": "ls -la"})),
                    truncated: false,
                },
                Some(ToolResultData {
                    tool_call_id: "call_1".into(),
                    output: "total 8\ndrwxr-xr-x  5 user staff  160 Aug  4 10:00 .\n-rw-r--r--  1 user staff  123 Aug  4 10:00 file.txt".into(),
                    is_error: false,
                }),
            )),
            "tool_bash_error" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_2".into(),
                    name: "Bash".into(),
                    args: args(json!({"command": "ls -la"})),
                    truncated: false,
                },
                Some(ToolResultData {
                    tool_call_id: "call_2".into(),
                    output: "ls: no such file: /nope".into(),
                    is_error: true,
                }),
            )),
            "tool_bash_inflight" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_3".into(),
                    name: "Bash".into(),
                    args: args(json!({"command": "sleep 10"})),
                    truncated: false,
                },
                None,
            )),
            "tool_read_finished" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_4".into(),
                    name: "Read".into(),
                    args: args(json!({"file_path": "src/main.ts"})),
                    truncated: false,
                },
                Some(ToolResultData {
                    tool_call_id: "call_4".into(),
                    output: "import { x } from \"./y\";\nconst z = 1;".into(),
                    is_error: false,
                }),
            )),
            "tool_exitplan_approved" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_5".into(),
                    name: "ExitPlanMode".into(),
                    args: args(json!({"plan": "Step one\nStep two"})),
                    truncated: false,
                },
                Some(ToolResultData {
                    tool_call_id: "call_5".into(),
                    output: "{\"kind\":\"approved\",\"chosen\":\"Continue with the plan\"}".into(),
                    is_error: false,
                }),
            )),
            "tool_alldone" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_6".into(),
                    name: "AllDone".into(),
                    args: args(json!({})),
                    truncated: false,
                },
                Some(ToolResultData {
                    tool_call_id: "call_6".into(),
                    output: String::new(),
                    is_error: false,
                }),
            )),
            "tool_askuser" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_7".into(),
                    name: "AskUserQuestion".into(),
                    args: args(json!({"questions": [{"question": "Pick one?"}]})),
                    truncated: false,
                },
                Some(ToolResultData {
                    tool_call_id: "call_7".into(),
                    output: "{\"answers\":{}}".into(),
                    is_error: false,
                }),
            )),
            "tool_websearch" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_8".into(),
                    name: "WebSearch".into(),
                    args: args(json!({"query": "rust terminal ui"})),
                    truncated: false,
                },
                Some(ToolResultData {
                    tool_call_id: "call_8".into(),
                    output: "Search results...".into(),
                    is_error: false,
                }),
            )),
            "tool_truncated" => Box::new(ToolCallComponent::new(
                ToolCallData {
                    id: "call_9".into(),
                    name: "Bash".into(),
                    args: args(json!({"command": "echo truncated"})),
                    truncated: true,
                },
                None,
            )),
            other => panic!("unknown fixture {other}"),
        }
    }

    #[test]
    fn tools_golden_byte_exact() {
        set_palette(DARK_COLORS);
        let data = fs::read_to_string(golden_path()).expect("golden file");
        let mut passed = 0usize;
        for line in data.lines() {
            let fixture: Fixture = serde_json::from_str(line).expect("fixture json");
            let mut component = build(&fixture.name);
            let rendered = component.render(fixture.width);
            assert_eq!(
                rendered, fixture.lines,
                "fixture {} (width {})",
                fixture.name, fixture.width
            );
            passed += 1;
        }
        eprintln!("tools golden passed: {passed} fixtures");
    }
}
