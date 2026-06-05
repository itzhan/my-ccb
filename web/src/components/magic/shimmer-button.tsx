import React, { type CSSProperties } from 'react';
import { cn } from '@/lib/utils';

export interface ShimmerButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  shimmerColor?: string;
  shimmerSize?: string;
  borderRadius?: string;
  shimmerDuration?: string;
  background?: string;
}

export const ShimmerButton = React.forwardRef<HTMLButtonElement, ShimmerButtonProps>(
  (
    {
      shimmerColor = '#ffffff',
      shimmerSize = '0.05em',
      shimmerDuration = '3s',
      borderRadius = '12px',
      background = 'linear-gradient(135deg, #6366f1, #a855f7)',
      className,
      children,
      ...props
    },
    ref,
  ) => {
    return (
      <button
        ref={ref}
        style={
          {
            '--spread': '90deg',
            '--shimmer-color': shimmerColor,
            '--radius': borderRadius,
            '--speed': shimmerDuration,
            '--cut': shimmerSize,
            '--bg': background,
          } as CSSProperties
        }
        className={cn(
          'group relative z-0 flex cursor-pointer items-center justify-center overflow-hidden whitespace-nowrap border border-white/10 px-6 text-white',
          '[background:var(--bg)] [border-radius:var(--radius)] transition-transform active:translate-y-px disabled:opacity-60',
          className,
        )}
        {...props}
      >
        <div className="-z-30 blur-[2px] absolute inset-0 overflow-visible [container-type:size]">
          <div className="absolute inset-0 h-[100cqh] animate-shimmer-slide [aspect-ratio:1] [mask:none]">
            <div className="absolute -inset-full w-auto rotate-0 animate-spin-around [background:conic-gradient(from_calc(270deg-(var(--spread)*0.5)),transparent_0,var(--shimmer-color)_var(--spread),transparent_var(--spread))] [translate:0_0]" />
          </div>
        </div>
        <span className="relative z-10">{children}</span>
        <div className="absolute size-full rounded-[inherit] [box-shadow:inset_0_-8px_10px_#ffffff1f] transform-gpu transition-all duration-300 ease-in-out group-hover:[box-shadow:inset_0_-6px_10px_#ffffff3f] group-active:[box-shadow:inset_0_-10px_10px_#ffffff3f]" />
        <div className="absolute -z-20 [background:var(--bg)] [border-radius:var(--radius)] [inset:var(--cut)]" />
      </button>
    );
  },
);
ShimmerButton.displayName = 'ShimmerButton';
