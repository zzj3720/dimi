import { describe, it, expect } from 'vitest';

import {
  cacheHitRatePercent,
  formatCacheHitRate,
  formatDecimalTokenCount,
  formatTokenCount,
  promptTokenTotal,
  renderProgressBar,
  ratioSeverity,
  safeUsageRatio,
  usagePercent,
  usagePercentFromRatio,
} from '#/utils/usage/usage-format';

describe('formatTokenCount', () => {
  it('passes small values through unchanged', () => {
    expect(formatTokenCount(0)).toBe('0');
    expect(formatTokenCount(1)).toBe('1');
    expect(formatTokenCount(999)).toBe('999');
  });

  it('switches to k at 1024 and trims a redundant ".0"', () => {
    expect(formatTokenCount(1_000)).toBe('1000');
    expect(formatTokenCount(1_024)).toBe('1k');
    expect(formatTokenCount(1_536)).toBe('1.5k');
    expect(formatTokenCount(2_048)).toBe('2k');
  });

  it('rounds k values to 1 decimal', () => {
    expect(formatTokenCount(50_552)).toBe('49.4k');
    expect(formatTokenCount(262_144)).toBe('256k');
  });

  it('rounds k values at or above 100k to whole k', () => {
    expect(formatTokenCount(102_400)).toBe('100k');
    expect(formatTokenCount(999_999)).toBe('977k');
  });

  it('switches to M at 1024*1024', () => {
    expect(formatTokenCount(1_048_576)).toBe('1M');
    expect(formatTokenCount(1_572_864)).toBe('1.5M');
    expect(formatTokenCount(10_485_760)).toBe('10M');
  });

  it('clamps negatives and NaN to 0', () => {
    expect(formatTokenCount(-1)).toBe('0');
    expect(formatTokenCount(Number.NaN)).toBe('0');
    expect(formatTokenCount(Number.POSITIVE_INFINITY)).toBe('0');
  });
});

describe('formatDecimalTokenCount', () => {
  it('passes small values through unchanged', () => {
    expect(formatDecimalTokenCount(0)).toBe('0');
    expect(formatDecimalTokenCount(1)).toBe('1');
    expect(formatDecimalTokenCount(999)).toBe('999');
  });

  it('switches to k at 1000 with one trimmed decimal', () => {
    expect(formatDecimalTokenCount(1_000)).toBe('1k');
    expect(formatDecimalTokenCount(200_000)).toBe('200k');
    expect(formatDecimalTokenCount(950_000)).toBe('950k');
    expect(formatDecimalTokenCount(1_500)).toBe('1.5k');
  });

  it('switches to M at 1,000,000', () => {
    expect(formatDecimalTokenCount(1_000_000)).toBe('1M');
    expect(formatDecimalTokenCount(1_500_000)).toBe('1.5M');
    expect(formatDecimalTokenCount(10_000_000)).toBe('10M');
  });

  it('clamps negatives and NaN to 0', () => {
    expect(formatDecimalTokenCount(-1)).toBe('0');
    expect(formatDecimalTokenCount(Number.NaN)).toBe('0');
    expect(formatDecimalTokenCount(Number.POSITIVE_INFINITY)).toBe('0');
  });
});

describe('usagePercent', () => {
  it('returns 0 for zero usage', () => {
    expect(usagePercent(0, 1000)).toBe(0);
  });

  it('ceil-guarantees at least 1% for any non-zero usage', () => {
    expect(usagePercent(4, 10_000)).toBe(1);
  });

  it('ceils fractional percentages', () => {
    expect(usagePercent(427, 1000)).toBe(43);
    expect(usagePercent(992, 1000)).toBe(100);
  });

  it('clamps to 100 when used meets or exceeds max', () => {
    expect(usagePercent(1000, 1000)).toBe(100);
    expect(usagePercent(1200, 1000)).toBe(100);
  });

  it('returns 0 for a non-positive or non-finite max', () => {
    expect(usagePercent(500, 0)).toBe(0);
    expect(usagePercent(500, -1)).toBe(0);
    expect(usagePercent(500, Number.NaN)).toBe(0);
  });
});

