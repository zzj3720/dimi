import { useState, useEffect } from "react";
import {
  IconLoader2,
  IconCopy,
  IconCheck,
  IconExternalLink,
  IconArrowRight,
} from "@tabler/icons-react";
import { Button } from "@/components/ui/button";
import { DimiMascot } from "./DimiMascot";
import { bridge, Events } from "@/services";
import type { LoginStatus } from "shared/types";

interface LoginScreenProps {
  onLoginSuccess: () => void;
  onSkip: () => void;
}

type LoginState = "idle" | "pending" | "error";

export function LoginScreen({ onLoginSuccess, onSkip }: LoginScreenProps) {
  const [state, setState] = useState<LoginState>("idle");
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [providers, setProviders] = useState<LoginStatus["providers"]>([]);
  const [providerId, setProviderId] = useState("");
  const [method, setMethod] = useState<"oauth" | "api_key">("oauth");
  const [apiKey, setApiKey] = useState("");

  useEffect(() => {
    const off = bridge.on<{ url: string }>(Events.LoginUrl, ({ url }) => {
      setUrl(url);
    });
    void bridge.checkLoginStatus().then((status) => {
      const available = status.providers.filter((provider) => provider.methods.length > 0);
      setProviders(available);
      const first = available[0];
      if (first !== undefined) {
        setProviderId(first.id);
        setMethod(first.methods[0]?.type ?? "oauth");
      }
    });
    return off;
  }, []);

  const handleLogin = async () => {
    if (providerId.length === 0) return;
    if (method === "api_key" && apiKey.trim().length === 0) {
      setState("error");
      setError("Enter an API key.");
      return;
    }
    setState("pending");
    setUrl(null);
    setError(null);
    try {
      const result = await bridge.login({
        providerId,
        method,
        value: method === "api_key" ? apiKey.trim() : undefined,
      });
      if (result.success) {
        onLoginSuccess();
      } else {
        const errorMessage = result.error || "Login failed";
        setState("error");
        setError(errorMessage);
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      setState("error");
      setError(errorMessage);
    }
  };

  const selectedProvider = providers.find((provider) => provider.id === providerId);
  const selectProvider = (id: string) => {
    setProviderId(id);
    const provider = providers.find((entry) => entry.id === id);
    setMethod(provider?.methods[0]?.type ?? "oauth");
    setApiKey("");
  };

  const handleCopyUrl = async () => {
    if (!url) return;
    await navigator.clipboard.writeText(url);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  if (state === "pending") {
    return (
      <div className="h-full flex items-center justify-center p-6">
        <div className="max-w-sm w-full text-center space-y-6">
          <DimiMascot className="h-12 mx-auto" />
          <div className="space-y-2">
            <div className="inline-flex items-center gap-2 text-blue-500">
              <IconLoader2 className="size-5 animate-spin" />
              <span className="text-sm font-medium">Waiting for authorization...</span>
            </div>
            <p className="text-xs leading-5 text-muted-foreground text-left">
              A browser window should open automatically. Complete the sign-in process there.
            </p>
          </div>
          {url && (
            <div className="bg-muted/50 rounded-lg p-2 text-left space-y-3">
              <p className="text-xs text-muted-foreground">
                If the browser didn&apos;t open, visit this URL:
              </p>
              <div className="flex items-center gap-2">
                <code className="flex-1 text-xs bg-background rounded px-2 py-1.5 font-mono break-all select-all">
                  {url}
                </code>
                <Button
                  variant="ghost"
                  size="icon"
                  className="shrink-0 size-8"
                  onClick={() => {
                    void handleCopyUrl();
                  }}
                >
                  {copied ? (
                    <IconCheck className="size-4 text-emerald-500" />
                  ) : (
                    <IconCopy className="size-4" />
                  )}
                </Button>
              </div>
              <a
                href={url}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1.5 text-xs text-blue-500 hover:underline"
              >
                <IconExternalLink className="size-3.5" />
                Open in browser
              </a>
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="h-full flex items-center justify-center p-6">
        <div className="max-w-sm w-full text-center space-y-6">
          <DimiMascot className="h-12 mx-auto" />
          <div className="space-y-2">
            <h1 className="text-lg font-semibold">Connect a model provider</h1>
            <div className="text-left space-y-2">
              <p className="text-xs leading-5">
                Sign in with a supported account or use an API key.
              </p>
            </div>
          </div>

          {error && (
            <div className="bg-red-50 dark:bg-red-950/30 border border-red-200 dark:border-red-900/50 rounded-lg px-3 py-2 text-left">
              <p className="text-xs text-red-600 dark:text-red-400">{error}</p>
            </div>
          )}

          <div className="space-y-5">
            <div className="text-left space-y-1">
              <label className="text-xs text-muted-foreground" htmlFor="provider-login">
                Provider
              </label>
              <select
                id="provider-login"
                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
                value={providerId}
                onChange={(event) => selectProvider(event.target.value)}
              >
                {providers.map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.name}
                  </option>
                ))}
              </select>
            </div>

            {selectedProvider !== undefined && selectedProvider.methods.length > 1 && (
              <div className="grid grid-cols-2 gap-2">
                {selectedProvider.methods.map((entry) => (
                  <Button
                    key={entry.type}
                    type="button"
                    variant={method === entry.type ? "default" : "outline"}
                    onClick={() => setMethod(entry.type)}
                  >
                    {entry.label}
                  </Button>
                ))}
              </div>
            )}

            {method === "api_key" && (
              <div className="text-left space-y-1">
                <label className="text-xs text-muted-foreground" htmlFor="provider-api-key">
                  API key
                </label>
                <input
                  id="provider-api-key"
                  type="password"
                  autoComplete="off"
                  className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
                  value={apiKey}
                  onChange={(event) => setApiKey(event.target.value)}
                />
              </div>
            )}

            <div className="text-left space-y-1">
              <Button
                onClick={() => {
                  void handleLogin();
                }}
                disabled={providerId.length === 0}
                className="w-full justify-center gap-2"
              >
                {method === "oauth" ? "Sign in" : "Connect"}
              </Button>
            </div>

            <div className="text-left space-y-1">
              <Button
                type="button"
                variant="outline"
                onClick={onSkip}
                className="w-full relative justify-center font-normal"
              >
                <span>Skip</span>
                <IconArrowRight className="size-4 text-muted-foreground absolute right-3" />
              </Button>
              <p className="text-[11px] text-muted-foreground leading-4">
                Use your existing API key configuration.
              </p>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
