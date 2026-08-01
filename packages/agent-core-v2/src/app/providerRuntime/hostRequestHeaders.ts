/**
 * `providerRuntime` domain — host-provided default headers for outbound
 * provider requests.
 *
 * Mirrors v1's `dimiRequestHeaders`: the host (CLI / server) builds the full
 * Dimi identity headers (`User-Agent` + `X-Msh-*`) through
 * `createDimiDefaultHeaders` and seeds them here. Built-in providers merge
 * the full set only for first-party endpoints and only `User-Agent` for
 * third-party endpoints, so device identity never leaks across vendors.
 */

import { createDecorator, type ServiceIdentifier } from "#/_base/di/instantiation";
import {
  LifecycleScope,
  ScopeActivation,
  registerScopedService,
  type ScopeSeed,
} from "#/_base/di/scope";

export interface IHostRequestHeaders {
  readonly headers: Readonly<Record<string, string>>;
}

export const IHostRequestHeaders = createDecorator<IHostRequestHeaders>("hostRequestHeaders");

export class HostRequestHeaders implements IHostRequestHeaders {
  constructor(readonly headers: Readonly<Record<string, string>> = {}) {}
}

export function hostRequestHeadersSeed(headers: Readonly<Record<string, string>>): ScopeSeed {
  return [[IHostRequestHeaders as ServiceIdentifier<unknown>, new HostRequestHeaders(headers)]];
}

registerScopedService(
  LifecycleScope.App,
  IHostRequestHeaders,
  HostRequestHeaders,
  ScopeActivation.OnScopeCreated,
  "model",
);
