//! Golden tests for transcript message components against TS-captured
//! byte-exact output (`testdata/components-messages-golden.jsonl`).

#[cfg(test)]
mod tests {
    use crate::component::Component;
    use crate::components::messages::assistant_message::AssistantMessageComponent;
    use crate::components::messages::compaction::CompactionComponent;
    use crate::components::messages::status_message::{
        NoticeMessageComponent, StatusMessageComponent,
    };
    use crate::components::messages::thinking::{ThinkingComponent, ThinkingMode};
    use crate::components::messages::user_message::UserMessageComponent;
    use crate::theme::{DARK_COLORS, set_palette};
    use serde::Deserialize;
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
            "{}/testdata/components-messages-golden.jsonl",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    fn build(name: &str) -> Box<dyn Component> {
        match name {
            "user_simple" => Box::new(UserMessageComponent::new("Hello there!", None)),
            "user_wrap" => Box::new(UserMessageComponent::new(
                "A much longer user message that will wrap across multiple lines at a narrow width",
                None,
            )),
            "user_no_bullet" => {
                Box::new(UserMessageComponent::new("no bullet", Some(String::new())))
            }
            "user_custom_bullet" => Box::new(UserMessageComponent::new(
                "custom bullet",
                Some("> ".to_owned()),
            )),
            "assistant_simple" => {
                let mut c = AssistantMessageComponent::new(true);
                c.update_content("Hello **world**!");
                Box::new(c)
            }
            "assistant_markdown" => {
                let mut c = AssistantMessageComponent::new(true);
                c.update_content(
                    "# Heading\n\nSome *intro* with `code` and a [link](https://example.com).",
                );
                Box::new(c)
            }
            "assistant_empty" => {
                let mut c = AssistantMessageComponent::new(true);
                c.update_content("   ");
                Box::new(c)
            }
            "assistant_no_bullet" => {
                let mut c = AssistantMessageComponent::new(false);
                c.update_content("No bullet variant");
                Box::new(c)
            }
            "assistant_wrap" => {
                let mut c = AssistantMessageComponent::new(true);
                c.update_content(
                    "wrap me please wrap me please wrap me please wrap me please wrap me",
                );
                Box::new(c)
            }
            "thinking_finalized" => Box::new(ThinkingComponent::new(
                "Let me think about this carefully.",
                true,
                ThinkingMode::Finalized,
            )),
            "thinking_collapsed" => Box::new(ThinkingComponent::new(
                "line1\nline2\nline3\nline4\nline5",
                true,
                ThinkingMode::Finalized,
            )),
            "thinking_expanded" => {
                let mut c = ThinkingComponent::new(
                    "line1\nline2\nline3\nline4\nline5",
                    true,
                    ThinkingMode::Finalized,
                );
                c.set_expanded(true);
                Box::new(c)
            }
            "thinking_hidden" => {
                let mut c = ThinkingComponent::new("short", true, ThinkingMode::Finalized);
                c.set_hidden(true);
                Box::new(c)
            }
            "thinking_no_marker" => Box::new(ThinkingComponent::new(
                "Let me think.",
                false,
                ThinkingMode::Finalized,
            )),
            "thinking_live" => Box::new(ThinkingComponent::new(
                "Let me think.",
                true,
                ThinkingMode::Live,
            )),
            "status_plain" => Box::new(StatusMessageComponent::new("Session resumed", None)),
            "status_error" => Box::new(StatusMessageComponent::new(
                "Something failed",
                Some(crate::theme::ColorToken::Error),
            )),
            "status_multiline" => Box::new(StatusMessageComponent::new(
                "line1\nline2 with \r carriage",
                Some(crate::theme::ColorToken::Warning),
            )),
            "notice_full" => Box::new(NoticeMessageComponent::new(
                "Title here",
                Some("Detail line here"),
            )),
            "notice_title_only" => Box::new(NoticeMessageComponent::new("Title only", None)),
            "compaction_running" => {
                Box::new(CompactionComponent::new(Some("Please keep the summary")))
            }
            "compaction_done" => {
                let mut c = CompactionComponent::new(Some("Please keep the summary"));
                c.mark_done(Some(12000), Some(3000), Some("Summarized the conversation"));
                Box::new(c)
            }
            "compaction_done_no_summary" => {
                let mut c = CompactionComponent::new(None);
                c.mark_done(Some(12000), Some(3000), None);
                Box::new(c)
            }
            "compaction_cancelled" => {
                let mut c = CompactionComponent::new(Some("Please keep the summary"));
                c.mark_canceled();
                Box::new(c)
            }
            "compaction_done_expanded" => {
                let mut c = CompactionComponent::new(Some("Please keep the summary"));
                c.mark_done(
                    Some(12000),
                    Some(3000),
                    Some("A longer summary that explains what happened across multiple lines\nwith a second line here"),
                );
                c.set_expanded(true);
                Box::new(c)
            }
            other => panic!("unknown fixture {other}"),
        }
    }

    #[test]
    fn messages_golden_byte_exact() {
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
        eprintln!("messages golden passed: {passed} fixtures");
    }
}
