import { useRef, type ReactNode } from 'react';
import { motion, useInView } from 'framer-motion';

export function BlurFade({
  children,
  className,
  delay = 0,
  duration = 0.4,
  yOffset = 6,
}: {
  children: ReactNode;
  className?: string;
  delay?: number;
  duration?: number;
  yOffset?: number;
}) {
  const ref = useRef(null);
  const inView = useInView(ref, { once: true, margin: '-40px' });
  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, filter: 'blur(6px)', y: yOffset }}
      animate={inView ? { opacity: 1, filter: 'blur(0px)', y: 0 } : {}}
      transition={{ delay, duration, ease: 'easeOut' }}
      className={className}
    >
      {children}
    </motion.div>
  );
}
