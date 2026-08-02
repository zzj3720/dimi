/**
 * `terminal` domain (L6) — `IHostTerminalService` Rust-backed
 * implementation (M2, slice 5).
 *
 * Swap-in socket for `DIMI_RUST_PTY=1`: spawns through `dimi-exec::pty`
 * (portable-pty via the napi bridge) instead of node-pty. The bridge handle
 * streams output chunks through `setOnData` and the exit event through
 * `setOnExit`; `write`/`resize`/`kill` forward to the pty.
 */

import { Emitter, type Event } from '#/_base/event';
import { rustTerminalSpawn, type RustTerminalProcessHandle } from '@dimi-agent/dimi-native';
import { Disposable } from '#/_base/di/lifecycle';
import { LifecycleScope, ScopeActivation, registerScopedService } from '#/_base/di/scope';

import {
  IHostTerminalService,
  type TerminalProcess,
  type TerminalSpawnOptions,
} from '#/os/interface/terminal';

class RustTerminalProcess implements TerminalProcess {
  readonly onProcessData: Event<string>;
  readonly onProcessExit: Event<{ exitCode: number | null }>;

  private readonly dataEmitter = new Emitter<string>();
  private readonly exitEmitter = new Emitter<{ exitCode: number | null }>();
  private readonly handle: RustTerminalProcessHandle;

  constructor(handle: RustTerminalProcessHandle) {
    this.handle = handle;
    this.onProcessData = this.dataEmitter.event;
    this.onProcessExit = this.exitEmitter.event;
    handle.setOnData((data) => this.dataEmitter.fire(data));
    handle.setOnExit((exit) => this.exitEmitter.fire({ exitCode: exit.exitCode ?? null }));
  }

  write(data: string): void {
    this.handle.write(data);
  }

  resize(cols: number, rows: number): void {
    this.handle.resize(cols, rows);
  }

  kill(): void {
    this.handle.kill();
  }
}

export class RustHostTerminalService extends Disposable implements IHostTerminalService {
  declare readonly _serviceBrand: undefined;

  private readonly processes = new Set<TerminalProcess>();

  async spawn(options: TerminalSpawnOptions): Promise<TerminalProcess> {
    const handle = rustTerminalSpawn({
      cwd: options.cwd,
      shell: options.shell,
      cols: options.cols,
      rows: options.rows,
      env: globalThis.process.env as Record<string, string>,
    });
    const terminalProcess = new RustTerminalProcess(handle);
    this.processes.add(terminalProcess);
    return terminalProcess;
  }

  override dispose(): void {
    for (const process of this.processes) {
      try {
        process.kill();
      } catch {
      }
    }
    this.processes.clear();
    super.dispose();
  }
}

registerScopedService(
  LifecycleScope.App,
  IHostTerminalService,
  RustHostTerminalService,
  ScopeActivation.OnScopeCreated,
  'rustTerminal',
);
