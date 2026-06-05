import type { CSSProperties, ReactNode } from 'react';
import { cn } from '@/lib/utils';

export function AuroraText({
  children,
  className,
  colors = ['#a78bfa', '#6366f1', '#ec4899', '#8b5cf6'],
}: {
  children: ReactNode;
  className?: string;
  colors?: string[];
}) {
  const style: CSSProperties = {
    backgroundImage: `linear-gradient(135deg, ${colors.join(', ')}, ${colors[0]})`,
    WebkitBackgroundClip: 'text',
    WebkitTextFillColor: 'transparent',
    backgroundClip: 'text',
    color: 'transparent',
  };
  return (
    <span className={cn('relative inline-block bg-[length:200%_auto] animate-shine', className)} style={style}>
      {children}
    </span>
  );
}
