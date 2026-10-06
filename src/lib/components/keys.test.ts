import { describe, expect, it } from 'vitest';
import { sendsOnEnter } from './keys';

const enter = { key: 'Enter', isComposing: false, shiftKey: false, metaKey: false, ctrlKey: false };

describe('composer enter', () => {
  it('ignores IME composition and shift-enter newlines', () => {
    expect(sendsOnEnter({ ...enter, isComposing: true }, 'enter')).toBe(false);
    expect(sendsOnEnter({ ...enter, shiftKey: true }, 'enter')).toBe(false);
    expect(sendsOnEnter(enter, 'enter')).toBe(true);
  });

  it('requires a modifier when Command-Enter is the send key', () => {
    expect(sendsOnEnter(enter, 'cmd_enter')).toBe(false);
    expect(sendsOnEnter({ ...enter, metaKey: true }, 'cmd_enter')).toBe(true);
    expect(sendsOnEnter({ ...enter, ctrlKey: true }, 'cmd_enter')).toBe(true);
    expect(sendsOnEnter({ ...enter, metaKey: true, isComposing: true }, 'cmd_enter')).toBe(false);
    expect(sendsOnEnter({ ...enter, metaKey: true, shiftKey: true }, 'cmd_enter')).toBe(false);
  });
});
