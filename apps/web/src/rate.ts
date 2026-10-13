// How often a gamepad's data changes: the browser's update rate for it.

/** The window the rate is counted over, in milliseconds. */
const WINDOW_MS = 1000;

/** Counts the changes of a timestamp over the last second. */
export class RateMeter {
  /** When each change was seen (`performance.now()`), oldest first. */
  private changes: number[] = [];
  private last: number | undefined;

  /** Records the timestamp read at `now`; a new value counts as a change. */
  sample(timestamp: number, now: number): void {
    if (timestamp !== this.last) {
      if (this.last !== undefined) this.changes.push(now);
      this.last = timestamp;
    }
    this.forget(now);
  }

  /** Changes per second, over the last second. */
  rate(now: number): number {
    this.forget(now);
    return this.changes.length;
  }

  private forget(now: number): void {
    let old = 0;
    while (old < this.changes.length && now - (this.changes[old] ?? now) > WINDOW_MS) old++;
    if (old > 0) this.changes.splice(0, old);
  }
}
