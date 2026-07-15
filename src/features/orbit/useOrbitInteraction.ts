import { useCallback, useEffect, useReducer, useRef } from 'react';
import { collapsedState, reduceOrbit, type OrbitState } from './orbit.machine';

export const HOVER_MS = 150;
export const LEAVE_MS = 200;
export const DRAG_THRESHOLD_PX = 6;

interface UseOrbitInteractionOptions {
  startDragging: () => Promise<void>;
  onExpandedChange: (expanded: boolean) => void;
  allowFlip?: boolean;
  dragThresholdPx?: number;
}

interface PointerOrigin {
  x: number;
  y: number;
}

interface OrbitHandlers {
  onClick(): void;
  onPointerDown(event: React.PointerEvent): void;
  onPointerEnter(): void;
  onPointerLeave(): void;
  onPointerMove(event: React.PointerEvent): void;
  onPointerUp(): void;
}

export interface OrbitInteraction {
  state: OrbitState;
  handlers: OrbitHandlers;
}

export function useOrbitInteraction({
  startDragging,
  onExpandedChange,
  allowFlip = true,
  dragThresholdPx = DRAG_THRESHOLD_PX,
}: UseOrbitInteractionOptions): OrbitInteraction {
  const [state, dispatch] = useReducer(reduceOrbit, collapsedState);
  const hoverTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const leaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pointerOrigin = useRef<PointerOrigin | null>(null);
  const dragged = useRef(false);
  const dragStarted = useRef(false);
  const suppressNextClick = useRef(false);
  const expandedRef = useRef(state.expanded);

  const clearHoverTimer = useCallback(() => {
    if (hoverTimer.current !== null) {
      clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
  }, []);

  const clearLeaveTimer = useCallback(() => {
    if (leaveTimer.current !== null) {
      clearTimeout(leaveTimer.current);
      leaveTimer.current = null;
    }
  }, []);

  const clearTimers = useCallback(() => {
    clearHoverTimer();
    clearLeaveTimer();
  }, [clearHoverTimer, clearLeaveTimer]);

  useEffect(() => {
    if (expandedRef.current !== state.expanded) {
      expandedRef.current = state.expanded;
      onExpandedChange(state.expanded);
    }
  }, [onExpandedChange, state.expanded]);

  useEffect(() => clearTimers, [clearTimers]);

  const flipIfEligible = useCallback(() => {
    if (allowFlip && expandedRef.current && !dragged.current) {
      dispatch({ type: 'click' });
      return true;
    }
    return false;
  }, [allowFlip]);

  return {
    state,
    handlers: {
      onClick() {
        if (suppressNextClick.current) {
          suppressNextClick.current = false;
          return;
        }
        flipIfEligible();
      },
      onPointerDown(event) {
        pointerOrigin.current = { x: event.clientX, y: event.clientY };
        dragged.current = false;
        dragStarted.current = false;
        suppressNextClick.current = false;
      },
      onPointerEnter() {
        clearLeaveTimer();
        if (!expandedRef.current) {
          clearHoverTimer();
          hoverTimer.current = setTimeout(() => {
            hoverTimer.current = null;
            dispatch({ type: 'hoverConfirmed' });
          }, HOVER_MS);
        }
      },
      onPointerLeave() {
        clearHoverTimer();
        clearLeaveTimer();
        leaveTimer.current = setTimeout(() => {
          leaveTimer.current = null;
          dispatch({ type: 'leaveExpired' });
        }, LEAVE_MS);
      },
      onPointerMove(event) {
        if (pointerOrigin.current === null || dragStarted.current) {
          return;
        }
        const distance = Math.hypot(
          event.clientX - pointerOrigin.current.x,
          event.clientY - pointerOrigin.current.y,
        );
        if (distance > dragThresholdPx) {
          dragged.current = true;
          dragStarted.current = true;
          clearTimers();
          void startDragging().catch(() => undefined);
        }
      },
      onPointerUp() {
        pointerOrigin.current = null;
        if (flipIfEligible()) {
          suppressNextClick.current = true;
        }
      },
    },
  };
}
