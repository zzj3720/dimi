import * as SecureStore from "expo-secure-store";

import type { StoredRemote } from "@dimi-agent/remote";

const REMOTE_KEY = "k-3720.remote.v1";

export async function loadStoredRemote(): Promise<StoredRemote | undefined> {
  const value = await SecureStore.getItemAsync(REMOTE_KEY);
  if (value === null) return undefined;
  try {
    return JSON.parse(value) as StoredRemote;
  } catch {
    await SecureStore.deleteItemAsync(REMOTE_KEY);
    return undefined;
  }
}

export function saveStoredRemote(remote: StoredRemote): Promise<void> {
  return SecureStore.setItemAsync(REMOTE_KEY, JSON.stringify(remote), {
    keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
  });
}

export function clearStoredRemote(): Promise<void> {
  return SecureStore.deleteItemAsync(REMOTE_KEY);
}
