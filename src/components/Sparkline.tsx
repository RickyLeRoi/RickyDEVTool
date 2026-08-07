import { SPARKLINE } from "../lib/styles";

interface SparklineProps {
  values: number[];
  max?: number;
  width?: number;
  height?: number;
  stroke?: string;
}

export function Sparkline({
  values,
  max = SPARKLINE.max,
  width = SPARKLINE.width,
  height = SPARKLINE.height,
  stroke = SPARKLINE.stroke,
}: SparklineProps) {
  if (values.length < 2) {
    return <svg width={width} height={height} className="sparkline" />;
  }
  const step = width / (values.length - 1);
  const usable = height - SPARKLINE.inset * 2;
  const points = values
    .map((v, i) => {
      const y = height - (Math.min(v, max) / max) * usable - SPARKLINE.inset;
      return `${(i * step).toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg width={width} height={height} className="sparkline">
      <polyline
        points={points}
        fill="none"
        stroke={stroke}
        strokeWidth={SPARKLINE.strokeWidth}
        strokeLinejoin="round"
      />
    </svg>
  );
}
