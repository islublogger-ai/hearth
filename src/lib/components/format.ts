import type { ModelInfo } from '../types';

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return 'unknown';
  const gib = bytes / 1024 ** 3;
  if (gib >= 10) return `${Math.round(gib)} GB`;
  if (gib >= 1) return `${gib.toFixed(1)} GB`;
  return `${Math.max(1, Math.round(bytes / 1024 ** 2))} MB`;
}

/** Never returns green. A KV size is not available, so a comfortable weight estimate stays unknown. */
export function fitLabel(sizeBytes: number | null, ramBytes: number): { level: 'red' | 'amber' | 'unknown'; text: string } {
  if (!ramBytes) return { level: 'unknown', text: 'Fit unknown — memory size was not reported' };
  if (sizeBytes == null || sizeBytes <= 0) return { level: 'unknown', text: 'Fit unknown — weights were not reported' };
  const reserve = ramBytes <= 12 * 1024 ** 3 ? Math.max(3 * 1024 ** 3, ramBytes * 0.45) : 6 * 1024 ** 3;
  const available = Math.max(0, ramBytes - reserve);
  if (sizeBytes > available) return { level: 'red', text: `Does not fit in ${formatBytes(ramBytes)}` };
  if (sizeBytes > available * 0.6) return { level: 'amber', text: `Tight on ${formatBytes(ramBytes)}` };
  return { level: 'unknown', text: 'KV shape unknown' };
}

export function loadedLabel(model: Pick<ModelInfo, 'loaded'>): string {
  if (model.loaded === true) return 'loaded';
  if (model.loaded === false) return 'not loaded';
  return 'loaded state unknown';
}

export function hardwareLine(chip: string, ramBytes: number, gpuBytes: number | null): string {
  const name = chip.trim() || 'This Mac';
  const ram = ramBytes > 0 ? formatBytes(ramBytes) : 'memory unknown';
  const gpu = gpuBytes && gpuBytes > 0 ? `GPU wired limit ${formatBytes(gpuBytes)}` : 'GPU wired limit unset';
  return `${name} · ${ram} · ${gpu}`;
}
