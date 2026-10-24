// Both bars and their numeric labels use the same confirmed, interpolated value.
export interface Channel {
  from: number;
  value: number;
  target: number;
  at: number;
}
export const channel = (): Channel => ({ from: 0, value: 0, target: 0, at: 0 });
export function advance(c: Channel, now: number) {
  c.value = Math.max(
    c.value,
    Math.min(
      c.target,
      c.from +
        (c.target - c.from) * Math.max(0, Math.min(1, (now - c.at) / 250)),
    ),
  );
  return c.value;
}
export function target(
  c: Channel,
  value: number,
  now: number,
  immediate = false,
) {
  advance(c, now);
  if (Number.isFinite(value) && value > c.target) {
    c.from = c.value;
    c.target = Math.min(100, Math.max(0, value));
    c.at = now;
  }
  if (immediate || c.target === 100) c.value = c.target;
}
