import { describe, expect, it } from 'vitest';
import { fitLabel, formatBytes, hardwareLine, loadedLabel } from './format';

const GB = 1024 ** 3;

describe('fit labels stay honest', () => {
  it('does not invent a green fit without a KV size', () => {
    expect(fitLabel(1.5 * GB, 32 * GB).level).not.toBe('green');
    expect(fitLabel(1.5 * GB, 32 * GB).text).toMatch(/KV shape unknown/);
  });

  it('marks a 17 GB weight estimate as too large for 8 GB', () => {
    const label = fitLabel(17 * GB, 8 * GB);
    expect(label.level).toBe('red');
    expect(label.text).toMatch(/Does not fit/);
  });

  it('marks a 7B-class weight estimate as tight on 8 GB', () => {
    expect(fitLabel(4 * GB, 8 * GB)).toMatchObject({ level: 'amber' });
  });

  it('says unknown when weights or memory were not reported', () => {
    expect(fitLabel(null, 8 * GB).text).toMatch(/weights were not reported/);
    expect(fitLabel(GB, 0).text).toMatch(/memory size was not reported/);
    expect(formatBytes(null)).toBe('unknown');
    expect(formatBytes(0)).toBe('unknown');
  });

  it('does not turn a missing loaded flag into loaded or not loaded', () => {
    expect(loadedLabel({ loaded: null })).toBe('loaded state unknown');
    expect(loadedLabel({ loaded: false })).toBe('not loaded');
    expect(hardwareLine('Browser preview', 0, null)).toBe('Browser preview · memory unknown · GPU wired limit unset');
  });
});
