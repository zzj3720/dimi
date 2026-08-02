import type { WireMigration, WireMigrationRecord } from './migration';

export const migrateV1_3ToV1_4: WireMigration = {
  sourceVersion: '1.3',
  targetVersion: '1.4',
  migrateRecord(record: WireMigrationRecord): WireMigrationRecord {
    // Legacy goal records are no longer recognized by the runtime and pass
    // through unchanged; restore skips unknown record types.
    return record;
  },
};
