<script setup lang="ts">
import { ref } from 'vue';
import { Input } from '@/components/ui/input';
import AuroraBackground from '@/components/inspira/AuroraBackground.vue';
import { login } from '../router';

const password = ref('');
const error = ref('');
const loading = ref(false);

async function submit() {
  if (!password.value.trim()) {
    error.value = '请输入密码';
    return;
  }
  error.value = '';
  loading.value = true;
  try {
    await login(password.value.trim());
  } catch {
    error.value = '密码错误';
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <AuroraBackground>
    <div class="relative z-10 w-full max-w-sm px-4">
      <div
        class="rounded-3xl border border-white/50 bg-white/70 backdrop-blur-xl shadow-2xl shadow-[#c4704f]/10 p-8"
      >
        <!-- 品牌头部 -->
        <div class="flex flex-col items-center mb-7">
          <div
            class="w-16 h-16 rounded-2xl bg-gradient-to-br from-[#c4704f] to-[#e0a878] flex items-center justify-center shadow-lg shadow-[#c4704f]/30 mb-4 ring-1 ring-white/40"
          >
            <img src="/favicon.svg" alt="Logo" class="w-9 h-9" />
          </div>
          <h1 class="text-2xl font-semibold tracking-tight text-[#29261e]">Claude Code Gateway</h1>
          <p class="text-sm text-[#8c8475] mt-1.5">管理控制台</p>
        </div>

        <form @submit.prevent="submit" class="space-y-4">
          <Input
            v-model="password"
            type="password"
            placeholder="管理员密码"
            class="bg-white/60 border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 h-11 rounded-xl"
          />
          <p v-if="error" class="text-red-500 text-sm text-center">{{ error }}</p>

          <!-- 渐变 + 微光按钮 -->
          <button
            type="submit"
            :disabled="loading"
            class="group relative w-full h-11 overflow-hidden rounded-xl bg-gradient-to-r from-[#c4704f] to-[#b5623f] text-white font-medium shadow-lg shadow-[#c4704f]/25 transition-all duration-200 hover:shadow-xl hover:shadow-[#c4704f]/35 disabled:opacity-60"
          >
            <span
              class="absolute inset-0 -translate-x-full bg-gradient-to-r from-transparent via-white/30 to-transparent transition-transform duration-700 group-hover:translate-x-full"
            />
            <span class="relative">{{ loading ? '登录中…' : '登录' }}</span>
          </button>
        </form>

        <p class="text-center text-xs text-[#b5b0a6] mt-6">
          多账号 · 负载均衡 · 指纹对齐
        </p>
      </div>
    </div>
  </AuroraBackground>
</template>
