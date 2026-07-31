import { useEffect, useMemo, useRef, useState } from "react";
import {
  FlatList,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { Ionicons } from "@expo/vector-icons";
import type {
  ApprovalRequest,
  ApprovalResponse,
  QuestionAnswer,
  QuestionRequest,
} from "@moonshot-ai/protocol";
import type { ToolCallFrame } from "@moonshot-ai/transcript";

import { MarkdownMessage } from "./MarkdownMessage";
import type { MobileRuntime, MobileRuntimeState } from "./runtime";
import { buildTimeline, summarizeTools, type TimelineEntry } from "./timeline";
import { colors, spacing } from "./theme";

interface ChatScreenProps {
  readonly runtime: MobileRuntime;
  readonly state: MobileRuntimeState;
  readonly onBack: () => void;
  readonly onOpenCode: (code: string, language?: string) => void;
}

export function ChatScreen({ runtime, state, onBack, onOpenCode }: ChatScreenProps) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const list = useRef<FlatList<TimelineEntry>>(null);
  const timeline = useMemo(() => buildTimeline(state.transcript), [state.transcript]);
  const session = state.sessions.find((entry) => entry.id === state.selectedSessionId);
  const approval = state.approvals[0];
  const question = state.questions[0];
  const hasInteraction = approval !== undefined || question !== undefined;

  useEffect(() => {
    if (timeline.length > 0)
      requestAnimationFrame(() => list.current?.scrollToEnd({ animated: true }));
  }, [timeline.length]);

  const submit = async (steer: boolean): Promise<void> => {
    const value = text.trim();
    if (value.length === 0 || sending) return;
    setSending(true);
    try {
      if (steer) await runtime.steerPrompt(value);
      else await runtime.sendPrompt(value);
      setText("");
    } catch {
      // MobileRuntime exposes the request error in shared screen state.
    } finally {
      setSending(false);
    }
  };

  return (
    <KeyboardAvoidingView
      behavior={Platform.OS === "ios" ? "padding" : "height"}
      keyboardVerticalOffset={0}
      style={styles.container}
    >
      <View style={styles.header}>
        <Pressable
          accessibilityLabel="Back to sessions"
          onPress={onBack}
          style={styles.headerButton}
        >
          <Ionicons name="chevron-back" size={23} color={colors.text} />
        </Pressable>
        <View style={styles.headerTitle}>
          <Text numberOfLines={1} style={styles.title}>
            {session?.title ?? "Session"}
          </Text>
          <Text style={styles.subtitle}>
            {state.connection === "online" ? "Online" : "Offline"}
            {hasInteraction ? " · Needs input" : session?.busy === true ? " · Working" : ""}
          </Text>
        </View>
        {session?.busy === true ? (
          <Pressable
            accessibilityLabel="Cancel current run"
            onPress={() => void runtime.cancel()}
            style={styles.headerButton}
          >
            <Ionicons name="stop-circle-outline" size={23} color={colors.danger} />
          </Pressable>
        ) : (
          <View style={styles.headerButton} />
        )}
      </View>
      {hasInteraction ? (
        <ScrollView
          contentContainerStyle={styles.interactionStage}
          keyboardShouldPersistTaps="handled"
        >
          {approval !== undefined ? (
            <ApprovalPanel
              request={approval}
              onRespond={(response) => void runtime.respondApproval(approval.approval_id, response)}
            />
          ) : null}
          {question !== undefined ? (
            <QuestionPanel
              request={question}
              onDismiss={() => void runtime.dismissQuestion(question.question_id)}
              onSubmit={(answers) => void runtime.respondQuestion(question.question_id, answers)}
            />
          ) : null}
        </ScrollView>
      ) : (
        <FlatList
          ref={list}
          contentContainerStyle={styles.timeline}
          data={timeline}
          keyExtractor={(entry) => entry.id}
          keyboardDismissMode="interactive"
          renderItem={({ item }) => <TimelineRow entry={item} onOpenCode={onOpenCode} />}
          ListFooterComponent={
            state.activity !== undefined ? (
              <View style={styles.activity}>
                <View style={styles.activityDot} />
                <Text style={styles.activityText}>{state.activity}</Text>
              </View>
            ) : null
          }
        />
      )}
      {state.error !== undefined ? <Text style={styles.error}>{state.error}</Text> : null}
      {!hasInteraction ? (
        <View style={styles.composer}>
          <TextInput
            multiline
            onChangeText={setText}
            placeholder={session?.busy === true ? "Queue a message…" : "Message…"}
            placeholderTextColor={colors.textMuted}
            style={styles.input}
            value={text}
          />
          {session?.busy === true && text.trim().length > 0 ? (
            <Pressable
              accessibilityLabel="Steer current run"
              onPress={() => void submit(true)}
              style={styles.steer}
            >
              <Text style={styles.steerText}>Steer</Text>
            </Pressable>
          ) : null}
          <Pressable
            accessibilityLabel="Send message"
            disabled={text.trim().length === 0 || sending || state.connection !== "online"}
            onPress={() => void submit(false)}
            style={[
              styles.send,
              (text.trim().length === 0 || sending || state.connection !== "online") &&
                styles.disabled,
            ]}
          >
            <Ionicons name="arrow-up" size={19} color={colors.onPrimary} />
          </Pressable>
        </View>
      ) : null}
    </KeyboardAvoidingView>
  );
}

