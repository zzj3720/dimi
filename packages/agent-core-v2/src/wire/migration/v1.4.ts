import type { WireMigration, WireMigrationRecord } from './migration';

export const migrateV1_3ToV1_4: WireMigration = {
  sourceVersion: '1.3',
  targetVersion: '1.4',
  migrateRecord(record: WireMigrationRecord): WireMigrationRecord {
    return record;
  },
};
