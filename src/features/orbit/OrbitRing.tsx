export function OrbitRing({
  percent,
  tone,
}: {
  percent: number | null;
  tone: 'ice' | 'violet';
}) {
  const value = percent === null ? 0 : Math.max(0, Math.min(100, percent));

  return (
    <span className="orbit-ring-anchor">
      <svg
        aria-label="剩余额度"
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={percent ?? undefined}
        className={`orbit-ring orbit-ring--${tone}`}
        role="progressbar"
        viewBox="0 0 72 72"
      >
        <circle className="orbit-ring__track" cx="36" cy="36" r="30" />
        <circle
          className="orbit-ring__value"
          cx="36"
          cy="36"
          pathLength="100"
          r="30"
          strokeDasharray="100"
          strokeDashoffset={100 - value}
        />
      </svg>
      <span className="orbit-ring__label">{percent === null ? '--%' : `${Math.round(percent)}%`}</span>
    </span>
  );
}
