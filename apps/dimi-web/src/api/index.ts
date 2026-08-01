// apps/dimi-web/src/api/index.ts
// Singleton factory for the DimiWebApi daemon client.

import { readDimiApiConfig } from './config';
import type { DimiWebApi } from './types';
import { DaemonDimiWebApi } from './daemon/client';

let singleton: DimiWebApi | undefined;

export function getDimiWebApi(): DimiWebApi {
  singleton ??= new DaemonDimiWebApi(readDimiApiConfig());
  return singleton;
}
