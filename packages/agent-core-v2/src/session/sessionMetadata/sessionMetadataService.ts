/**
 * `sessionMetadata` domain (L6) — `ISessionMetadata` implementation.
 *
 * Persists the session metadata document (`state.json`) through the `storage`
 * access-pattern store (`IAtomicDocumentStore`), rooted at the `metaScope`
 * namespace from `sessionContext`. Loads the existing document on
 * construction (creating it on first run), and logs through `log`. The
 * plain-data state (`data`) is registered into `sessionState`
 * (`ISessionStateService`) and read/written through it. The
 * document always carries `version`, `cwd`, `agents`, and `custom` as one
 * authoritative current schema. Re-registering an agent whose metadata is unchanged is
 * a no-op (no write, no mirror, no event), so resuming a session — which
 * re-registers its agents as they materialize — never bumps `updatedAt` and
 * never reorders session listings. Bound at Session scope.
 */

import { Disposable } from '#/_base/di/lifecycle';
import { LifecycleScope, ScopeActivation, registerScopedService } from '#/_base/di/scope';
import { Emitter, type Event } from '#/_base/event';
import { ILogService } from '#/_base/log/log';
import { defineState } from '#/_base/state/stateRegistry';
import { IAtomicDocumentStore } from '#/persistence/interface/atomicDocumentStore';
import { ISessionContext } from '#/session/sessionContext/sessionContext';
import { ISessionStateService } from '#/session/state/sessionState';

import {
  ISessionMetadata,
  SESSION_META_VERSION,
  type AgentMeta,
  type SessionMeta,
  type SessionMetadataChangedEvent,
  type SessionMetaPatch,
} from './sessionMetadata';

const META_KEY = 'state.json';

export const sessionMetadataDataKey = defineState<SessionMeta | undefined>(
  'sessionMetadata.data',
  () => undefined,
);

export class SessionMetadata extends Disposable implements ISessionMetadata {
  declare readonly _serviceBrand: undefined;
  readonly ready: Promise<void>;
  readonly onDidChangeMetadata: Event<SessionMetadataChangedEvent>;

  private readonly _onDidChangeMetadata = this._register(
    new Emitter<SessionMetadataChangedEvent>(),
  );
  private readonly scope: string;
  private updateQueue: Promise<void> = Promise.resolve();

  constructor(
    @ISessionStateService private readonly states: ISessionStateService,
    @ISessionContext private readonly ctx: ISessionContext,
    @IAtomicDocumentStore private readonly store: IAtomicDocumentStore,
    @ILogService private readonly log: ILogService,
  ) {
    super();
    this.states.register(sessionMetadataDataKey);
    this.scope = ctx.metaScope;
    this.onDidChangeMetadata = this._onDidChangeMetadata.event;
    this.ready = this.load();
  }

  private get data(): SessionMeta {
    return this.states.get(sessionMetadataDataKey) as SessionMeta;
  }

  private set data(value: SessionMeta) {
    this.states.set(sessionMetadataDataKey, value);
  }

  async read(): Promise<SessionMeta> {
    await this.ready;
    return this.data;
  }

  async update(patch: SessionMetaPatch): Promise<void> {
    return this.enqueueUpdate(() => this.applyUpdate(patch));
  }

  private async applyUpdate(patch: SessionMetaPatch): Promise<void> {
    await this.ready;
    this.data = { ...this.data, ...patch, updatedAt: Date.now() };
    await this.store.set(this.scope, META_KEY, this.data);
    this._onDidChangeMetadata.fire({
      changed: Object.keys(patch) as (keyof SessionMeta)[],
    });
  }

  async setTitle(title: string): Promise<void> {
    await this.update({ title, isCustomTitle: true });
  }

  async setArchived(archived: boolean): Promise<void> {
    await this.update({ archived });
  }

  async registerAgent(agentId: string, meta: AgentMeta): Promise<void> {
    return this.enqueueUpdate(async () => {
      await this.ready;
      const existing = this.data.agents?.[agentId];
      if (existing !== undefined && agentMetaEquals(existing, meta)) return;
      const agents = { ...this.data.agents, [agentId]: meta };
      await this.applyUpdate({ agents });
    });
  }

  private enqueueUpdate(work: () => Promise<void>): Promise<void> {
    const run = this.updateQueue.then(work, work);
    this.updateQueue = run.catch(() => {});
    return run;
  }

  private async load(): Promise<void> {
    const existing = await this.store.get<SessionMeta>(this.scope, META_KEY);
    if (existing !== undefined) {
      this.data = existing;
      return;
    }
    const now = Date.now();
    this.data = {
      id: this.ctx.sessionId,
      version: SESSION_META_VERSION,
      cwd: this.ctx.cwd,
      createdAt: now,
      updatedAt: now,
      archived: false,
      agents: {},
      custom: {},
    };
    await this.store.set(this.scope, META_KEY, this.data);
    this.log.debug('session metadata created', { sessionId: this.ctx.sessionId });
  }
}

function agentMetaEquals(a: AgentMeta, b: AgentMeta): boolean {
  return (
    a.homedir === b.homedir &&
    a.type === b.type &&
    (a.parentAgentId ?? null) === (b.parentAgentId ?? null) &&
    a.forkedFrom === b.forkedFrom &&
    a.swarmItem === b.swarmItem &&
    recordEquals(a.labels, b.labels)
  );
}

function recordEquals(a: AgentMeta['labels'], b: AgentMeta['labels']): boolean {
  const entriesA = Object.entries(a ?? {});
  const entriesB = Object.entries(b ?? {});
  return (
    entriesA.length === entriesB.length && entriesA.every(([key, value]) => b?.[key] === value)
  );
}

registerScopedService(
  LifecycleScope.Session,
  ISessionMetadata,
  SessionMetadata,
  ScopeActivation.OnScopeCreated,
  'sessionMetadata',
);
