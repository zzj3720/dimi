import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

import lockfile from "proper-lockfile";
import { parse, printParseErrorCode } from "jsonc-parser";

import { atomicWrite } from "#/_base/utils/fs";
import { Error2 } from "#/_base/errors/errors";

import { ProviderRuntimeErrors } from "./errors";

import type {
  Credential,
  CredentialInfo,
  CredentialStore,
  CustomProviderDefinition,
  ModelsStore,
  ModelsStoreEntry,
} from "./types";

const FILE_MODE = 0o600;
const DIRECTORY_MODE = 0o700;

type CredentialDocument = Record<string, Credential>;
type ModelsDocument = Record<string, ModelsStoreEntry>;
interface CustomProvidersDocument {
  providers: Record<string, Omit<CustomProviderDefinition, "id">>;
}

/** Shared JSONC parser for models.json and provider import entry points. */
export function parseJsonc(input: string): unknown {
  const errors: import("jsonc-parser").ParseError[] = [];
  const value = parse(input, errors, { allowTrailingComma: true, disallowComments: false });
  if (errors.length > 0) {
    throw new SyntaxError(errors.map((error) => printParseErrorCode(error.error)).join("; "));
  }
  return value;
}

export interface CustomProvidersLoadResult {
  providers: readonly CustomProviderDefinition[];
  error?: Error;
}

export class InMemoryCredentialStore implements CredentialStore {
  private readonly credentials = new Map<string, Credential>();
  private readonly chains = new Map<string, Promise<unknown>>();

  read(providerId: string): Promise<Credential | undefined> {
    return Promise.resolve(this.credentials.get(providerId));
  }

  list(): Promise<readonly CredentialInfo[]> {
    return Promise.resolve(
      [...this.credentials].map(([providerId, credential]) => ({
        providerId,
        type: credential.type,
      })),
    );
  }

  modify(
    providerId: string,
    fn: (current: Credential | undefined) => Promise<Credential | undefined>,
  ): Promise<Credential | undefined> {
    return this.enqueue(providerId, async () => {
      const current = this.credentials.get(providerId);
      const next = await fn(current);
      if (next !== undefined) this.credentials.set(providerId, next);
      return next ?? current;
    });
  }

  delete(providerId: string): Promise<void> {
    return this.enqueue(providerId, async () => {
      this.credentials.delete(providerId);
    });
  }

  private enqueue<T>(providerId: string, task: () => Promise<T>): Promise<T> {
    const previous = this.chains.get(providerId) ?? Promise.resolve();
    const next = previous.catch(() => undefined).then(task);
    this.chains.set(
      providerId,
      next.catch(() => undefined),
    );
    return next;
  }
}

export class InMemoryModelsStore implements ModelsStore {
  private readonly entries = new Map<string, ModelsStoreEntry>();

  async read(providerId: string): Promise<ModelsStoreEntry | undefined> {
    const entry = this.entries.get(providerId);
    return entry === undefined ? undefined : structuredClone(entry);
  }

  async write(providerId: string, entry: ModelsStoreEntry): Promise<void> {
    this.entries.set(providerId, structuredClone(entry));
  }

  delete(providerId: string): Promise<void> {
    this.entries.delete(providerId);
    return Promise.resolve();
  }
}

class LockedJsonFile<T extends object> {
  private pending: Promise<void> = Promise.resolve();

  constructor(
    private readonly path: string,
    private readonly empty: T,
  ) {}

  private async ensure(): Promise<void> {
    await mkdir(dirname(this.path), { recursive: true, mode: DIRECTORY_MODE });
    try {
      await writeFile(this.path, JSON.stringify(this.empty), {
        encoding: "utf8",
        flag: "wx",
        mode: FILE_MODE,
      });
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    }
  }

  private async parse(): Promise<T> {
    const source = await readFile(this.path, "utf8");
    return source.trim().length === 0
      ? structuredClone(this.empty)
      : (parseJsonc(source) as T);
  }

  async read(): Promise<T> {
    return this.exclusive(async () => {
      await this.ensure();
      const release = await lockfile.lock(this.path, {
        realpath: false,
        retries: { retries: 10, factor: 2, minTimeout: 50, maxTimeout: 1_000 },
        stale: 30_000,
      });
      try {
        return structuredClone(await this.parse());
      } finally {
        await release();
      }
    });
  }

  async update<R>(fn: (current: T) => Promise<{ result: R; next?: T }>): Promise<R> {
    return this.exclusive(async () => {
      await this.ensure();
      const release = await lockfile.lock(this.path, {
        realpath: false,
        retries: { retries: 10, factor: 2, minTimeout: 50, maxTimeout: 10_000 },
        stale: 30_000,
      });
      try {
        const current = await this.parse();
        const { result, next } = await fn(current);
        if (next !== undefined) {
          await atomicWrite(this.path, JSON.stringify(next, null, 2), undefined, FILE_MODE);
        }
        return result;
      } finally {
        await release();
      }
    });
  }

