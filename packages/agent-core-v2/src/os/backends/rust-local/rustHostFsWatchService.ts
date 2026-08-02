/**
 * `hostFsWatch` domain (L1) — `IHostFsWatchService` Rust-backed
 * implementation (M2, slice 4).
 *
 * Swap-in socket for `DIMI_RUST_WATCH=1`: watches through `dimi-exec`
 * (`notify` via the napi bridge) instead of chokidar. The bridge handle
 * normalizes events to the chokidar surface (created/modified/deleted ×
 * file/directory), reports lexical absolute paths and filters `.git` path
 * segments (the node-local DEFAULT_IGNORED). The `ignored` callback option
 * stays here on the adapter, exactly like the node-local backend.
 */

import { rustFsWatch, type RustFsWatchHandle } from '@dimi-agent/dimi-native';

import { Emitter, type Event } from '#/_base/event';
import { LifecycleScope, ScopeActivation, registerScopedService } from '#/_base/di/scope';

import {
  type HostFsChange,
  type HostFsWatchOptions,
  type IHostFsWatchHandle,
  IHostFsWatchService,
} from '#/os/interface/hostFsWatch';

class RustHostFsWatchHandle implements IHostFsWatchHandle {
  readonly onDidChange: Event<HostFsChange>;

  private readonly emitter: Emitter<HostFsChange>;
  private readonly handle: RustFsWatchHandle;
  private readonly ignored: ((path: string) => boolean) | undefined;
  private disposed = false;

  constructor(path: string, options: HostFsWatchOptions | undefined) {
    this.emitter = new Emitter<HostFsChange>();
    this.onDidChange = this.emitter.event;
    this.ignored = options?.ignored;
    this.handle = rustFsWatch(path, { recursive: options?.recursive });
    this.handle.setOnChange((change) => {
      if (this.disposed) return;
      if (this.ignored !== undefined && this.ignored(change.path)) return;
      this.emitter.fire(change);
    });
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.handle.dispose();
    this.emitter.dispose();
  }
}

export class RustHostFsWatchService implements IHostFsWatchService {
  declare readonly _serviceBrand: undefined;

  watch(path: string, options?: HostFsWatchOptions): IHostFsWatchHandle {
    return new RustHostFsWatchHandle(path, options);
  }
}

registerScopedService(
  LifecycleScope.App,
  IHostFsWatchService,
  RustHostFsWatchService,
  ScopeActivation.OnScopeCreated,
  'rustHostFsWatch',
);
