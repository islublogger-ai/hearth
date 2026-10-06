export function sendsOnEnter(
  event: { key: string; isComposing: boolean; shiftKey: boolean; metaKey: boolean; ctrlKey: boolean },
  sendKey: 'enter' | 'cmd_enter',
): boolean {
  if (event.key !== 'Enter' || event.isComposing || event.shiftKey) return false;
  if (sendKey === 'cmd_enter') return event.metaKey || event.ctrlKey;
  return !event.metaKey && !event.ctrlKey;
}