describe('usagePercentFromRatio', () => {
  it('coerces NaN to 0', () => {
    expect(usagePercentFromRatio(Number.NaN)).toBe(0);
  });

  it('returns 0 for zero usage', () => {
    expect(usagePercentFromRatio(0)).toBe(0);
  });

  it('ceil-guarantees at least 1% for any non-zero ratio', () => {
    expect(usagePercentFromRatio(0.004)).toBe(1);
  });

  it('ceils fractional percentages and clamps above 100', () => {
    expect(usagePercentFromRatio(0.427)).toBe(43);
    expect(usagePercentFromRatio(1.5)).toBe(100);
  });
});

describe('renderProgressBar', () => {
  it('empty bar at ratio 0', () => {
    expect(renderProgressBar(0, 10)).toBe('░'.repeat(10));
  });
  it('full bar at ratio 1', () => {
    expect(renderProgressBar(1, 10)).toBe('█'.repeat(10));
  });
  it('half bar at ratio 0.5', () => {
    expect(renderProgressBar(0.5, 10)).toBe('█'.repeat(5) + '░'.repeat(5));
  });
  it('clamps ratios outside [0,1]', () => {
    expect(renderProgressBar(-1, 8)).toBe('░'.repeat(8));
    expect(renderProgressBar(2, 8)).toBe('█'.repeat(8));
  });
  it('coerces NaN to 0', () => {
    expect(renderProgressBar(Number.NaN, 6)).toBe('░'.repeat(6));
  });
});

describe('safeUsageRatio', () => {
  it('matches footer context usage clamping semantics', () => {
    expect(safeUsageRatio(Number.NaN)).toBe(0);
    expect(safeUsageRatio(-1)).toBe(0);
    expect(safeUsageRatio(0.427)).toBe(0.427);
    expect(safeUsageRatio(1.5)).toBe(1);
  });
});

describe('ratioSeverity', () => {
  it('green below 0.5', () => {
    expect(ratioSeverity(0)).toBe('ok');
    expect(ratioSeverity(0.49)).toBe('ok');
  });
  it('yellow in [0.5, 0.85)', () => {
    expect(ratioSeverity(0.5)).toBe('warn');
    expect(ratioSeverity(0.7)).toBe('warn');
    expect(ratioSeverity(0.849)).toBe('warn');
  });
  it('red at or above 0.85', () => {
    expect(ratioSeverity(0.85)).toBe('danger');
    expect(ratioSeverity(1)).toBe('danger');
  });
});

describe('promptTokenTotal / cacheHitRatePercent', () => {
  it('sums input + cache read + cache write', () => {
    expect(
      promptTokenTotal({
        inputOther: 100,
        output: 50,
        inputCacheRead: 800,
        inputCacheCreation: 100,
      }),
    ).toBe(1000);
  });

  it('ignores non-finite / non-positive buckets', () => {
    expect(
      promptTokenTotal({
        inputOther: Number.NaN,
        output: 10,
        inputCacheRead: -1,
        inputCacheCreation: 0,
      }),
    ).toBe(0);
  });

  it('returns undefined when no cache activity was reported', () => {
    expect(
      cacheHitRatePercent({
        inputOther: 1000,
        output: 10,
        inputCacheRead: 0,
        inputCacheCreation: 0,
      }),
    ).toBeUndefined();
  });

  it('computes CH% from cache read / prompt total (pi semantics)', () => {
    expect(
      cacheHitRatePercent({
        inputOther: 100,
        output: 20,
        inputCacheRead: 800,
        inputCacheCreation: 100,
      }),
    ).toBeCloseTo(80, 5);
  });

  it('counts a pure cache-write as 0% hit (not undefined)', () => {
    expect(
      cacheHitRatePercent({
        inputOther: 0,
        output: 0,
        inputCacheRead: 0,
        inputCacheCreation: 500,
      }),
    ).toBe(0);
  });

  it('formats compact CH labels', () => {
    expect(formatCacheHitRate(85)).toBe('CH85.0%');
    expect(formatCacheHitRate(99.82)).toBe('CH99.8%');
    expect(formatCacheHitRate(Number.NaN)).toBe('CH?%');
  });
});
