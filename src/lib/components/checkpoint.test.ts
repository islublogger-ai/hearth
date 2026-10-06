import { afterEach, describe, expect, it, vi } from 'vitest';
import { createCheckpoint } from './checkpoint';

describe('conversation checkpoint', () => {
  afterEach(() => { vi.useRealTimers(); });

  it('writes the latest id once per interval instead of postponing on every update', () => {
    vi.useFakeTimers();
    const saved: string[] = [];
    const checkpoint = createCheckpoint((id) => saved.push(id), 400);
    checkpoint.schedule('older');
    vi.advanceTimersByTime(200);
    checkpoint.schedule('latest');
    vi.advanceTimersByTime(199);
    expect(saved).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(saved).toEqual(['latest']);
  });

  it('starts a new interval after a write and flush does not double-fire', () => {
    vi.useFakeTimers();
    const saved: string[] = [];
    const checkpoint = createCheckpoint((id) => saved.push(id), 400);
    checkpoint.schedule('a');
    checkpoint.flush();
    expect(saved).toEqual(['a']);
    vi.advanceTimersByTime(400);
    expect(saved).toEqual(['a']);
    checkpoint.schedule('b');
    vi.advanceTimersByTime(400);
    expect(saved).toEqual(['a', 'b']);
  });
});
