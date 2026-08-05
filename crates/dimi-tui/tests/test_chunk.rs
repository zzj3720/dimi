#[test]
fn process_stdin_chunk_forwards_plain_text_after_start() {
    use dimi_tui::process_terminal::ProcessTerminal;
    use dimi_tui::terminal::Terminal;
    let mut t = ProcessTerminal::new();
    let mut start_cb = |_data: &str| {};
    let mut on_resize = || {};
    t.start(&mut start_cb, &mut on_resize);
    let mut got = Vec::new();
    let mut on_input = |data: &str| got.push(data.to_owned());
    t.process_stdin_chunk("hello", &mut on_input);
    assert!(
        !got.is_empty(),
        "plain text should be forwarded after start, got {got:?}"
    );
}
