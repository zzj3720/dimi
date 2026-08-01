import { useEffect, useMemo, useState } from "react";
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from "react-native";

import { createAndroidUpdatePlatform } from "./androidUpdate";
import { colors, spacing } from "./theme";
import { UpdateController, type UpdateState } from "./update";

export function UpdateBanner() {
  const controller = useMemo(() => {
    const platform = createAndroidUpdatePlatform();
    return platform === undefined ? undefined : new UpdateController(platform);
  }, []);
  const [state, setState] = useState<UpdateState>({ phase: "idle" });

  useEffect(() => {
    if (controller === undefined) return;
    const unsubscribe = controller.subscribe(setState);
    void controller.check();
    return () => {
      unsubscribe();
      controller.dispose();
    };
  }, [controller]);

  if (controller === undefined || state.phase === "idle" || state.phase === "checking") return null;

  const busy = state.phase === "downloading" || state.phase === "installing";
  const title =
    state.phase === "downloading"
      ? `Downloading ${state.manifest.version}`
      : state.phase === "installing"
        ? "Opening Android installer"
        : state.phase === "ready"
          ? `${state.manifest.version} is ready`
          : "Update unavailable";

  return (
    <View accessibilityRole="summary" style={styles.banner}>
      <View style={styles.copy}>
        <Text style={styles.title}>{title}</Text>
        {state.phase === "error" && <Text style={styles.detail}>{state.message}</Text>}
      </View>
      {busy && <ActivityIndicator color={colors.primary} />}
      {state.phase === "ready" && (
        <Pressable accessibilityRole="button" onPress={() => void controller.install()} style={styles.action}>
          <Text style={styles.actionText}>Install</Text>
        </Pressable>
      )}
      {state.phase === "error" && (
        <Pressable accessibilityRole="button" onPress={() => void controller.check()} style={styles.action}>
          <Text style={styles.actionText}>Retry</Text>
        </Pressable>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  banner: {
    minHeight: 52,
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.md,
    paddingHorizontal: spacing.lg,
    paddingVertical: spacing.sm,
    borderBottomWidth: 1,
    borderBottomColor: colors.borderStrong,
    backgroundColor: colors.primaryMuted,
  },
  copy: { flex: 1 },
  title: { color: colors.text, fontWeight: "700" },
  detail: { marginTop: spacing.xs, color: colors.danger, fontSize: 12 },
  action: {
    minHeight: 36,
    justifyContent: "center",
    paddingHorizontal: spacing.md,
    borderRadius: 6,
    backgroundColor: colors.primary,
  },
  actionText: { color: colors.onPrimary, fontWeight: "700" },
});
