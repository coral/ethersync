// Independent same-computer TOD reference. Never used to steer synchronization.
export function compareTod(frames: number, fps: number, wallUtcMs: number, timezoneOffsetMinutes: number) {
  const day = 86_400_000;
  const wrap = (n: number) => ((n % day) + day) % day;
  const wallTodMs = wrap(wallUtcMs - timezoneOffsetMinutes * 60_000);
  const timelineTodMs = wrap(frames / fps * 1000);
  return { wallTodMs, timelineTodMs, differenceMs: wrap(timelineTodMs - wallTodMs + day / 2) - day / 2 };
}
export interface WallReference {
  localMs: number; wallUtcMs: number; timezoneOffsetMinutes: number; bracketMs: number;
  frames: number; fps: number; speed: number; synchronization: string; discontinuity: string;
  differenceMs: number;
}
