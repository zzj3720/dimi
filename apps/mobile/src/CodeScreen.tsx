import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";

import { Ionicons } from "@expo/vector-icons";

import { colors, spacing } from "./theme";

interface CodeScreenProps {
  readonly code: string;
  readonly language?: string;
  readonly onBack: () => void;
}

export function CodeScreen({ code, language, onBack }: CodeScreenProps) {
  return (
    <View style={styles.container}>
      <View style={styles.header}>
        <Pressable accessibilityLabel="Back to conversation" onPress={onBack} style={styles.back}>
          <Ionicons name="chevron-back" size={23} color={colors.text} />
        </Pressable>
        <Text style={styles.title}>{language ?? "Code"}</Text>
        <View style={styles.back} />
      </View>
      <ScrollView horizontal contentContainerStyle={styles.codeScroll}>
        <ScrollView>
          <Text selectable style={styles.code}>
            {code}
          </Text>
        </ScrollView>
      </ScrollView>
    </View>
  );
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
  back: { width: 44, height: 44, alignItems: "center", justifyContent: "center" },
  title: {
    flex: 1,
    color: colors.text,
    textAlign: "center",
    fontSize: 15,
    fontWeight: "600",
  },
  codeScroll: { minWidth: "100%", padding: spacing.lg },
  code: { color: colors.text, fontFamily: "Menlo", fontSize: 13, lineHeight: 20 },
});
