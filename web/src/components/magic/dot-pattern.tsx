import { useId } from 'react';
import { cn } from '@/lib/utils';

export function DotPattern({
  width = 22,
  height = 22,
  cr = 1,
  className,
}: {
  width?: number;
  height?: number;
  cr?: number;
  className?: string;
}) {
  const id = useId();
  return (
    <svg
      aria-hidden
      className={cn('pointer-events-none absolute inset-0 h-full w-full fill-white/[0.12]', className)}
    >
      <defs>
        <pattern id={id} width={width} height={height} patternUnits="userSpaceOnUse" patternContentUnits="userSpaceOnUse">
          <circle cx={cr} cy={cr} r={cr} />
        </pattern>
      </defs>
      <rect width="100%" height="100%" fill={`url(#${id})`} />
    </svg>
  );
}
