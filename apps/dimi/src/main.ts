/**
 * Dimi process entry point.
 *
 * This file must stay dependency-free (no static imports): ESM static
 * imports resolve and evaluate *before* any top-level code here runs, and
 * the agent-core-v2 OS backend modules read the legacy switch at module
 * load time. The process-wide `--legacy` flag therefore has to be set from
 * argv before anything else loads, then the real entry (`main-app`) is
 * loaded dynamically.
 */

if (process.argv.includes('--legacy')) {
  process.env['DIMI_LEGACY'] = '1';
}

await import('./main-app');
