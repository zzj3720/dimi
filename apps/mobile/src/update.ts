import { z } from "zod";

export const updateManifestSchema = z.object({
  schema: z.literal(1),
  channel: z.literal("internal"),
  platform: z.literal("android"),
  version: z.string().min(1),
  versionCode: z.number().int().positive(),
  minVersionCode: z.number().int().positive(),
  publishedAt: z.iso.datetime(),
  commit: z.string().min(7),
  apk: z.object({
    url: z.url().refine((url) => url.startsWith("https://")),
    sha256: z.string().regex(/^[a-f0-9]{64}$/),
    size: z.number().int().positive(),
  }),
});

export type UpdateManifest = z.infer<typeof updateManifestSchema>;
export type UpdateState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "downloading"; manifest: UpdateManifest }
  | { phase: "ready"; manifest: UpdateManifest; artifact: string }
  | { phase: "installing"; manifest: UpdateManifest }
  | { phase: "error"; message: string };

export interface UpdatePlatform {
  currentVersionCode(): number;
  fetchManifest(): Promise<UpdateManifest>;
  downloadAndVerify(manifest: UpdateManifest): Promise<string>;
  install(artifact: string): Promise<void>;
}

export class UpdateController {
  #state: UpdateState = { phase: "idle" };
  #listeners = new Set<(state: UpdateState) => void>();
  #operation = 0;

  constructor(private readonly platform: UpdatePlatform) {}

  get state(): UpdateState {
    return this.#state;
  }

  subscribe(listener: (state: UpdateState) => void): () => void {
    this.#listeners.add(listener);
    listener(this.#state);
    return () => this.#listeners.delete(listener);
  }

  async check(): Promise<void> {
    const operation = ++this.#operation;
    this.#set({ phase: "checking" });
    try {
      const manifest = await this.platform.fetchManifest();
      const current = this.platform.currentVersionCode();
      if (current >= manifest.versionCode) {
        this.#set({ phase: "idle" });
        return;
      }
      if (current < manifest.minVersionCode) {
        throw new Error("This build is too old to update automatically. Reinstall from the install page.");
      }
      this.#set({ phase: "downloading", manifest });
      const artifact = await this.platform.downloadAndVerify(manifest);
      if (operation === this.#operation) this.#set({ phase: "ready", manifest, artifact });
    } catch (error) {
      if (operation === this.#operation) this.#set({ phase: "error", message: messageFor(error) });
    }
  }

  async install(): Promise<void> {
    if (this.#state.phase !== "ready") return;
    const { artifact, manifest } = this.#state;
    this.#set({ phase: "installing", manifest });
    try {
      await this.platform.install(artifact);
      this.#set({ phase: "ready", manifest, artifact });
    } catch (error) {
      this.#set({ phase: "error", message: messageFor(error) });
    }
  }

  dispose(): void {
    this.#operation += 1;
    this.#listeners.clear();
  }

  #set(state: UpdateState): void {
    this.#state = state;
    for (const listener of this.#listeners) listener(state);
  }
}

function messageFor(error: unknown): string {
  return error instanceof Error ? error.message : "Update failed.";
}
