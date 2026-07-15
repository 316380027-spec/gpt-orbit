import { describe, expect, it } from 'vitest';
import { collapsedState, reduceOrbit } from './orbit.machine';

describe('orbit machine', () => {
  it('expands to the front face after hover confirmation', () => {
    expect(reduceOrbit(collapsedState, { type: 'hoverConfirmed' })).toEqual({
      expanded: true,
      face: 'front',
    });
  });

  it('toggles faces only while expanded', () => {
    expect(reduceOrbit(collapsedState, { type: 'click' })).toBe(collapsedState);
    expect(reduceOrbit({ expanded: true, face: 'front' }, { type: 'click' })).toEqual({
      expanded: true,
      face: 'back',
    });
    expect(reduceOrbit({ expanded: true, face: 'back' }, { type: 'click' })).toEqual({
      expanded: true,
      face: 'front',
    });
  });

  it('collapses and restores the front face after leave expiry', () => {
    expect(reduceOrbit({ expanded: true, face: 'back' }, { type: 'leaveExpired' })).toEqual({
      expanded: false,
      face: 'front',
    });
  });
});
