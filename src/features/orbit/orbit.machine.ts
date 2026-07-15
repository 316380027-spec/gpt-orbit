export interface OrbitState {
  expanded: boolean;
  face: 'front' | 'back';
}

export type OrbitEvent =
  | { type: 'hoverConfirmed' }
  | { type: 'click' }
  | { type: 'leaveExpired' };

export const collapsedState: OrbitState = { expanded: false, face: 'front' };

export function reduceOrbit(state: OrbitState, event: OrbitEvent): OrbitState {
  if (event.type === 'hoverConfirmed') {
    return { expanded: true, face: 'front' };
  }
  if (event.type === 'leaveExpired') {
    return collapsedState;
  }
  if (event.type === 'click' && state.expanded) {
    return { expanded: true, face: state.face === 'front' ? 'back' : 'front' };
  }
  return state;
}
