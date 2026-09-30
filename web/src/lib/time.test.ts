import { describe, expect, it } from 'vitest';
import { fmtAge, fmtDuration, fmtUtcDateTime, fmtUtcDayHm, fmtUtcDayMonth, fmtUtcHms, fmtUtcTime, isoDay, parseIsoDay } from './time';

describe('time', () => {
  const t = Date.UTC(2026, 8, 25, 1, 12, 4, 210);
  it('formats durations like the spec (4.19 s)', () => {
    expect(fmtDuration(4190)).toBe('4.19 s');
    expect(fmtDuration(84)).toBe('0.08 s');
    expect(fmtDuration(125_000)).toBe('2 min 5 s');
    expect(fmtDuration(7_500_000)).toBe('2 h 5 min');
    expect(fmtDuration(-5)).toBe('0.00 s');
  });
  it('ticks ages as mm:ss.d', () => {
    expect(fmtAge(4200)).toBe('00:04.2');
    expect(fmtAge(3_725_000)).toBe('62:05.0');
    expect(fmtAge(-1)).toBe('00:00.0');
  });
  it('shows UTC and parses days', () => {
    expect(fmtUtcTime(t)).toBe('01:12:04.210 UTC');
    expect(fmtUtcDateTime(t)).toBe('2026-09-25 01:12:04 UTC');
    expect(isoDay(t)).toBe('2026-09-25');
    expect(parseIsoDay('2026-09-25')).toBe(Date.UTC(2026, 8, 25));
    expect(parseIsoDay('25/09/2026')).toBeNull();
  });
  it('formats a UTC clock time and a day heading', () => {
    const t = Date.UTC(2026, 8, 27, 17, 32, 11, 450);
    expect(fmtUtcHms(t)).toBe('17:32:11');
    expect(fmtUtcDayMonth(t)).toBe('27 SEP');
  });

  it('writes a UTC day and minute for epoch starts', () => {
    expect(fmtUtcDayHm(Date.UTC(2026, 8, 29, 19, 58, 12))).toBe('2026-09-29 19:58 UTC');
  });
});
