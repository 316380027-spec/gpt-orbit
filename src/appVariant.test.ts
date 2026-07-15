import { describe, expect, it } from 'vitest';
import { resolveAppVariant } from './appVariant';

describe('resolveAppVariant', () => {
  it('selects weekly only for the frozen weekly token', () => {
    expect(resolveAppVariant('weekly')).toBe('weekly');
    expect(resolveAppVariant('standard')).toBe('standard');
    expect(resolveAppVariant(undefined)).toBe('standard');
  });
});
