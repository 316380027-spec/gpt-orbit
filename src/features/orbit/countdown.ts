function remainingMs(resetsAt: number | null, now = Date.now()): number | null {
  if (resetsAt === null) {
    return null;
  }
  return Math.max(0, resetsAt * 1000 - now);
}

function pad2(value: number): string {
  return String(value).padStart(2, '0');
}

export function formatCompactCountdown(resetsAt: number | null, now = Date.now()): string {
  const remaining = remainingMs(resetsAt, now);
  if (remaining === null) {
    return '--:--';
  }
  const totalMinutes = Math.ceil(remaining / 60_000);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return `${pad2(hours)}:${pad2(minutes)}`;
}

export function formatWeeklyCompactCountdown(
  resetsAt: number | null,
  now = Date.now(),
): string {
  if (resetsAt === null || !Number.isFinite(resetsAt)) {
    return '--D --H';
  }
  const remaining = remainingMs(resetsAt, now);
  if (remaining === null || !Number.isFinite(remaining)) {
    return '--D --H';
  }
  const totalHours = Math.ceil(remaining / 3_600_000);
  const days = Math.floor(totalHours / 24);
  const hours = totalHours % 24;
  return `${days}D ${pad2(hours)}H`;
}

export function formatFiveHourReset(resetsAt: number | null, now = Date.now()): string {
  const remaining = remainingMs(resetsAt, now);
  if (remaining === null) {
    return '重置时间暂不可用';
  }
  const totalMinutes = Math.ceil(remaining / 60_000);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return `${hours} 小时 ${minutes} 分后重置`;
}

export function formatWeeklyReset(resetsAt: number | null, locale?: string): string {
  if (resetsAt === null) {
    return '周额度暂不可用';
  }
  const reset = new Date(resetsAt * 1000);
  const weekday = new Intl.DateTimeFormat(locale, { weekday: 'short' }).format(reset);
  return `${weekday} ${pad2(reset.getHours())}:${pad2(
    reset.getMinutes(),
  )} 重置`;
}
