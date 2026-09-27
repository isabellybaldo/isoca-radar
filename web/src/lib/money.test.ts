import { describe, expect, it, vi } from 'vitest';
import { compareDecimal, formatDecimal, formatMoney, isZero, signOf } from './money';

describe('formatMoney', () => {
  it('never formats money through a float', () => {
    const spies = [
      vi.spyOn(Number.prototype, 'toFixed'),
      vi.spyOn(Number.prototype, 'toLocaleString'),
      vi.spyOn(Intl, 'NumberFormat'),
      vi.spyOn(globalThis, 'parseFloat'),
      vi.spyOn(Math, 'round'),
    ];
    const out = ['12345678901234567.89', '0.000001', '0.1', '-99.84', '1000', '0'].map((v) => formatMoney(v));
    expect(out).toEqual(['$12,345,678,901,234,567.89', '$0.000001', '$0.10', '−$99.84', '$1,000.00', '$0.00']);
    for (const s of spies) expect(s).not.toHaveBeenCalled();
  });

  it('signs gains, keeps zero unsigned, and shows null as not measured', () => {
    expect(formatMoney('0.41', { signed: true })).toBe('+$0.41');
    expect(formatMoney('-0.12', { signed: true })).toBe('−$0.12');
    expect(formatMoney('0', { signed: true })).toBe('$0.00');
    expect(formatMoney('-0', { signed: true })).toBe('$0.00');
    expect(formatMoney(null)).toBe('—');
    expect(formatMoney('garbage')).toBe('garbage');
  });

  it('formats other decimals as sent, with grouping', () => {
    expect(formatDecimal('83')).toBe('83');
    expect(formatDecimal('-0.0008')).toBe('−0.0008');
    expect(formatDecimal('1234567.5')).toBe('1,234,567.5');
    expect(formatDecimal(undefined)).toBe('—');
    expect([signOf('1'), signOf('-1'), signOf('0.000'), signOf(null)]).toEqual([1, -1, 0, 0]);
    expect(isZero('0.00')).toBe(true);
  });
});

describe('compareDecimal', () => {
  it('orders decimal strings exactly, without floats', () => {
    expect(compareDecimal('0.4', '1.00')).toBe(-1);
    expect(compareDecimal('1', '1.00')).toBe(0);
    expect(compareDecimal('1.000001', '1')).toBe(1);
    expect(compareDecimal('-2', '1')).toBe(-1);
    expect(compareDecimal('-0.5', '-0.25')).toBe(-1);
    expect(compareDecimal('12345678901234567.89', '12345678901234567.88')).toBe(1);
  });
});
