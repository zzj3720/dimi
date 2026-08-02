/**
 * The migration is now the identity — every record passes through unchanged.
 */
import type { WireMigration, WireMigrationRecord } from './migration';

export const migrateV1_4ToV1_5: WireMigration = {
  sourceVersion: '1.4',
  targetVersion: '1.5',
  migrateRecord(record: WireMigrationRecord): WireMigrationRecord {
    return record;
  },
};
