import { describe, expect, it } from 'vitest';
import { endpointError } from './endpoint';

describe('loopback endpoint checks', () => {
  it('accepts IPv4, localhost, bracketed IPv6, and a trailing slash', () => {
    expect(endpointError('http://127.0.0.1:1234/v1')).toBe('');
    expect(endpointError('http://localhost:11434/v1/')).toBe('');
    expect(endpointError('http://[::1]:8000/v1')).toBe('');
    expect(endpointError('http://127.0.0.1:1234')).toBe('');
    expect(endpointError('  http://[::1]:8000/v1  ')).toBe('');
  });

  it('rejects credentials, queries, fragments, port zero, and non-loopback hosts', () => {
    expect(endpointError('http://user:pass@127.0.0.1:1234/v1')).toMatch(/credentials/);
    expect(endpointError('http://127.0.0.1:1234/v1?x=1')).toMatch(/query/);
    expect(endpointError('http://127.0.0.1:1234/v1#frag')).toMatch(/fragment/);
    expect(endpointError('http://127.0.0.1:0/v1')).toMatch(/port/);
    expect(endpointError('https://127.0.0.1:1234/v1')).toMatch(/HTTP loopback/);
    expect(endpointError('http://192.168.1.4:1234/v1')).toMatch(/LAN/);
    expect(endpointError('http://127.0.0.1:1234/v1/models')).toMatch(/\/v1/);
    expect(endpointError('not a url')).toMatch(/valid/);
  });
});
