interface SparklineProps {
  values: number[];
  max?: number;
  width?: number;
  height?: number;
  stroke?: string;
}

export function Sparkline({
  values,
  max = 100,
  width = 180,
  height = 28,
  stroke = "var(--accent)",
}: SparklineProps) {
  if (values.length < 2) {
    return <svg width={width} height={height} className="sparkline" />;
  }
  const step = width / (values.length - 1);
  const points = values
    .map((v, i) => {
      const y = height - (Math.min(v, max) / max) * (height - 2) - 1;
      return `${(i * step).toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg width={width} height={height} className="sparkline">
      <polyline
        points={points}
        fill="none"
        stroke={stroke}
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
    </svg>
  );
}
