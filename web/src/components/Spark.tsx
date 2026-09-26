/** Pixel bar spark (counts, never money). 4 units per bar, 12 high. */
export function Spark({ values, label }: { values: number[]; label: string }) {
  const max = Math.max(0, ...values);
  const width = Math.max(1, values.length) * 4;
  return (
    <svg className="spark" viewBox={`0 0 ${width} 12`} preserveAspectRatio="none" shapeRendering="crispEdges" role="img" aria-label={label}>
      {values.map((v, i) => {
        const h = v <= 0 || max === 0 ? 0 : Math.max(1, Math.round((v / max) * 12));
        return <rect key={i} className="spark-bar" x={i * 4} y={12 - h} width={4} height={h} />;
      })}
    </svg>
  );
}
