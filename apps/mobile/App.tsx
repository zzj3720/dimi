import { useEffect, useMemo, useState } from "react";
import { StyleSheet, View } from "react-native";

import { StatusBar } from "expo-status-bar";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";

import { ChatScreen } from "./src/ChatScreen";
import { CodeScreen } from "./src/CodeScreen";
import { PairScreen } from "./src/PairScreen";
import { MobileRuntime } from "./src/runtime";
import { SessionListScreen } from "./src/SessionListScreen";
import { UpdateBanner } from "./src/UpdateBanner";
import { colors } from "./src/theme";

export default function App() {
  const runtime = useMemo(() => new MobileRuntime(), []);
  const [state, setState] = useState(runtime.state);
  const [booted, setBooted] = useState(false);
  const [code, setCode] = useState<{ code: string; language?: string }>();

  useEffect(() => {
    const unsubscribe = runtime.subscribe(setState);
    void runtime.initialize().finally(() => {
      setBooted(true);
    });
    return () => {
      unsubscribe();
      runtime.dispose();
    };
  }, [runtime]);

  const content = (() => {
    if (!booted) return <View style={styles.loading} />;
    if (state.remoteName === undefined)
      return <PairScreen onPair={(value) => runtime.pair(value)} />;
    if (code !== undefined) {
      return (
        <CodeScreen
          code={code.code}
          language={code.language}
          onBack={() => {
            setCode(undefined);
          }}
        />
      );
    }
    if (state.selectedSessionId !== undefined) {
      return (
        <ChatScreen
          runtime={runtime}
          state={state}
          onBack={() => {
            runtime.deselectSession();
          }}
          onOpenCode={(next, language) => {
            setCode({ code: next, language });
          }}
        />
      );
    }
    return (
      <SessionListScreen
        state={state}
        onSelect={(sessionId) => void runtime.selectSession(sessionId)}
        onRefresh={() => void runtime.refreshSessions()}
        onForget={() => void runtime.forgetRemote()}
      />
    );
  })();

  return (
    <SafeAreaProvider>
      <SafeAreaView edges={["top", "bottom"]} style={styles.container}>
        <UpdateBanner />
        {content}
        <StatusBar style="dark" />
      </SafeAreaView>
    </SafeAreaProvider>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, backgroundColor: colors.background },
  loading: { flex: 1, backgroundColor: colors.background },
});