function TimelineRow({
  entry,
  onOpenCode,
}: {
  readonly entry: TimelineEntry;
  readonly onOpenCode: (code: string, language?: string) => void;
}) {
  if (entry.kind === "tools") return <ToolGroup tools={entry.tools} />;
  if (entry.kind === "notice") {
    return (
      <View style={styles.notice}>
        <Ionicons name="alert-circle-outline" size={17} color={colors.warning} />
        <Text style={styles.noticeText}>{entry.text}</Text>
      </View>
    );
  }
  if (entry.kind === "user") {
    return (
      <View style={styles.userRow}>
        <Text style={styles.userText}>{entry.text}</Text>
      </View>
    );
  }
  return (
    <View style={styles.assistantRow}>
      <MarkdownMessage value={entry.text} onOpenCode={onOpenCode} />
    </View>
  );
}

function ToolGroup({ tools }: { readonly tools: readonly ToolCallFrame[] }) {
  const [expanded, setExpanded] = useState(false);
  return (
    <View style={styles.toolGroup}>
      <Pressable
        onPress={() => {
          setExpanded((value) => !value);
        }}
        style={styles.toolSummary}
      >
        <Ionicons name="construct-outline" size={16} color={colors.textMuted} />
        <Text style={styles.toolSummaryText}>{summarizeTools(tools)}</Text>
        <Ionicons
          name={expanded ? "chevron-up" : "chevron-down"}
          size={15}
          color={colors.textMuted}
        />
      </Pressable>
      {expanded ? (
        <View style={styles.toolDetails}>
          {tools.map((tool) => (
            <View key={tool.toolCallId} style={styles.toolDetail}>
              <Text style={styles.toolName}>{tool.name}</Text>
              <Text style={tool.state === "error" ? styles.toolError : styles.toolState}>
                {tool.state}
              </Text>
            </View>
          ))}
        </View>
      ) : null}
    </View>
  );
}

