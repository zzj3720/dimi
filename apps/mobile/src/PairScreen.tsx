import { useState } from "react";
import {
  KeyboardAvoidingView,
  Platform,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { Ionicons } from "@expo/vector-icons";
import { CameraView, useCameraPermissions } from "expo-camera";
import * as Clipboard from "expo-clipboard";

import { colors, spacing } from "./theme";

interface PairScreenProps {
  readonly onPair: (value: string) => Promise<void>;
}

export function PairScreen({ onPair }: PairScreenProps) {
  const [value, setValue] = useState("");
  const [scanning, setScanning] = useState(false);
  const [permission, requestPermission] = useCameraPermissions();
  const [error, setError] = useState<string>();

  const submit = async (next = value): Promise<void> => {
    try {
      setError(undefined);
      await onPair(next.trim());
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    }
  };

  const openScanner = async (): Promise<void> => {
    if (permission?.granted !== true) {
      const result = await requestPermission();
      if (!result.granted) {
        setError("Camera access is required to scan a pairing code.");
        return;
      }
    }
    setScanning(true);
  };

  if (scanning) {
    return (
      <View style={styles.scanner}>
        <CameraView
          style={StyleSheet.absoluteFill}
          barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
          onBarcodeScanned={({ data }) => {
            setScanning(false);
            setValue(data);
            void submit(data);
          }}
        />
        <View style={styles.scanGuide} />
        <Pressable
          onPress={() => {
            setScanning(false);
          }}
          style={styles.closeScanner}
        >
          <Ionicons name="close" size={24} color="#FFFFFF" />
        </Pressable>
        <Text style={styles.scanLabel}>Scan the code shown by the runtime</Text>
      </View>
    );
  }

  return (
    <KeyboardAvoidingView
      behavior={Platform.OS === "ios" ? "padding" : undefined}
      style={styles.container}
    >
      <View style={styles.wordmark}>
        <View style={styles.mark}>
          <Ionicons name="swap-horizontal" size={20} color="#FFFFFF" />
        </View>
        <Text style={styles.brand}>k-3720</Text>
      </View>
      <View style={styles.intro}>
        <Text style={styles.title}>Connect this device</Text>
        <Text style={styles.body}>
          Messages are encrypted before they leave your runtime. The relay cannot read them.
        </Text>
      </View>
      <Pressable
        onPress={() => {
          void openScanner();
        }}
        style={({ pressed }) => [styles.primary, pressed && styles.pressed]}
      >
        <Ionicons name="scan-outline" size={19} color="#FFFFFF" />
        <Text style={styles.primaryText}>Scan pairing code</Text>
      </Pressable>
      <View style={styles.dividerRow}>
        <View style={styles.divider} />
        <Text style={styles.or}>or paste a link</Text>
        <View style={styles.divider} />
      </View>
      <View style={styles.inputRow}>
        <TextInput
          autoCapitalize="none"
          autoCorrect={false}
          onChangeText={setValue}
          placeholder="k-3720://pair?…"
          placeholderTextColor={colors.textMuted}
          style={styles.input}
          value={value}
        />
        <Pressable
          accessibilityLabel="Paste pairing link"
          onPress={() => {
            void Clipboard.getStringAsync().then(setValue);
          }}
          style={styles.iconButton}
        >
          <Ionicons name="clipboard-outline" size={19} color={colors.primary} />
        </Pressable>
      </View>
      <Pressable
        disabled={value.trim().length === 0}
        onPress={() => void submit()}
        style={({ pressed }) => [
          styles.secondary,
          value.trim().length === 0 && styles.disabled,
          pressed && styles.pressed,
        ]}
      >
        <Text style={styles.secondaryText}>Connect</Text>
      </Pressable>
      {error !== undefined ? <Text style={styles.error}>{error}</Text> : null}
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    justifyContent: "center",
    paddingHorizontal: spacing.xl,
    backgroundColor: colors.background,
  },
  wordmark: { flexDirection: "row", alignItems: "center", gap: spacing.sm, marginBottom: 56 },
  mark: {
    width: 34,
    height: 34,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.primary,
  },
  brand: { color: colors.text, fontSize: 18, fontWeight: "700", letterSpacing: -0.3 },
  intro: { marginBottom: 34 },
  title: {
    color: colors.text,
    fontSize: 30,
    lineHeight: 36,
    fontWeight: "700",
    letterSpacing: -0.8,
  },
  body: { color: colors.textMuted, fontSize: 16, lineHeight: 24, marginTop: spacing.md },
  primary: {
    height: 50,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: spacing.sm,
    backgroundColor: colors.primary,
  },
  primaryText: { color: "#FFFFFF", fontSize: 16, fontWeight: "600" },
  dividerRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.md,
    marginVertical: spacing.xl,
  },
  divider: { flex: 1, height: StyleSheet.hairlineWidth, backgroundColor: colors.border },
  or: { color: colors.textMuted, fontSize: 13 },
  inputRow: {
    minHeight: 50,
    flexDirection: "row",
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.borderStrong,
    backgroundColor: colors.surface,
  },
  input: { flex: 1, paddingHorizontal: spacing.md, color: colors.text, fontSize: 14 },
  iconButton: { width: 48, alignItems: "center", justifyContent: "center" },
  secondary: {
    height: 48,
    marginTop: spacing.md,
    alignItems: "center",
    justifyContent: "center",
    borderWidth: 1,
    borderColor: colors.primary,
  },
  secondaryText: { color: colors.primary, fontSize: 15, fontWeight: "600" },
  disabled: { opacity: 0.4 },
  pressed: { opacity: 0.72 },
  error: { marginTop: spacing.md, color: colors.danger, fontSize: 14, lineHeight: 20 },
  scanner: { flex: 1, backgroundColor: "#000000" },
  scanGuide: {
    position: "absolute",
    top: "31%",
    left: "15%",
    width: "70%",
    aspectRatio: 1,
    borderWidth: 2,
    borderColor: "#FFFFFF",
  },
  closeScanner: {
    position: "absolute",
    top: 54,
    right: 22,
    width: 44,
    height: 44,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: "rgba(0,0,0,0.42)",
  },
  scanLabel: {
    position: "absolute",
    bottom: 80,
    alignSelf: "center",
    color: "#FFFFFF",
    fontSize: 15,
    fontWeight: "600",
  },
});
