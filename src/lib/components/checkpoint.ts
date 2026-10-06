/** One write per interval while updates keep arriving. Resetting the timer would skip a live stream. */
export function createCheckpoint(save: (id: string) => void, intervalMs = 400) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: string | null = null;
  const fire = () => {
    timer = undefined;
    const id = pending;
    pending = null;
    if (id) save(id);
  };
  return {
    schedule(id: string) {
      pending = id;
      if (timer !== undefined) return;
      timer = setTimeout(fire, intervalMs);
    },
    flush() {
      if (timer === undefined && pending === null) return;
      clearTimeout(timer);
      fire();
    },
  };
}
