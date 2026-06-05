import { useEffect, useRef } from 'react';
import { useInView, useMotionValue, useSpring } from 'framer-motion';
import { cn } from '@/lib/utils';

export function NumberTicker({
  value,
  className,
  decimalPlaces = 0,
}: {
  value: number;
  className?: string;
  decimalPlaces?: number;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const mv = useMotionValue(0);
  const spring = useSpring(mv, { damping: 32, stiffness: 130 });
  const inView = useInView(ref, { once: true });

  useEffect(() => {
    if (inView) mv.set(value);
  }, [inView, value, mv]);

  useEffect(() => {
    return spring.on('change', (latest) => {
      if (ref.current) {
        ref.current.textContent = Intl.NumberFormat('en-US', {
          minimumFractionDigits: decimalPlaces,
          maximumFractionDigits: decimalPlaces,
        }).format(Number(latest.toFixed(decimalPlaces)));
      }
    });
  }, [spring, decimalPlaces]);

  return <span ref={ref} className={cn('inline-block tabular-nums', className)}>0</span>;
}
