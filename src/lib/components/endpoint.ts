/** Empty string when the URL matches the Rust loopback rule. */
export function endpointError(raw: string): string {
  let url: URL;
  try { url = new URL(raw.trim()); }
  catch { return 'Enter a valid local backend URL.'; }
  if (url.protocol !== 'http:' || url.username !== '' || url.password !== '' || url.search !== '' || url.hash !== '') {
    return 'Use an HTTP loopback URL without credentials, a query, or a fragment.';
  }
  const host = url.hostname;
  if (host !== '127.0.0.1' && host !== 'localhost' && host !== '::1' && host !== '[::1]') {
    return 'Use 127.0.0.1, localhost, or ::1. LAN endpoints are not in this release.';
  }
  if (url.port === '0') return 'Backend port cannot be zero.';
  const path = url.pathname.replace(/\/+$/, '');
  if (path !== '' && path !== '/v1') return 'Use the server’s /v1 URL.';
  return '';
}
