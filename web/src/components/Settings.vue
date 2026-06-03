<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { api } from '../api';
import { useToast } from '../composables/useToast';

const { show: toast } = useToast();

const clientRestriction = ref<'off' | 'ua' | 'strict'>('off');
const loading = ref(false);
const saving = ref(false);

const options: { value: 'off' | 'ua' | 'strict'; title: string; desc: string }[] = [
  { value: 'off', title: '关闭', desc: '不限制。任何带有效令牌的客户端都能访问（普通 API 客户端会被伪装成 CC 转发）。' },
  { value: 'ua', title: '仅校验 UA', desc: '只检查 User-Agent 是 claude-code / claude-cli。宽松，可被伪造。' },
  { value: 'strict', title: '严格', desc: 'UA + 系统提示相似度 + 必需 header，只放行真实 Claude Code 客户端。' },
];

async function load() {
  loading.value = true;
  try {
    const s = await api.getSettings();
    clientRestriction.value = (s.client_restriction as 'off' | 'ua' | 'strict') || 'off';
  } catch {
    /* ignore */
  }
  loading.value = false;
}

async function save() {
  saving.value = true;
  try {
    const s = await api.updateSettings({ client_restriction: clientRestriction.value });
    clientRestriction.value = (s.client_restriction as 'off' | 'ua' | 'strict') || 'off';
    toast('已保存，立即生效');
  } catch (e: unknown) {
    toast((e as Error).message || '保存失败');
  }
  saving.value = false;
}

onMounted(load);
</script>

<template>
  <div class="space-y-6">
    <div>
      <h2 class="text-lg font-semibold text-[#29261e]">全局设置</h2>
      <p class="text-sm text-[#8c8475] mt-1">这些设置立即生效、持久化保存，无需重启。</p>
    </div>

    <!-- 客户端访问限制 -->
    <div class="rounded-2xl border border-[#e8e2d9] bg-white p-6 shadow-sm shadow-black/5 space-y-4">
      <div>
        <h3 class="text-base font-medium text-[#29261e]">限制客户端访问</h3>
        <p class="text-sm text-[#8c8475] mt-1">控制只允许哪类客户端调用网关。</p>
      </div>

      <div class="grid gap-3">
        <button
          v-for="opt in options"
          :key="opt.value"
          type="button"
          @click="clientRestriction = opt.value"
          class="text-left rounded-xl border p-4 transition-all"
          :class="clientRestriction === opt.value
            ? 'border-[#c4704f] bg-[#c4704f]/5 ring-1 ring-[#c4704f]/30'
            : 'border-[#e8e2d9] bg-[#f9f6f1] hover:border-[#c4704f]/40'"
        >
          <div class="flex items-center gap-2">
            <span
              class="w-4 h-4 rounded-full border-2 flex items-center justify-center"
              :class="clientRestriction === opt.value ? 'border-[#c4704f]' : 'border-[#d6cdbf]'"
            >
              <span v-if="clientRestriction === opt.value" class="w-2 h-2 rounded-full bg-[#c4704f]" />
            </span>
            <span class="text-sm font-medium text-[#29261e]">{{ opt.title }}</span>
            <span v-if="opt.value === 'strict'" class="text-[10px] px-1.5 py-0.5 rounded bg-emerald-50 text-emerald-600 border border-emerald-200">推荐</span>
          </div>
          <p class="text-xs text-[#8c8475] mt-1.5 ml-6">{{ opt.desc }}</p>
        </button>
      </div>

      <div class="flex items-center justify-between pt-2">
        <p class="text-xs text-[#b5b0a6]">
          ⚠️ UA/header 可被伪造，这不是安全边界；真正的访问控制靠令牌。
        </p>
        <button
          @click="save"
          :disabled="saving || loading"
          class="px-5 h-10 rounded-xl bg-gradient-to-r from-[#c4704f] to-[#b5623f] text-white text-sm font-medium shadow-lg shadow-[#c4704f]/25 hover:shadow-xl transition-all disabled:opacity-60"
        >
          {{ saving ? '保存中…' : '保存' }}
        </button>
      </div>
    </div>
  </div>
</template>
