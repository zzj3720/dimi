/**
 * `hostIdentity` domain (L3) — runtime identity of the embedding host.
 *
 * Holds the optional reply-style guide an embedding host injects at the
 * composition root. Bound at App scope.
 */

import { createDecorator, type ServiceIdentifier } from '#/_base/di/instantiation';
import { LifecycleScope, registerScopedService, ScopeActivation, type ScopeSeed } from '#/_base/di/scope';

export interface HostIdentityOverrides {
  readonly replyStyleGuide?: string;
}

export interface IHostIdentity {
  readonly _serviceBrand: undefined;
  readonly replyStyleGuide?: string;
}

export const IHostIdentity: ServiceIdentifier<IHostIdentity> =
  createDecorator<IHostIdentity>('hostIdentity');

export class HostIdentity implements IHostIdentity {
  declare readonly _serviceBrand: undefined;

  constructor(readonly replyStyleGuide?: string) {}
}

export function hostIdentitySeed(overrides: HostIdentityOverrides | undefined): ScopeSeed {
  if (overrides === undefined) return [];
  if (overrides.replyStyleGuide === undefined) return [];
  return [
    [
      IHostIdentity as ServiceIdentifier<unknown>,
      new HostIdentity(overrides.replyStyleGuide),
    ],
  ];
}

registerScopedService(
  LifecycleScope.App,
  IHostIdentity,
  HostIdentity,
  ScopeActivation.OnScopeCreated,
  'hostIdentity',
);