  private exclusive<R>(operation: () => Promise<R>): Promise<R> {
    const result = this.pending.then(operation, operation);
    this.pending = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }
}

export class FileCredentialStore implements CredentialStore {
  private readonly file: LockedJsonFile<CredentialDocument>;

  constructor(path: string) {
    this.file = new LockedJsonFile(path, {});
  }

  async read(providerId: string): Promise<Credential | undefined> {
    return (await this.file.read())[providerId];
  }

  async list(): Promise<readonly CredentialInfo[]> {
    return Object.entries(await this.file.read()).map(([providerId, credential]) => ({
      providerId,
      type: credential.type,
    }));
  }

  async modify(
    providerId: string,
    fn: (current: Credential | undefined) => Promise<Credential | undefined>,
  ): Promise<Credential | undefined> {
    return this.file.update(async (current) => {
      const nextCredential = await fn(current[providerId]);
      if (nextCredential === undefined) {
        return { result: current[providerId] };
      }
      const next = { ...current, [providerId]: nextCredential };
      return { result: nextCredential, next };
    });
  }

  async delete(providerId: string): Promise<void> {
    await this.file.update(async (current) => {
      if (!(providerId in current)) return { result: undefined };
      const next = { ...current };
      delete next[providerId];
      return { result: undefined, next };
    });
  }
}

export class FileModelsStore implements ModelsStore {
  private readonly file: LockedJsonFile<ModelsDocument>;

  constructor(path: string) {
    this.file = new LockedJsonFile(path, {});
  }

  async read(providerId: string): Promise<ModelsStoreEntry | undefined> {
    return (await this.file.read())[providerId];
  }

  async write(providerId: string, entry: ModelsStoreEntry): Promise<void> {
    await this.file.update(async (current) => ({
      result: undefined,
      next: { ...current, [providerId]: structuredClone(entry) },
    }));
  }

  async delete(providerId: string): Promise<void> {
    await this.file.update(async (current) => {
      if (!(providerId in current)) return { result: undefined };
      const next = { ...current };
      delete next[providerId];
      return { result: undefined, next };
    });
  }
}

export class FileCustomProvidersStore {
  private readonly file: LockedJsonFile<CustomProvidersDocument>;

  constructor(path: string) {
    this.file = new LockedJsonFile(path, { providers: {} });
  }

  async list(): Promise<readonly CustomProviderDefinition[]> {
    return Object.entries(this.validated(await this.file.read()).providers).map(([id, definition]) => ({
      ...definition,
      id,
    }));
  }

  /**
   * ModelConfig-style read for product boot: an invalid user-editable file is
   * reported to the caller but never makes the built-in catalog unavailable.
   * Mutations deliberately keep using `list`/`validated`, so a damaged file is
   * neither overwritten nor silently repaired.
   */
  async load(): Promise<CustomProvidersLoadResult> {
    try {
      return { providers: await this.list() };
    } catch (error) {
      return {
        providers: [],
        error: error instanceof Error ? error : new Error(String(error)),
      };
    }
  }

  async set(
    definition: CustomProviderDefinition,
    validate?: (definitions: readonly CustomProviderDefinition[]) => void,
  ): Promise<void> {
    await this.file.update(async (current) => {
      const document = this.validated(current);
      const providers = {
        ...document.providers,
        [definition.id]: withoutId(definition),
      };
      validate?.(Object.entries(providers).map(([id, value]) => ({ ...value, id })));
      return {
        result: undefined,
        next: {
          ...document,
          providers,
        },
      };
    });
  }

  async delete(id: string): Promise<void> {
    await this.file.update(async (current) => {
      const document = this.validated(current);
      if (!(id in document.providers)) return { result: undefined };
      const providers = { ...document.providers };
      delete providers[id];
      return { result: undefined, next: { ...document, providers } };
    });
  }

  private validated(value: unknown): CustomProvidersDocument {
    if (
      typeof value !== "object" ||
      value === null ||
      Array.isArray(value) ||
      Object.keys(value).some((key) => key !== "providers") ||
      !("providers" in value) ||
      typeof value.providers !== "object" ||
      value.providers === null ||
      Array.isArray(value.providers)
    ) {
      throw new Error2(
        ProviderRuntimeErrors.codes.PROVIDER_DEFINITION_STORE_INVALID,
        "models.json must contain { providers: { ... } }",
      );
    }
    return value as CustomProvidersDocument;
  }
}

function withoutId(definition: CustomProviderDefinition): Omit<CustomProviderDefinition, "id"> {
  const { id: _id, ...stored } = definition;
  return structuredClone(stored);
}
