<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { api, type UsageLog, type UsageStat, type Account, type ApiToken } from '../api';
import { Card } from '@/components/ui/card';
import { Button } from '@/components/ui/button';

/** 明细列表 */
const logs = ref<UsageLog[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = 50;
const loading = ref(false);

/** 汇总（今日 / 累计） */
const todayStat = ref<UsageStat | null>(null);
const allStat = ref<UsageStat | null>(null);

/** 过滤项 */
const accounts = ref<Account[]>([]);
const tokens = ref<ApiToken[]>([]);
const filterAccount = ref<number | ''>('');
const filterToken = ref<number | ''>('');
const filterModel = ref('');
const filterResult = ref('');
const filterStart = ref('');
const filterEnd = ref('');
const models = ref<string[]>([]);

const accountName = (id: number) => accounts.value.find(a => a.id === id)?.name || (id ? `#${id}` : '-');
const tokenName = (id: number) => tokens.value.find(t => t.id === id)?.name || (id ? `#${id}` : '-');

function fmt(n: number): string {
  return (n ?? 0).toLocaleString('en-US');
}

const totalPages = computed(() => Math.max(1, Math.ceil(total.value / pageSize)));

/** 今日 UTC 日期（与后端 usage_daily.day 一致） */
function todayUtc(): string {
  return new Date().toISOString().slice(0, 10);
}

async function loadStats() {
  try {
    const [t, a] = await Promise.all([
      api.getUsageStats({ group_by: 'total', start: todayUtc(), end: todayUtc() }),
      api.getUsageStats({ group_by: 'total' }),
    ]);
    todayStat.value = t.data?.[0] ?? null;
    allStat.value = a.data?.[0] ?? null;
  } catch {
    todayStat.value = null;
    allStat.value = null;
  }
}

async function loadLogs() {
  loading.value = true;
  try {
    const res = await api.getUsageLogs({
      page: page.value,
      page_size: pageSize,
      account_id: filterAccount.value === '' ? undefined : Number(filterAccount.value),
      token_id: filterToken.value === '' ? undefined : Number(filterToken.value),
      model: filterModel.value || undefined,
      result: filterResult.value || undefined,
      start: filterStart.value ? `${filterStart.value}T00:00:00Z` : undefined,
      end: filterEnd.value ? `${filterEnd.value}T23:59:59Z` : undefined,
    });
    logs.value = res.data ?? [];
    total.value = res.total ?? 0;
  } catch {
    logs.value = [];
    total.value = 0;
  } finally {
    loading.value = false;
  }
}

function applyFilter() {
  page.value = 1;
  loadLogs();
}

function resetFilter() {
  filterAccount.value = '';
  filterToken.value = '';
  filterModel.value = '';
  filterResult.value = '';
  filterStart.value = '';
  filterEnd.value = '';
  page.value = 1;
  loadLogs();
}

function go(p: number) {
  if (p < 1 || p > totalPages.value) return;
  page.value = p;
  loadLogs();
}

/** 展开的明细行 id */
const expanded = ref<number | null>(null);
function toggle(id: number) {
  expanded.value = expanded.value === id ? null : id;
}

/** 把 JSON 字符串美化；解析失败则原样返回 */
function pretty(s: string): string {
  if (!s) return '';
  try {
    return JSON.stringify(JSON.parse(s), null, 2);
  } catch {
    return s;
  }
}

async function loadModels() {
  try {
    const res = await api.getUsageStats({ group_by: 'model' });
    models.value = (res.data ?? []).map(d => d.key).filter(Boolean);
  } catch {
    models.value = [];
  }
}

onMounted(async () => {
  try {
    const [accRes, tokRes] = await Promise.all([api.listAccounts(1, 200), api.listTokens(1, 200)]);
    accounts.value = accRes.data ?? [];
    tokens.value = tokRes.data ?? [];
  } catch { /* ignore */ }
  loadModels();
  loadStats();
  loadLogs();
});
</script>

<template>
  <div class="space-y-6">
    <!-- 汇总卡 -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
      <Card class="p-5 border-[#e7e0d6]">
        <p class="text-[#8c8475] text-xs mb-3 font-medium">今日用量 (UTC)</p>
        <div class="grid grid-cols-4 gap-3 text-center">
          <div><p class="text-lg font-semibold text-[#29261e]">{{ fmt(todayStat?.input_tokens || 0) }}</p><p class="text-[11px] text-[#8c8475]">输入</p></div>
          <div><p class="text-lg font-semibold text-[#29261e]">{{ fmt(todayStat?.output_tokens || 0) }}</p><p class="text-[11px] text-[#8c8475]">输出</p></div>
          <div><p class="text-lg font-semibold text-[#c4704f]">{{ fmt(todayStat?.cache_read_tokens || 0) }}</p><p class="text-[11px] text-[#8c8475]">缓存读</p></div>
          <div><p class="text-lg font-semibold text-[#c4704f]">{{ fmt(todayStat?.cache_creation_tokens || 0) }}</p><p class="text-[11px] text-[#8c8475]">缓存创建</p></div>
        </div>
        <p class="text-[11px] text-[#8c8475] mt-3 text-center">{{ fmt(todayStat?.req_count || 0) }} 次调用</p>
      </Card>
      <Card class="p-5 border-[#e7e0d6]">
        <p class="text-[#8c8475] text-xs mb-3 font-medium">累计用量</p>
        <div class="grid grid-cols-4 gap-3 text-center">
          <div><p class="text-lg font-semibold text-[#29261e]">{{ fmt(allStat?.input_tokens || 0) }}</p><p class="text-[11px] text-[#8c8475]">输入</p></div>
          <div><p class="text-lg font-semibold text-[#29261e]">{{ fmt(allStat?.output_tokens || 0) }}</p><p class="text-[11px] text-[#8c8475]">输出</p></div>
          <div><p class="text-lg font-semibold text-[#c4704f]">{{ fmt(allStat?.cache_read_tokens || 0) }}</p><p class="text-[11px] text-[#8c8475]">缓存读</p></div>
          <div><p class="text-lg font-semibold text-[#c4704f]">{{ fmt(allStat?.cache_creation_tokens || 0) }}</p><p class="text-[11px] text-[#8c8475]">缓存创建</p></div>
        </div>
        <p class="text-[11px] text-[#8c8475] mt-3 text-center">{{ fmt(allStat?.req_count || 0) }} 次调用</p>
      </Card>
    </div>

    <!-- 过滤 -->
    <Card class="p-4 border-[#e7e0d6]">
      <div class="flex flex-wrap items-end gap-3">
        <div>
          <label class="block text-xs text-[#8c8475] mb-1">账号</label>
          <select v-model="filterAccount" @change="applyFilter" class="h-9 px-2 rounded-lg border border-[#e7e0d6] bg-white text-sm text-[#29261e]">
            <option value="">全部</option>
            <option v-for="a in accounts" :key="a.id" :value="a.id">{{ a.name || a.email }}</option>
          </select>
        </div>
        <div>
          <label class="block text-xs text-[#8c8475] mb-1">令牌</label>
          <select v-model="filterToken" @change="applyFilter" class="h-9 px-2 rounded-lg border border-[#e7e0d6] bg-white text-sm text-[#29261e]">
            <option value="">全部</option>
            <option v-for="t in tokens" :key="t.id" :value="t.id">{{ t.name || ('#' + t.id) }}</option>
          </select>
        </div>
        <div>
          <label class="block text-xs text-[#8c8475] mb-1">结果</label>
          <select v-model="filterResult" @change="applyFilter" class="h-9 px-2 rounded-lg border border-[#e7e0d6] bg-white text-sm text-[#29261e]">
            <option value="">全部</option>
            <option value="success">仅成功</option>
            <option value="error">仅失败</option>
          </select>
        </div>
        <div>
          <label class="block text-xs text-[#8c8475] mb-1">模型</label>
          <select v-model="filterModel" @change="applyFilter" class="h-9 px-2 rounded-lg border border-[#e7e0d6] bg-white text-sm text-[#29261e]">
            <option value="">全部</option>
            <option v-for="m in models" :key="m" :value="m">{{ m }}</option>
          </select>
        </div>
        <div>
          <label class="block text-xs text-[#8c8475] mb-1">起始日期</label>
          <input type="date" v-model="filterStart" class="h-9 px-2 rounded-lg border border-[#e7e0d6] bg-white text-sm text-[#29261e]" />
        </div>
        <div>
          <label class="block text-xs text-[#8c8475] mb-1">结束日期</label>
          <input type="date" v-model="filterEnd" class="h-9 px-2 rounded-lg border border-[#e7e0d6] bg-white text-sm text-[#29261e]" />
        </div>
        <Button class="h-9 bg-[#c4704f] hover:bg-[#b3613f] text-white" @click="applyFilter">查询</Button>
        <Button variant="outline" class="h-9 border-[#e7e0d6]" @click="resetFilter">重置</Button>
      </div>
    </Card>

    <!-- 明细表 -->
    <Card class="border-[#e7e0d6] overflow-hidden">
      <div class="overflow-x-auto">
        <table class="w-full text-sm">
          <thead>
            <tr class="text-left text-[#8c8475] text-xs border-b border-[#e7e0d6]">
              <th class="px-4 py-3 font-medium">时间</th>
              <th class="px-4 py-3 font-medium">令牌</th>
              <th class="px-4 py-3 font-medium">账号</th>
              <th class="px-4 py-3 font-medium">模型</th>
              <th class="px-4 py-3 font-medium text-right">输入</th>
              <th class="px-4 py-3 font-medium text-right">输出</th>
              <th class="px-4 py-3 font-medium text-right">缓存读</th>
              <th class="px-4 py-3 font-medium text-right">缓存创建</th>
              <th class="px-4 py-3 font-medium text-right">耗时</th>
              <th class="px-4 py-3 font-medium text-right">状态</th>
            </tr>
          </thead>
          <tbody>
            <tr v-if="loading">
              <td colspan="10" class="px-4 py-8 text-center text-[#8c8475]">加载中…</td>
            </tr>
            <tr v-else-if="logs.length === 0">
              <td colspan="10" class="px-4 py-8 text-center text-[#8c8475]">暂无调用记录</td>
            </tr>
            <template v-for="r in logs" :key="r.id">
              <tr class="border-b border-[#f0ebe4] hover:bg-[#faf7f2] cursor-pointer" :class="{ 'border-b-0': expanded === r.id }" @click="toggle(r.id)">
                <td class="px-4 py-2.5 text-[#29261e] whitespace-nowrap">
                  <span class="inline-block w-3 text-[#b5b0a6]">{{ expanded === r.id ? '▾' : '▸' }}</span>
                  {{ new Date(r.created_at).toLocaleString() }}
                </td>
                <td class="px-4 py-2.5 text-[#8c8475]">{{ tokenName(r.token_id) }}</td>
                <td class="px-4 py-2.5 text-[#8c8475]">{{ accountName(r.account_id) }}</td>
                <td class="px-4 py-2.5 text-[#29261e]">{{ r.model || '-' }}<span v-if="r.stream" class="ml-1 text-[10px] text-[#c4704f]">流</span></td>
                <td class="px-4 py-2.5 text-right text-[#29261e]">{{ fmt(r.input_tokens) }}</td>
                <td class="px-4 py-2.5 text-right text-[#29261e]">{{ fmt(r.output_tokens) }}</td>
                <td class="px-4 py-2.5 text-right text-[#c4704f]">{{ fmt(r.cache_read_tokens) }}</td>
                <td class="px-4 py-2.5 text-right text-[#c4704f]">{{ fmt(r.cache_creation_tokens) }}</td>
                <td class="px-4 py-2.5 text-right text-[#8c8475]">{{ r.duration_ms }}ms</td>
                <td class="px-4 py-2.5 text-right" :class="r.status_code >= 200 && r.status_code < 300 ? 'text-emerald-600' : 'text-red-500'">{{ r.status_code }}</td>
              </tr>
              <tr v-if="expanded === r.id" class="border-b border-[#f0ebe4] bg-[#faf7f2]">
                <td colspan="10" class="px-4 pb-4 pt-1">
                  <div v-if="r.error" class="mb-3">
                    <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider mb-1">错误正文</p>
                    <pre class="text-[11px] text-red-600 bg-red-50 rounded-lg px-3 py-2 whitespace-pre-wrap break-all max-h-48 overflow-auto">{{ pretty(r.error) }}</pre>
                  </div>
                  <div class="grid grid-cols-2 md:grid-cols-3 gap-x-6 gap-y-2 mb-3">
                    <div><span class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">请求ID</span><p class="font-mono text-[11px] text-[#5c5647] break-all">{{ r.request_id || '-' }}</p></div>
                    <div><span class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">客户端IP</span><p class="font-mono text-[11px] text-[#5c5647]">{{ r.client_ip || '-' }}</p></div>
                    <div><span class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">出口代理</span><p class="font-mono text-[11px] text-[#5c5647] break-all">{{ r.proxy || '直连' }}</p></div>
                    <div><span class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">User-Agent</span><p class="font-mono text-[11px] text-[#5c5647] break-all">{{ r.user_agent || '-' }}</p></div>
                    <div><span class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">会话ID</span><p class="font-mono text-[11px] text-[#5c5647] break-all">{{ r.session_id || '-' }}</p></div>
                    <div><span class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">user_id</span><p class="font-mono text-[11px] text-[#5c5647] break-all">{{ r.user_id || '-' }}</p></div>
                    <div><span class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">路径</span><p class="font-mono text-[11px] text-[#5c5647] break-all">{{ r.path || '-' }}</p></div>
                  </div>
                  <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
                    <div>
                      <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider mb-1">上游响应头(含限流/cf-ray/request-id)</p>
                      <pre class="text-[11px] text-[#5c5647] bg-white border border-[#e7e0d6] rounded-lg px-3 py-2 whitespace-pre-wrap break-all max-h-64 overflow-auto">{{ pretty(r.resp_headers) || '-' }}</pre>
                    </div>
                    <div>
                      <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider mb-1">请求头(已脱敏)</p>
                      <pre class="text-[11px] text-[#5c5647] bg-white border border-[#e7e0d6] rounded-lg px-3 py-2 whitespace-pre-wrap break-all max-h-64 overflow-auto">{{ pretty(r.req_headers) || '-' }}</pre>
                    </div>
                  </div>
                </td>
              </tr>
            </template>
          </tbody>
        </table>
      </div>
      <div v-if="totalPages > 1" class="flex items-center justify-between px-4 py-3 border-t border-[#e7e0d6]">
        <p class="text-xs text-[#8c8475]">共 {{ fmt(total) }} 条，第 {{ page }} / {{ totalPages }} 页</p>
        <div class="flex gap-2">
          <Button variant="outline" class="h-8 px-3 border-[#e7e0d6]" :disabled="page <= 1" @click="go(1)">首页</Button>
          <Button variant="outline" class="h-8 px-3 border-[#e7e0d6]" :disabled="page <= 1" @click="go(page - 1)">上一页</Button>
          <Button variant="outline" class="h-8 px-3 border-[#e7e0d6]" :disabled="page >= totalPages" @click="go(page + 1)">下一页</Button>
          <Button variant="outline" class="h-8 px-3 border-[#e7e0d6]" :disabled="page >= totalPages" @click="go(totalPages)">末页</Button>
        </div>
      </div>
    </Card>
  </div>
</template>
