<script setup lang="ts">
// 移植自 Inspira UI (MagicUI 的 Vue 版)的 Aurora Background，配色调整为本项目品牌暖色。
import { cn } from "@/lib/utils";
import { computed } from "vue";

interface AuroraBackgroundProps {
  radialGradient?: boolean;
  class?: string;
}

const props = withDefaults(defineProps<AuroraBackgroundProps>(), {
  radialGradient: true,
});

const styles = computed(() => {
  return {
    "--aurora":
      "repeating-linear-gradient(100deg,#c4704f_10%,#e0a878_15%,#d98e63_20%,#f0d9c0_25%,#c4704f_30%)",
    "--white-gradient":
      "repeating-linear-gradient(100deg,#fff_0%,#fff_7%,transparent_10%,transparent_12%,#fff_16%)",
    "--terracotta": "#c4704f",
    "--amber": "#e0a878",
    "--sand": "#d98e63",
    "--cream": "#f0d9c0",
    "--white": "#fff",
    "--transparent": "transparent",
    "--animate-aurora": "aurora 60s linear infinite",
  };
});
</script>

<template>
  <div
    :class="
      cn(
        `relative flex min-h-screen flex-col items-center justify-center bg-[#f9f6f1] text-[#29261e]`,
        props.class,
      )
    "
  >
    <div :style="styles" class="absolute inset-0 overflow-hidden">
      <div
        :class="
          cn(
            `after:animate-aurora pointer-events-none absolute -inset-[10px] [background-image:var(--white-gradient),var(--aurora)] [background-size:300%,_200%] [background-position:50%_50%,50%_50%] opacity-40 blur-[10px] will-change-transform [--aurora:repeating-linear-gradient(100deg,var(--terracotta)_10%,var(--amber)_15%,var(--sand)_20%,var(--cream)_25%,var(--terracotta)_30%)] [--white-gradient:repeating-linear-gradient(100deg,var(--white)_0%,var(--white)_7%,var(--transparent)_10%,var(--transparent)_12%,var(--white)_16%)] after:absolute after:inset-0 after:[background-image:var(--white-gradient),var(--aurora)] after:[background-size:200%,_100%] after:[background-attachment:fixed] after:mix-blend-soft-light after:content-[\'\']`,
            props.radialGradient &&
              `[mask-image:radial-gradient(ellipse_at_100%_0%,black_10%,var(--transparent)_70%)]`,
          )
        "
      />
    </div>
    <slot />
  </div>
</template>
