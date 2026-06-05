import { Sparkles } from 'lucide-react';
import { BlurFade } from '@/components/magic/blur-fade';

export default function Placeholder({ title }: { title: string }) {
  return (
    <BlurFade>
      <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-border py-24 text-center">
        <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-primary/10 text-primary">
          <Sparkles className="h-6 w-6" />
        </div>
        <h2 className="text-lg font-semibold">{title}</h2>
        <p className="mt-1 text-sm text-muted-foreground">该页面将在阶段二迁移到新界面</p>
      </div>
    </BlurFade>
  );
}
