import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useOrbitInteraction } from './useOrbitInteraction';

function Harness({
  startDragging = vi.fn(async () => undefined),
  onExpandedChange = vi.fn(),
  allowFlip,
}: {
  startDragging?: () => Promise<void>;
  onExpandedChange?: (expanded: boolean) => void;
  allowFlip?: boolean;
}) {
  const orbit = useOrbitInteraction({ startDragging, onExpandedChange, allowFlip });

  return (
    <button
      data-expanded={String(orbit.state.expanded)}
      data-face={orbit.state.face}
      onClick={orbit.handlers.onClick}
      onPointerDown={orbit.handlers.onPointerDown}
      onPointerEnter={orbit.handlers.onPointerEnter}
      onPointerLeave={orbit.handlers.onPointerLeave}
      onPointerMove={orbit.handlers.onPointerMove}
      onPointerUp={orbit.handlers.onPointerUp}
      type="button"
    >
      orbit
    </button>
  );
}

function orbitButton() {
  return screen.getByRole('button', { name: 'orbit' });
}

function pointer(
  element: HTMLElement,
  type: 'pointerenter' | 'pointerleave' | 'pointerdown' | 'pointermove' | 'pointerup',
  init: MouseEventInit = {},
) {
  fireEvent(element, new MouseEvent(type, { bubbles: true, ...init }));
}

describe('useOrbitInteraction', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanup();
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it('waits 150ms before expanding and keeps hover on the front face', () => {
    const onExpandedChange = vi.fn();
    render(<Harness onExpandedChange={onExpandedChange} />);
    const button = orbitButton();

    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(149);
    });

    expect(button).toHaveAttribute('data-expanded', 'false');

    act(() => {
      vi.advanceTimersByTime(1);
    });

    expect(button).toHaveAttribute('data-expanded', 'true');
    expect(button).toHaveAttribute('data-face', 'front');
    expect(onExpandedChange).toHaveBeenLastCalledWith(true);
  });

  it('toggles weekly face by click only after expansion', () => {
    render(<Harness />);
    const button = orbitButton();

    fireEvent.click(button);

    expect(button).toHaveAttribute('data-expanded', 'false');
    expect(button).toHaveAttribute('data-face', 'front');

    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(150);
    });
    fireEvent.click(button);

    expect(button).toHaveAttribute('data-expanded', 'true');
    expect(button).toHaveAttribute('data-face', 'back');

    fireEvent.click(button);

    expect(button).toHaveAttribute('data-face', 'front');
  });

  it('never flips on click or pointer-up when flipping is disabled', () => {
    render(<Harness allowFlip={false} />);
    const button = orbitButton();

    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(150);
    });
    fireEvent.click(button);

    expect(button).toHaveAttribute('data-expanded', 'true');
    expect(button).toHaveAttribute('data-face', 'front');

    pointer(button, 'pointerdown', { clientX: 0, clientY: 0 });
    pointer(button, 'pointerup');

    expect(button).toHaveAttribute('data-expanded', 'true');
    expect(button).toHaveAttribute('data-face', 'front');
  });

  it('cancels a pending collapse when the pointer re-enters before 200ms', () => {
    render(<Harness />);
    const button = orbitButton();

    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(150);
    });
    fireEvent.click(button);
    fireEvent.pointerLeave(button);
    act(() => {
      vi.advanceTimersByTime(199);
    });
    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(1);
    });

    expect(button).toHaveAttribute('data-expanded', 'true');
    expect(button).toHaveAttribute('data-face', 'back');
  });

  it('collapses to the front face 200ms after pointer leave', () => {
    const onExpandedChange = vi.fn();
    render(<Harness onExpandedChange={onExpandedChange} />);
    const button = orbitButton();

    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(150);
    });
    fireEvent.click(button);
    fireEvent.pointerLeave(button);
    act(() => {
      vi.advanceTimersByTime(200);
    });

    expect(button).toHaveAttribute('data-expanded', 'false');
    expect(button).toHaveAttribute('data-face', 'front');
    expect(onExpandedChange).toHaveBeenLastCalledWith(false);
  });

  it('keeps a 6px move click eligible and starts drag once above 6px', async () => {
    const startDragging = vi.fn(async () => undefined);
    render(<Harness startDragging={startDragging} />);
    const button = orbitButton();

    fireEvent.pointerEnter(button);
    act(() => {
      vi.advanceTimersByTime(150);
    });
    pointer(button, 'pointerdown', { clientX: 0, clientY: 0 });
    pointer(button, 'pointermove', { clientX: 6, clientY: 0 });
    pointer(button, 'pointerup');

    expect(startDragging).not.toHaveBeenCalled();
    expect(button).toHaveAttribute('data-face', 'back');

    pointer(button, 'pointerdown', { clientX: 0, clientY: 0 });
    pointer(button, 'pointermove', { clientX: 7, clientY: 0 });
    pointer(button, 'pointermove', { clientX: 8, clientY: 0 });
    pointer(button, 'pointerup');

    expect(startDragging).toHaveBeenCalledTimes(1);
    expect(button).toHaveAttribute('data-face', 'back');
  });

  it('clears timers on unmount', () => {
    const { unmount } = render(<Harness />);
    const button = orbitButton();

    fireEvent.pointerEnter(button);
    fireEvent.pointerLeave(button);
    unmount();

    expect(vi.getTimerCount()).toBe(0);
  });
});