function ApprovalPanel({
  request,
  onRespond,
}: {
  readonly request: ApprovalRequest;
  readonly onRespond: (response: ApprovalResponse) => void;
}) {
  const [feedback, setFeedback] = useState("");
  const respond = (decision: ApprovalResponse["decision"], scope?: "session"): void => {
    const note = feedback.trim();
    onRespond({
      decision,
      scope,
      feedback: note.length === 0 ? undefined : note,
    });
  };
  return (
    <View style={styles.interaction}>
      <Text style={styles.interactionLabel}>APPROVAL</Text>
      <Text style={styles.interactionTitle}>{request.tool_name}</Text>
      <Text style={styles.interactionBody}>{request.action}</Text>
      <ScrollView style={styles.approvalPreview}>
        <Text selectable style={styles.approvalPreviewText}>
          {formatApprovalDisplay(request.tool_input_display)}
        </Text>
      </ScrollView>
      <TextInput
        onChangeText={setFeedback}
        placeholder="Optional feedback"
        placeholderTextColor={colors.textMuted}
        style={styles.feedbackInput}
        value={feedback}
      />
      <View style={styles.interactionActions}>
        <Pressable
          onPress={() => {
            respond("rejected");
          }}
          style={styles.rejectButton}
        >
          <Text style={styles.rejectText}>Reject</Text>
        </Pressable>
        <Pressable
          onPress={() => {
            respond("approved");
          }}
          style={styles.approveButton}
        >
          <Text style={styles.approveText}>Approve once</Text>
        </Pressable>
      </View>
      <Pressable
        onPress={() => {
          respond("approved", "session");
        }}
        style={styles.sessionApprove}
      >
        <Text style={styles.sessionApproveText}>Approve for this session</Text>
      </Pressable>
    </View>
  );
}

interface QuestionDraft {
  readonly optionIds: readonly string[];
  readonly otherText: string;
  readonly useOther: boolean;
}

function QuestionPanel({
  request,
  onDismiss,
  onSubmit,
}: {
  readonly request: QuestionRequest;
  readonly onDismiss: () => void;
  readonly onSubmit: (answers: Record<string, QuestionAnswer>) => void;
}) {
  const [drafts, setDrafts] = useState<Record<string, QuestionDraft>>({});
  useEffect(() => {
    setDrafts({});
  }, [request.question_id]);
  const complete = request.questions.every((question) => {
    const draft = drafts[question.id];
    return draft?.useOther === true
      ? draft.otherText.trim().length > 0
      : (draft?.optionIds.length ?? 0) > 0;
  });
  return (
    <View style={styles.interaction}>
      <Text style={styles.interactionLabel}>QUESTION</Text>
      {request.questions.map((question) => (
        <View key={question.id} style={styles.question}>
          <Text style={styles.interactionTitle}>{question.header ?? question.question}</Text>
          {question.header !== undefined ? (
            <Text style={styles.interactionBody}>{question.question}</Text>
          ) : null}
          <View style={styles.options}>
            {question.options.map((option) => {
              const draft = drafts[question.id];
              const selected = draft?.optionIds.includes(option.id) === true;
              return (
                <Pressable
                  key={option.id}
                  onPress={() => {
                    setDrafts((current) => {
                      const previous = current[question.id] ?? {
                        optionIds: [],
                        otherText: "",
                        useOther: false,
                      };
                      const optionIds =
                        question.multi_select === true
                          ? selected
                            ? previous.optionIds.filter((id) => id !== option.id)
                            : [...previous.optionIds, option.id]
                          : [option.id];
                      return {
                        ...current,
                        [question.id]: { ...previous, optionIds, useOther: false },
                      };
                    });
                  }}
                  style={[styles.option, selected && styles.optionSelected]}
                >
                  <Ionicons
                    name={selected ? "radio-button-on" : "radio-button-off"}
                    size={17}
                    color={selected ? colors.primary : colors.textMuted}
                  />
                  <View style={styles.optionCopy}>
                    <Text style={styles.optionText}>{option.label}</Text>
                    {option.description !== undefined ? (
                      <Text style={styles.optionDescription}>{option.description}</Text>
                    ) : null}
                  </View>
                </Pressable>
              );
            })}
            {question.allow_other === true ? (
              <>
                <Pressable
                  onPress={() => {
                    setDrafts((current) => {
                      const previous = current[question.id] ?? {
                        optionIds: [],
                        otherText: "",
                        useOther: false,
                      };
                      return {
                        ...current,
                        [question.id]: { ...previous, useOther: true },
                      };
                    });
                  }}
                  style={[
                    styles.option,
                    drafts[question.id]?.useOther === true && styles.optionSelected,
                  ]}
                >
                  <Ionicons
                    name={
                      drafts[question.id]?.useOther === true
                        ? "radio-button-on"
                        : "radio-button-off"
                    }
                    size={17}
                    color={
                      drafts[question.id]?.useOther === true
                        ? colors.primary
                        : colors.textMuted
                    }
                  />
                  <Text style={styles.optionText}>{question.other_label ?? "Other"}</Text>
                </Pressable>
                {drafts[question.id]?.useOther === true ? (
                  <TextInput
                    autoFocus
                    multiline
                    onChangeText={(otherText) => {
                      setDrafts((current) => ({
                        ...current,
                        [question.id]: {
                          optionIds: current[question.id]?.optionIds ?? [],
                          otherText,
                          useOther: true,
                        },
                      }));
                    }}
                    placeholder={question.other_description ?? "Type your answer"}
                    placeholderTextColor={colors.textMuted}
                    style={styles.otherInput}
                    value={drafts[question.id]?.otherText ?? ""}
                  />
                ) : null}
              </>
            ) : null}
          </View>
        </View>
      ))}
      <View style={styles.interactionActions}>
        <Pressable onPress={onDismiss} style={styles.rejectButton}>
          <Text style={styles.rejectText}>Dismiss</Text>
        </Pressable>
        <Pressable
          disabled={!complete}
          onPress={() => {
            onSubmit(
              Object.fromEntries(
                request.questions.map((question) => [
                  question.id,
                  toQuestionAnswer(question.multi_select === true, drafts[question.id]!),
                ]),
              ),
            );
          }}
          style={[styles.approveButton, !complete && styles.disabled]}
        >
          <Text style={styles.approveText}>Submit</Text>
        </Pressable>
      </View>
    </View>
  );
}

