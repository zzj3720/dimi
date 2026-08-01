import { FlatList, Pressable, StyleSheet, Text, View } from "react-native";

import { Ionicons } from "@expo/vector-icons";
import type { Session } from "@dimi-agent/protocol";

import type { MobileRuntimeState } from "./runtime";
import { colors, spacing } from "./theme";

interface SessionListScreenProps {
  readonly state: MobileRuntimeState;
  readonly onSelect: (sessionId: string) => void;
  readonly onRefresh: () => void;
  readonly onForget: () => void;
}

export function SessionListScreen({
  state,
  onSelect,
  onRefresh,
  onForget,
}: SessionListScreenProps) {
  return (
    <View style={styles.container}>
      <View style={styles.header}>
        <View style={styles.identity}>
          <Text style={styles.eyebrow}>RUNTIME</Text>
          <Text style={styles.title}>{state.remoteName ?? "k-3720"}</Text>
          <View style={styles.connection}>
            <View
              style={[
                styles.connectionDot,
                state.connection === "online" ? styles.onlineDot : styles.offlineDot,
              ]}
            />
            <Text style={styles.connectionText}>
              {state.connection === "online" ? "Online" : titleCase(state.connection)}
            </Text>
          </View>
        </View>
        <View style={styles.headerActions}>
          <Pressable
            accessibilityLabel="Refresh sessions"
            onPress={onRefresh}
            style={styles.headerButton}
          >
            <Ionicons name="refresh" size={20} color={colors.text} />
          </Pressable>
          <Pressable
            accessibilityLabel="Forget runtime"
            onPress={onForget}
            style={styles.headerButton}
          >
            <Ionicons name="unlink-outline" size={20} color={colors.text} />
          </Pressable>
        </View>
      </View>
      <FlatList
        contentContainerStyle={state.sessions.length === 0 ? styles.emptyList : undefined}
        data={state.sessions}
        keyExtractor={(session) => session.id}
        ListEmptyComponent={
          <View style={styles.empty}>
            <Ionicons name="file-tray-outline" size={27} color={colors.textMuted} />
            <Text style={styles.emptyTitle}>No sessions yet</Text>
            <Text style={styles.emptyBody}>
              Start a session on the runtime, then refresh this list.
            </Text>
          </View>
        }
        renderItem={({ item }) => (
          <SessionRow
            session={item}
            onPress={() => {
              onSelect(item.id);
            }}
          />
        )}
      />
      {state.error !== undefined ? <Text style={styles.error}>{state.error}</Text> : null}
    </View>
  );
}

function SessionRow({
  session,
  onPress,
}: {
  readonly session: Session;
  readonly onPress: () => void;
}) {
  const attention =
    session.pending_interaction !== undefined && session.pending_interaction !== "none";
  const status = attention ? "Needs input" : session.busy ? "Working" : undefined;
  return (
    <Pressable onPress={onPress} style={({ pressed }) => [styles.row, pressed && styles.pressed]}>
      <View style={styles.rowText}>
        <View style={styles.rowHeading}>
          <Text numberOfLines={1} style={styles.rowTitle}>
            {session.title || "Untitled session"}
          </Text>
          <Text style={styles.time}>{relativeTime(session.updated_at)}</Text>
        </View>
        <Text numberOfLines={2} style={styles.preview}>
          {session.last_prompt ?? session.metadata.cwd}
        </Text>
        {status !== undefined ? (
          <Text style={[styles.status, attention ? styles.attention : styles.working]}>
            {status}
          </Text>
        ) : null}
      </View>
      <Ionicons name="chevron-forward" size={17} color={colors.textMuted} />
    </Pressable>
  );
}

function relativeTime(value: string): string {
  const elapsed = Date.now() - new Date(value).getTime();
  if (elapsed < 60_000) return "Now";
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m`;
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)}h`;
  return `${Math.floor(elapsed / 86_400_000)}d`;
}

function titleCase(value: string): string {
  return `${value.slice(0, 1).toUpperCase()}${value.slice(1)}`;
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.background },
  header: {
    paddingHorizontal: spacing.lg,
    paddingTop: 14,
    paddingBottom: spacing.lg,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: colors.border,
  },
  identity: { flex: 1, minWidth: 0 },
  eyebrow: { color: colors.textMuted, fontSize: 11, fontWeight: "700", letterSpacing: 1.2 },
  title: { marginTop: 2, color: colors.text, fontSize: 22, fontWeight: "700", letterSpacing: -0.4 },
  headerActions: { flexDirection: "row", gap: spacing.xs },
  headerButton: { width: 44, height: 44, alignItems: "center", justifyContent: "center" },
  connection: {
    marginTop: spacing.sm,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  connectionDot: { width: 6, height: 6, borderRadius: 3 },
  onlineDot: { backgroundColor: colors.online },
  offlineDot: { backgroundColor: colors.textMuted },
  connectionText: { color: colors.textMuted, fontSize: 12, fontWeight: "600" },
  row: {
    minHeight: 96,
    paddingHorizontal: spacing.lg,
    paddingVertical: 14,
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.md,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: colors.border,
    backgroundColor: colors.background,
  },
  rowText: { flex: 1 },
  rowHeading: { flexDirection: "row", alignItems: "baseline", gap: spacing.md },
  rowTitle: { flex: 1, color: colors.text, fontSize: 15, fontWeight: "600" },
  preview: { marginTop: 5, color: colors.textMuted, fontSize: 13, lineHeight: 18 },
  time: { color: colors.textMuted, fontSize: 11 },
  status: { marginTop: 6, fontSize: 11, fontWeight: "700" },
  attention: { color: colors.warning },
  working: { color: colors.primary },
  pressed: { backgroundColor: colors.pressed },
  emptyList: { flexGrow: 1 },
  empty: { flex: 1, alignItems: "center", justifyContent: "center", paddingHorizontal: 48 },
  emptyTitle: { marginTop: spacing.md, color: colors.text, fontSize: 18, fontWeight: "600" },
  emptyBody: {
    marginTop: spacing.sm,
    color: colors.textMuted,
    textAlign: "center",
    lineHeight: 21,
  },
  error: { padding: spacing.md, color: colors.danger, fontSize: 13 },
});
