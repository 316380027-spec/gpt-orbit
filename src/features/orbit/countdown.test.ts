import { describe, expect, it, vi } from 'vitest';
import {
  formatCompactCountdown,
  formatFiveHourReset,
  formatWeeklyCompactCountdown,
  formatWeeklyReset,
} from './countdown';

const now = new Date(2026, 6, 12, 7, 12, 0).getTime();

describe('countdown formatters', () => {
  it('formats compact countdowns as HH:MM without going negative', () => {
    expect(formatCompactCountdown(now / 1000 + 2 * 60 * 60 + 18 * 60, now)).toBe('02:18');
    expect(formatCompactCountdown(now / 1000 - 60, now)).toBe('00:00');
    expect(formatCompactCountdown(null, now)).toBe('--:--');
  });

  it('formats localized five-hour reset copy', () => {
    expect(formatFiveHourReset(now / 1000 + 2 * 60 * 60 + 18 * 60, now)).toBe(
      '2 小时 18 分后重置',
    );
  });

  it('formats weekly reset as weekday and 24-hour time', () => {
    const mondayReset = new Date(2026, 6, 13, 9, 30, 0).getTime() / 1000;

    expect(formatWeeklyReset(mondayReset, 'zh-CN')).toBe('周一 09:30 重置');
    expect(formatWeeklyReset(null, 'zh-CN')).toBe('周额度暂不可用');
  });

  it('defaults weekly reset copy to Chinese independently of the OS locale', () => {
    const mondayReset = new Date(2026, 6, 13, 9, 30, 0).getTime() / 1000;
    const formatter = vi.spyOn(Intl, 'DateTimeFormat');

    try {
      formatWeeklyReset(mondayReset);
      expect(formatter).toHaveBeenCalledWith('zh-CN', { weekday: 'short' });
    } finally {
      formatter.mockRestore();
    }
  });

  it('formats weekly compact countdowns as rounded-up days and hours', () => {
    expect(formatWeeklyCompactCountdown(now / 1000 + 26 * 60 * 60 + 18 * 60, now)).toBe(
      '1D 03H',
    );
    expect(formatWeeklyCompactCountdown(now / 1000 - 60, now)).toBe('0D 00H');
    expect(formatWeeklyCompactCountdown(null, now)).toBe('--D --H');
    expect(formatWeeklyCompactCountdown(Number.NaN, now)).toBe('--D --H');
  });
});
