//! Golden tests for the slice-8 components (loader / working tips / diff
//! preview / image thumbnail fallback) against TS-captured byte-exact output
//! (`testdata/slice8-golden.jsonl`). Fixtures are captured from the real TS
//! components — see `.tmp/capture-slice8.vitest.ts` to regenerate.

#[cfg(test)]
mod tests {
    use crate::code_highlight::lang_from_path;
    use crate::component::Component;
    use crate::diff::{ClusteredDiffOptions, render_diff_lines, render_diff_lines_clustered};
    use crate::image::{ImageAttachment, ImageThumbnailComponent};
    use crate::loader::{MoonLoader, SpinnerStyle};
    use crate::theme::{ColorToken, DARK_COLORS, current_theme, set_palette};
    use crate::working_tips::current_working_tip;
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
            "{}/testdata/slice8-golden.jsonl",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    /// Build the rendered lines for a fixture name. Mirrors the TS capture
    /// setup exactly (same args, same construction order).
    fn render_fixture(name: &str, width: usize) -> Vec<String> {
        match name {
            "loader_moon" => {
                let mut l = MoonLoader::new(SpinnerStyle::Moon, None, "Downloading…");
                l.render(width)
            }
            "loader_braille" => {
                let mut l = MoonLoader::new(
                    SpinnerStyle::Braille,
                    Some(|s: &str| current_theme().fg(ColorToken::Primary, s)),
                    "Updating",
                );
                l.render(width)
            }
            "loader_tip_fits" => {
                let mut l = MoonLoader::new(SpinnerStyle::Moon, None, "Working");
                l.set_tip(" · Tip: /tasks to check progress");
                l.set_available_width(80);
                l.render(width)
            }
            "loader_tip_dropped" => {
                let mut l = MoonLoader::new(SpinnerStyle::Moon, None, "Working");
                l.set_tip(" · Tip: /tasks to check progress");
                l.set_available_width(5);
                l.render(width)
            }
            "working_tip_now1" => {
                vec![
                    current_working_tip(1_000_000)
                        .map(|t| t.text)
                        .unwrap_or("")
                        .to_owned(),
                ]
            }
            "working_tip_now2" => {
                vec![
                    current_working_tip(250_000)
                        .map(|t| t.text)
                        .unwrap_or("")
                        .to_owned(),
                ]
            }
            "diff_basic" => render_diff_lines("A\nB\nC\nD", "A\nB", "test.ts", false, 1, 1, None),
            "diff_pure_add" => render_diff_lines("", "A\nB\nC", "new.ts", false, 1, 1, None),
            "diff_pure_delete" => render_diff_lines("A\nB\nC", "", "gone.ts", false, 1, 1, None),
            "diff_incomplete" => {
                render_diff_lines("A\nB\nC\nD", "A\nB", "test.ts", true, 1, 1, None)
            }
            "diff_truncated" => render_diff_lines(
                "O1\nO2\nO3\nO4\nO5\nO6",
                "N1\nN2\nN3\nN4\nN5\nN6",
                "f.ts",
                false,
                1,
                1,
                Some(2),
            ),
            "diff_clustered_elide" => {
                let old = (1..=30)
                    .map(|i| format!("L{i}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut newv: Vec<String> = (1..=30).map(|i| format!("L{i}")).collect();
                newv[1] = "L2X".to_owned();
                newv[28] = "L29X".to_owned();
                let new = newv.join("\n");
                render_diff_lines_clustered(
                    &old,
                    &new,
                    "f.ts",
                    &ClusteredDiffOptions {
                        context_lines: Some(2),
                        ..Default::default()
                    },
                )
            }
            "diff_clustered_truncated" => {
                let mut newv: Vec<String> = (1..=50).map(|i| format!("L{i}")).collect();
                newv[1] = "L2X".to_owned();
                newv[20] = "L21X".to_owned();
                newv[40] = "L41X".to_owned();
                let old = (1..=50)
                    .map(|i| format!("L{i}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let new = newv.join("\n");
                render_diff_lines_clustered(
                    &old,
                    &new,
                    "f.ts",
                    &ClusteredDiffOptions {
                        context_lines: Some(2),
                        max_lines: Some(6),
                        ..Default::default()
                    },
                )
            }
            "image_fallback" | "image_fallback_narrow" => {
                let mut c = ImageThumbnailComponent::new(ImageAttachment {
                    placeholder: "[image #1 (800×600)]".to_owned(),
                    mime: "image/png".to_owned(),
                    width_px: 800,
                    height_px: 600,
                    bytes: vec![137, 80, 78, 71],
                });
                c.render(width)
            }
            other => panic!("unknown fixture {other}"),
        }
    }

    #[test]
    fn slice8_golden_byte_exact() {
        set_palette(DARK_COLORS);
        let data = fs::read_to_string(golden_path()).expect("golden file");
        let mut passed = 0usize;
        for line in data.lines() {
            let fixture: Fixture = serde_json::from_str(line).expect("fixture json");
            let rendered = render_fixture(&fixture.name, fixture.width);
            assert_eq!(
                rendered, fixture.lines,
                "fixture {} (width {})",
                fixture.name, fixture.width
            );
            passed += 1;
        }
        eprintln!("slice8 golden passed: {passed} fixtures");
        assert!(passed > 0, "no fixtures found in golden file");
    }

    #[test]
    fn code_highlight_lang_parity() {
        // lang_from_path is exercised beyond the golden set (not rendered).
        assert_eq!(lang_from_path("a.ts").as_deref(), Some("typescript"));
        assert_eq!(lang_from_path("a.rb").as_deref(), Some("ruby"));
        assert_eq!(lang_from_path("a.h").as_deref(), Some("c"));
    }
}
