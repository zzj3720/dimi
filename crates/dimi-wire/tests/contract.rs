//! Contract semantics: id rules, optional/null handling, enum tags,
//! unknown-field stripping, and byte-exact serialization of constructed
//! values. Byte-level round-trips of full documents live in `fixtures.rs`.

use dimi_wire::*;
use serde_json::json;

fn parse<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(s)
}

// ---------------------------------------------------------------- id rules

#[test]
fn is_plain_agent_id_accepts_plain_names() {
    for id in [
        "a",
        "abc",
        "a.b-c_1",
        "0123456789",
        "A-Z_9.x-y",
        &"x".repeat(128),
    ] {
        assert!(is_plain_agent_id(id), "should accept {id:?}");
    }
}

#[test]
fn is_plain_agent_id_rejects_path_hostile_or_empty() {
    for id in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "a b",
        "a\u{0}b",
        &"x".repeat(129),
    ] {
        assert!(!is_plain_agent_id(id), "should reject {id:?}");
    }
}

#[test]
fn empty_ids_are_rejected_on_parse() {
    let line = r#"{"kind":"turn","turnId":"","ordinal":0,"state":"completed","origin":{"kind":"user"},"steps":[]}"#;
    assert!(parse::<Item>(line).is_err());
    let line = r#"{"taskId":"","kind":"shell","state":"running","detached":false,"outputTail":""}"#;
    assert!(parse::<Task>(line).is_err());
}

// ---------------------------------------------------------------- optionals

#[test]
fn null_in_optional_field_is_rejected_like_zod() {
    let line = r#"{"kind":"turn","turnId":"t_1","ordinal":0,"state":"completed","origin":{"kind":"user"},"prompt":null,"steps":[]}"#;
    let err = parse::<Item>(line).unwrap_err();
    assert!(err.to_string().contains("null"), "{err}");

    let line = r#"{"kind":"turn","turnId":"t_1","ordinal":0,"state":"completed","origin":{"kind":"user"},"steps":[],"durationMs":null}"#;
    assert!(parse::<Item>(line).is_err());
}

#[test]
fn null_in_nested_optional_is_rejected() {
    let line = r#"{"kind":"tool","frameId":"f_1","toolCallId":"c_1","name":"x","state":"running","progress":null}"#;
    assert!(parse::<Frame>(line).is_err());

    let line = r#"{"kind":"retrying","turnId":3,"step":1,"stepId":"s_9","failedAttempt":1,"nextAttempt":2,"maxAttempts":3,"delayMs":500,"errorName":null,"since":1}"#;
    assert!(parse::<AgentPhase>(line).is_err());
}

#[test]
fn missing_required_field_is_rejected() {
    // detached and outputTail are required by transcriptTaskSchema.
    let line = r#"{"taskId":"t","kind":"shell","state":"running"}"#;
    assert!(parse::<Task>(line).is_err());
}

// ---------------------------------------------------------------- tags

