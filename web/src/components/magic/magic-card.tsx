import { type ReactNode, type MouseEvent } from 'react';
import { motion, useMotionTemplate, useMotionValue } from 'framer-motion';
import { cn } from '@/lib/utils';

export function MagicCard({
  children,
  className,
  gradientColor = 'hsl(252 90% 67% / 0.14)',
  gradientSize = 240,
}: {
  children: ReactNode;
  className?: string;
  gradientColor?: string;
  gradientSize?: number;
}) {
  const mx = useMotionValue(-gradientSize);
  const my = useMotionValue(-gradientSize);

  function onMove(e: MouseEvent<HTMLDivElement>) {
    const r = e.currentTarget.getBoundingClientRect();
    mx.set(e.clientX - r.left);
    my.set(e.clientY - r.top);
  }

  const bg = useMotionTemplate`radial-gradient(${gradientSize}px circle at ${mx}px ${my}px, ${gradientColor}, transparent 70%)`;

  return (
    <div
      onMouseMove={onMove}
      className={cn('group relative overflow-hidden rounded-xl border border-border bg-card', className)}
    >
      <motion.div
        className="pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-300 group-hover:opacity-100"
        style={{ background: bg }}
      />
      <div className="relative">{children}</div>
    </div>
  );
}
