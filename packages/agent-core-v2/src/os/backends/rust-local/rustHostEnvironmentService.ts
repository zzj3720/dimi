/**
 * `hostEnvironment` domain (L1) — `IHostEnvironment` Rust-backed
 * implementation (M2, slice 3).
 *
 * Default backend since M2 (the CLI `--legacy` flag keeps node-local): the OS / shell / path-style probe
 * runs on the Rust side (`rustHostEnvironmentProbe`); the login-shell PATH
 * enrichment stays on the Node side (`applyLoginShellPathFromNode`) because
 * it mutates `process.env.PATH` — a Node-specific side effect. `ready`
 * awaits both, exactly like the node-local service.
 */

import { BugIndicatingError } from '#/_base/errors/errors';
import { rustHostEnvironmentProbe, type RustHostEnvironmentInfo } from '@dimi-agent/dimi-native';
import { applyLoginShellPathFromNode } from '#/_base/execEnv/loginShellPath';

import {
  IHostEnvironment,
  type OsKind,
  type PathClass,
  type ShellName,
} from '#/os/interface/hostEnvironment';

export class RustHostEnvironmentService implements IHostEnvironment {
  declare readonly _serviceBrand: undefined;

  private _info?: RustHostEnvironmentInfo;
  readonly ready: Promise<void>;

  constructor() {
    this.ready = Promise.all([
      Promise.resolve().then(() => {
        this._info = rustHostEnvironmentProbe();
      }),
      applyLoginShellPathFromNode(),
    ]).then(() => {});
  }

  private require(field: keyof RustHostEnvironmentInfo): never | string {
    if (this._info === undefined) {
      throw new BugIndicatingError(
        `IHostEnvironment.${field} accessed before ready — await IHostEnvironment.ready first (composition root should do so before creating a Session scope).`,
      );
    }
    return this._info[field];
  }

  get osKind(): OsKind {
    return this.require('osKind') as OsKind;
  }

  get osArch(): string {
    return this.require('osArch') as string;
  }

  get osVersion(): string {
    return this.require('osVersion') as string;
  }

  get shellName(): ShellName {
    return this.require('shellName') as ShellName;
  }

  get shellPath(): string {
    return this.require('shellPath') as string;
  }

  get pathClass(): PathClass {
    return this.require('pathClass') as PathClass;
  }

  get homeDir(): string {
    return this.require('homeDir') as string;
  }
}
