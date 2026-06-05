import { motion } from 'framer-motion';
import type { CSSProperties } from 'react';
import { cn } from '@/lib/utils';

export function BorderBeam({
  className,
  size = 180,
  duration = 12,
  delay = 0,
  colorFrom = '#a78bfa',
  colorTo = '#6366f1',
}: {
  className?: string;
  size?: number;
  duration?: number;
  delay?: number;
  colorFrom?: string;
  colorTo?: string;
}) {
  return (
    <div
      className={cn(
        'pointer-events-none absolute inset-0 rounded-[inherit] [border:1px_solid_transparent]',
        '![mask-clip:padding-box,border-box] ![mask-composite:intersect]',
        '[mask:linear-gradient(transparent,transparent),linear-gradient(#fff,#fff)]',
      )}
    >
      <motion.div
        className={cn('absolute aspect-square bg-gradient-to-l from-[var(--from)] via-[var(--to)] to-transparent', className)}
        style={{
          width: size,
          offsetPath: `rect(0 auto auto 0 round ${size}px)`,
          '--from': colorFrom,
          '--to': colorTo,
        } as CSSProperties}
        initial={{ offsetDistance: '0%' }}
        animate={{ offsetDistance: '100%' }}
        transition={{ repeat: Infinity, ease: 'linear', duration, delay: -delay }}
      />
    </div>
  );
}
