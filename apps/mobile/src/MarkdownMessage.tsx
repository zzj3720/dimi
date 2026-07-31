import { Linking, Pressable, StyleSheet, Text, View } from "react-native";

import { Ionicons } from "@expo/vector-icons";
import Markdown, { type ASTNode, type RenderRules } from "react-native-markdown-renderer";

import { colors, spacing } from "./theme";

interface MarkdownMessageProps {
  readonly value: string;
  readonly onOpenCode: (code: string, language?: string) => void;
}

const LONG_CODE_LINES = 10;
const LONG_CODE_CHARS = 600;

export function MarkdownMessage({ value, onOpenCode }: MarkdownMessageProps) {
  const code = (node: ASTNode) => {
    const content = node.content.replace(/\n$/, "");
    const lines = content.split("\n").length;
    if (lines > LONG_CODE_LINES || content.length > LONG_CODE_CHARS) {
      return (
        <Pressable
          key={node.key}
          accessibilityRole="button"
          accessibilityLabel={`Open ${lines} line code block`}
          onPress={() => {
            onOpenCode(content, node.sourceInfo || undefined);
          }}
          style={({ pressed }) => [styles.codeLink, pressed && styles.pressed]}
        >
          <Ionicons name="code-slash-outline" size={17} color={colors.primary} />
          <Text style={styles.codeLinkText}>Code · {lines} lines</Text>
          <Ionicons name="chevron-forward" size={16} color={colors.textMuted} />
        </Pressable>
      );
    }
    return (
      <View key={node.key} style={styles.codeBlock}>
        <Text selectable style={styles.codeText}>
          {content}
        </Text>
      </View>
    );
  };

  const rules: RenderRules = {
    fence: code,
    code_block: code,
  };

  return (
    <Markdown
      rules={rules}
      style={markdownStyles}
      defaultImageHandler={null}
      allowedImageHandlers={[]}
      onLinkPress={(url) => {
        if (url.startsWith("https://") || url.startsWith("http://")) void Linking.openURL(url);
        return false;
      }}
    >
      {value}
    </Markdown>
  );
}

const markdownStyles = {
  body: { color: colors.text, fontSize: 16, lineHeight: 24 },
  paragraph: { marginTop: 0, marginBottom: 10 },
  heading1: { color: colors.text, fontSize: 23, lineHeight: 29, marginBottom: 10 },
  heading2: { color: colors.text, fontSize: 20, lineHeight: 26, marginBottom: 8 },
  heading3: { color: colors.text, fontSize: 18, lineHeight: 24, marginBottom: 6 },
  link: { color: colors.primary, textDecorationLine: "underline" },
  blockquote: {
    borderLeftColor: colors.borderStrong,
    borderLeftWidth: 2,
    paddingLeft: 12,
    marginLeft: 0,
  },
  codeInline: {
    backgroundColor: colors.code,
    color: colors.text,
    fontFamily: "Menlo",
    fontSize: 14,
  },
  table: { borderColor: colors.border },
  tr: { borderBottomColor: colors.border },
  th: { color: colors.text },
  td: { color: colors.text },
};

const styles = StyleSheet.create({
  codeLink: {
    minHeight: 44,
    marginBottom: spacing.md,
    paddingHorizontal: spacing.md,
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    backgroundColor: colors.code,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.border,
  },
  codeLinkText: {
    flex: 1,
    color: colors.primary,
    fontSize: 14,
    fontWeight: "600",
  },
  codeBlock: {
    marginBottom: spacing.md,
    padding: spacing.md,
    backgroundColor: colors.code,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.border,
  },
  codeText: {
    color: colors.text,
    fontFamily: "Menlo",
    fontSize: 13,
    lineHeight: 19,
  },
  pressed: { opacity: 0.7 },
});