#[test]
fn unknown_kind_is_rejected() {
    assert!(parse::<Item>(r#"{"kind":"bogus","turnId":"t_1"}"#).is_err());
    assert!(parse::<Frame>(r#"{"kind":"bogus","frameId":"f_1"}"#).is_err());
    assert!(parse::<AgentPhase>(r#"{"kind":"bogus"}"#).is_err());
    assert!(parse::<TurnOrigin>(r#"{"kind":"bogus"}"#).is_err());
}

#[test]
fn unknown_fields_are_stripped_like_zod() {
    // zod's default object behavior is `strip`; serde ignores unknown fields.
    let line = r#"{"kind":"marker","markerId":"m_1","marker":"x","bogus":1,"nested":{"a":1}}"#;
    let item: Item = parse(line).unwrap();
    match item {
        Item::Marker { marker, .. } => assert_eq!(marker, "x"),
        other => panic!("expected marker, got {other:?}"),
    }
}

#[test]
fn task_states_and_kinds_roundtrip() {
    for state in [
        "running",
        "completed",
        "failed",
        "timed_out",
        "killed",
        "lost",
    ] {
        let v = json!({"taskId":"task_1","kind":"shell","state":state,"detached":false,"outputTail":""});
        let task: Task = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(&task).unwrap(), v);
    }
    for kind in ["shell", "subagent", "tool", "other"] {
        let v = json!({"taskId":"task_1","kind":kind,"state":"running","detached":false,"outputTail":""});
        let task: Task = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(&task).unwrap(), v);
    }
}

#[test]
fn origin_variants_roundtrip() {
    for kind in ["compaction", "side", "other"] {
        let o: TurnOrigin = parse(&format!(r#"{{"kind":"{kind}"}}"#)).unwrap();
        assert_eq!(serde_json::to_value(&o).unwrap(), json!({"kind": kind}));
    }
    let o: TurnOrigin = parse(r#"{"kind":"task","taskId":"task_1","payload":{"p":1}}"#).unwrap();
    assert!(matches!(o, TurnOrigin::Task { .. }));
    // cron's taskId is optional.
    let o: TurnOrigin = parse(r#"{"kind":"cron"}"#).unwrap();
    assert!(matches!(o, TurnOrigin::Cron { task_id: None, .. }));
}

#[test]
fn frame_variants_parse() {
    let frame: Frame =
        parse(r#"{"kind":"text","frameId":"f_1","role":"user","text":"hi"}"#).unwrap();
    assert!(matches!(
        frame,
        Frame::Text {
            role: TextRole::User,
            ..
        }
    ));
    let frame: Frame =
        parse(r#"{"kind":"notice","frameId":"f_2","level":"warning","message":"slow"}"#).unwrap();
    assert!(matches!(
        frame,
        Frame::Notice {
            level: NoticeLevel::Warning,
            ..
        }
    ));
    let frame: Frame = parse(r#"{"kind":"thinking","frameId":"f_3","text":"hmm"}"#).unwrap();
    assert!(matches!(frame, Frame::Thinking { .. }));
}

// ---------------------------------------------------------------- bytes

#[test]
fn progress_percent_stays_integer_on_reserialize() {
    let line = r#"{"kind":"tool","frameId":"f_1","toolCallId":"c_1","name":"x","state":"running","progress":{"kind":"custom","percent":45,"customKind":"spinner","customData":{"spin":1}}}"#;
    let frame: Frame = parse(line).unwrap();
    let re = serde_json::to_string(&frame).unwrap();
    assert_eq!(re, line, "percent must not gain a .0");
}

#[test]
fn minimal_turn_serializes_without_optional_fields() {
    let item = Item::Turn {
        turn_id: "t_1".parse().unwrap(),
        ordinal: 0,
        state: TurnState::Queued,
        origin: TurnOrigin::User { payload: None },
        prompt: None,
        attachment_ids: None,
        steps: Vec::new(),
        started_at: None,
        ended_at: None,
        usage: None,
        duration_ms: None,
        error: None,
    };
    assert_eq!(
        serde_json::to_string(&item).unwrap(),
        r#"{"kind":"turn","turnId":"t_1","ordinal":0,"state":"queued","origin":{"kind":"user"},"steps":[]}"#
    );
}

#[test]
fn transcript_usage_serializes_cache_write_in_camel_case() {
    let usage = TranscriptUsage {
        input_tokens: Some(100),
        output_tokens: Some(5),
        cached_tokens: Some(20),
        cache_write_tokens: Some(7),
        cost: None,
    };
    assert_eq!(
        serde_json::to_string(&usage).unwrap(),
        r#"{"inputTokens":100,"outputTokens":5,"cachedTokens":20,"cacheWriteTokens":7}"#
    );
}

#[test]
fn phase_retrying_serializes_camel_case() {
    let phase = AgentPhase::Retrying {
        turn_id: 3,
        step: 1,
        step_id: "s_9".into(),
        failed_attempt: 1,
        next_attempt: 2,
        max_attempts: 3,
        delay_ms: 500,
        error_name: Some("TimeoutError".into()),
        status_code: Some(408),
        since: 1_750_000_000_002,
    };
    assert_eq!(
        serde_json::to_string(&phase).unwrap(),
        r#"{"kind":"retrying","turnId":3,"step":1,"stepId":"s_9","failedAttempt":1,"nextAttempt":2,"maxAttempts":3,"delayMs":500,"errorName":"TimeoutError","statusCode":408,"since":1750000000002}"#
    );
}

#[test]
fn multi_key_open_envelope_preserves_order() {
    let line = r#"{"kind":"tool","frameId":"f_1","toolCallId":"c_1","name":"x","state":"running","input":{"b":2,"a":1,"c":{"z":1,"y":2}}}"#;
    let frame: Frame = parse(line).unwrap();
    assert_eq!(serde_json::to_string(&frame).unwrap(), line);
}