function toQuestionAnswer(multi: boolean, draft: QuestionDraft): QuestionAnswer {
  const otherText = draft.otherText.trim();
  if (draft.useOther) {
    return multi && draft.optionIds.length > 0
      ? { kind: "multi_with_other", option_ids: [...draft.optionIds], other_text: otherText }
      : { kind: "other", text: otherText };
  }
  return multi
    ? { kind: "multi", option_ids: [...draft.optionIds] }
    : { kind: "single", option_id: draft.optionIds[0]! };
}

function formatApprovalDisplay(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value !== "object" || value === null) return String(value);
  const display = value as Record<string, unknown>;
  for (const key of ["command", "diff", "content", "summary"]) {
    if (typeof display[key] === "string") return display[key].slice(0, 4_000);
  }
  return (JSON.stringify(value, null, 2) ?? "Unsupported approval input").slice(0, 4_000);
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.background },
  header: {
    minHeight: 62,
    paddingHorizontal: spacing.sm,
    flexDirection: "row",
    alignItems: "center",
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: colors.border,
  },
  headerButton: { width: 44, height: 44, alignItems: "center", justifyContent: "center" },
  headerTitle: { flex: 1, alignItems: "center" },
  title: { maxWidth: "100%", color: colors.text, fontSize: 16, fontWeight: "600" },
  subtitle: { marginTop: 2, color: colors.textMuted, fontSize: 11 },
  timeline: { paddingHorizontal: spacing.lg, paddingTop: spacing.lg, paddingBottom: spacing.xl },
  interactionStage: {
    flexGrow: 1,
    paddingHorizontal: spacing.xl,
    paddingTop: spacing.xl,
    paddingBottom: spacing.xl * 2,
  },
  userRow: {
    alignSelf: "flex-end",
    maxWidth: "84%",
    marginBottom: spacing.lg,
    paddingHorizontal: spacing.md,
    paddingVertical: 10,
    backgroundColor: colors.primaryMuted,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: "#C2D9E8",
  },
  userText: { color: colors.text, fontSize: 16, lineHeight: 22 },
  assistantRow: { marginBottom: spacing.md },
  toolGroup: {
    marginBottom: spacing.lg,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderColor: colors.border,
  },
  toolSummary: { minHeight: 40, flexDirection: "row", alignItems: "center", gap: spacing.sm },
  toolSummaryText: { flex: 1, color: colors.textMuted, fontSize: 13, fontWeight: "600" },
  toolDetails: { paddingBottom: spacing.sm },
  toolDetail: {
    minHeight: 30,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  toolName: { color: colors.text, fontFamily: "Menlo", fontSize: 12 },
  toolState: { color: colors.textMuted, fontSize: 12 },
  toolError: { color: colors.danger, fontSize: 12 },
  notice: {
    marginBottom: spacing.lg,
    padding: spacing.md,
    flexDirection: "row",
    gap: spacing.sm,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.border,
  },
  noticeText: { flex: 1, color: colors.textMuted, fontSize: 14, lineHeight: 20 },
  activity: { minHeight: 42, flexDirection: "row", alignItems: "center", gap: spacing.sm },
  activityDot: { width: 7, height: 7, borderRadius: 4, backgroundColor: colors.primary },
  activityText: { color: colors.primary, fontSize: 14, fontWeight: "600" },
  interaction: {
    width: "100%",
  },
  interactionLabel: {
    color: colors.textMuted,
    fontSize: 11,
    fontWeight: "700",
    letterSpacing: 1.1,
  },
  interactionTitle: { marginTop: spacing.sm, color: colors.text, fontSize: 20, fontWeight: "600" },
  interactionBody: { marginTop: spacing.sm, color: colors.textMuted, fontSize: 15, lineHeight: 22 },
  approvalPreview: {
    maxHeight: 180,
    marginTop: spacing.lg,
    padding: spacing.md,
    backgroundColor: colors.code,
  },
  approvalPreviewText: {
    color: colors.text,
    fontFamily: "Menlo",
    fontSize: 12,
    lineHeight: 18,
  },
  feedbackInput: {
    minHeight: 44,
    marginTop: spacing.md,
    paddingHorizontal: spacing.md,
    color: colors.text,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.borderStrong,
  },
  interactionActions: { flexDirection: "row", gap: spacing.sm, marginTop: spacing.xl },
  rejectButton: {
    flex: 1,
    height: 48,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.pressed,
  },
  rejectText: { color: colors.text, fontSize: 14, fontWeight: "600" },
  approveButton: {
    flex: 1,
    minHeight: 48,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.primary,
  },
  approveText: { color: colors.onPrimary, fontSize: 14, fontWeight: "600" },
  sessionApprove: {
    minHeight: 44,
    marginTop: spacing.sm,
    alignItems: "center",
    justifyContent: "center",
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.primary,
  },
  sessionApproveText: { color: colors.primary, fontSize: 14, fontWeight: "600" },
  question: { marginTop: spacing.xl },
  options: {
    marginTop: spacing.md,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: colors.border,
  },
  option: {
    minHeight: 52,
    paddingVertical: spacing.sm,
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: colors.border,
  },
  optionSelected: { backgroundColor: colors.primaryMuted },
  optionCopy: { flex: 1 },
  optionText: { color: colors.text, fontSize: 14, fontWeight: "600" },
  optionDescription: { marginTop: 2, color: colors.textMuted, fontSize: 12, lineHeight: 17 },
  otherInput: {
    minHeight: 72,
    marginTop: spacing.md,
    padding: spacing.md,
    color: colors.text,
    textAlignVertical: "top",
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.borderStrong,
  },
  error: {
    paddingHorizontal: spacing.lg,
    paddingBottom: spacing.sm,
    color: colors.danger,
    fontSize: 13,
  },
  composer: {
    minHeight: 62,
    paddingHorizontal: spacing.md,
    paddingVertical: spacing.sm,
    flexDirection: "row",
    alignItems: "flex-end",
    gap: spacing.sm,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: colors.borderStrong,
    backgroundColor: colors.surface,
  },
  input: {
    flex: 1,
    maxHeight: 120,
    minHeight: 44,
    paddingHorizontal: spacing.md,
    paddingVertical: 11,
    color: colors.text,
    fontSize: 16,
    backgroundColor: colors.background,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.border,
  },
  steer: { height: 44, paddingHorizontal: 10, alignItems: "center", justifyContent: "center" },
  steerText: { color: colors.primary, fontSize: 13, fontWeight: "700" },
  send: {
    width: 44,
    height: 44,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.primary,
  },
  disabled: { opacity: 0.35 },
});
