<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { api, type Account, type OAuthExchangeResult } from '../api';
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from '@/components/ui/table';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import { Badge } from '@/components/ui/badge';
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from '@/components/ui/dialog';
import { useToast } from '../composables/useToast';

const emit = defineEmits<{ refresh: [] }>();
const { show: toast } = useToast();

/** 账号列表 */
const accounts = ref<Account[]>([]);
/** 分页状态 */
const currentPage = ref(1);
const totalPages = ref(1);
const totalCount = ref(0);
const pageSize = 12;
/** 表单弹窗是否可见 */
const showForm = ref(false);
/** 删除确认弹窗是否可见 */
const showDeleteConfirm = ref(false);
/** 待删除账号 ID */
const deleteTargetId = ref<number | null>(null);
/** 当前编辑的账号（null 表示新建） */
const editing = ref<Account | null>(null);
/** 表单数据 */
const form = ref({
  name: '',
  email: '',
  auth_type: 'setup_token',
  setup_token: '',
  access_token: '',
  refresh_token: '',
  expires_at: '',
  proxy_url: '',
  billing_mode: 'strip',
  account_uuid: '',
  organization_uuid: '',
  subscription_type: '',
  concurrency: 3,
  priority: 50,
  auto_telemetry: false,
  rpm_limit: 0,
  identity_mode: 'passthrough',
  virtual_user: '',
  virtual_git_name: '',
  recapture_days: 0,
  max_sessions: 3,
  allowed_client_types: [] as string[],
});
/** 客户端类型放行选项（账号级限制） */
const clientTypeOptions = [
  { value: 'cli', label: 'cli 终端' },
  { value: 'vscode', label: 'VSCode 插件' },
  { value: 'sdk', label: 'Agent SDK' },
  { value: 'desktop', label: '桌面三方' },
  { value: 'other', label: '其它/非CC' },
];
function toggleClientType(v: string) {
  const arr = form.value.allowed_client_types;
  const i = arr.indexOf(v);
  if (i >= 0) arr.splice(i, 1);
  else arr.push(v);
}

/** 正在测试的账号 ID */
const testing = ref<number | null>(null);
/** 测试结果 */
const testResult = ref<{ status: string; message?: string } | null>(null);
/** 正在刷新用量的账号 ID */
const refreshingUsage = ref<number | null>(null);

/** 加载账号列表 */
async function load() {
  try {
    const res = await api.listAccounts(currentPage.value, pageSize);
    accounts.value = res.data ?? [];
    totalPages.value = res.total_pages;
    totalCount.value = res.total;
  } catch {
    accounts.value = [];
  }
}

/** 翻页 */
function goToPage(page: number) {
  if (page < 1 || page > totalPages.value) return;
  currentPage.value = page;
  load();
}

/** 可见的页码列表 */
const visiblePages = computed(() => {
  const pages: number[] = [];
  const total = totalPages.value;
  const current = currentPage.value;
  let start = Math.max(1, current - 2);
  let end = Math.min(total, start + 4);
  start = Math.max(1, end - 4);
  for (let i = start; i <= end; i++) pages.push(i);
  return pages;
});

/** 活着优先排序：active&未限流(0) > active&限流(1) > error(2) > disabled/其它(3)。
 *  同级保持后端顺序(后端已按状态分级 + 优先级排,这里在当前页内细排限流态)。 */
const sortedAccounts = computed(() => {
  const rank = (a: Account): number => {
    if (a.status === 'active') return isRateLimited(a) ? 1 : 0;
    if (a.status === 'error') return 2;
    return 3;
  };
  return [...accounts.value].sort((x, y) => rank(x) - rank(y));
});

/** 自动重载定时器 */
let autoReloadTimer: ReturnType<typeof setInterval> | null = null;

onMounted(() => {
  load();
  // 每 8 秒静默重拉账户列表（实时并发 / usage_data 随之刷新）
  autoReloadTimer = setInterval(() => {
    load();
  }, 8 * 1000);
});

onUnmounted(() => {
  if (autoReloadTimer) {
    clearInterval(autoReloadTimer);
    autoReloadTimer = null;
  }
});

/** 打开新建账号弹窗 */
function openCreate() {
  editing.value = null;
  form.value = {
    name: '',
    email: '',
    auth_type: 'setup_token',
    setup_token: '',
    access_token: '',
    refresh_token: '',
    expires_at: '',
    proxy_url: '',
    billing_mode: 'strip',
    account_uuid: '',
    organization_uuid: '',
    subscription_type: '',
    concurrency: 3,
    priority: 50,
    auto_telemetry: false,
    rpm_limit: 0,
    identity_mode: 'passthrough',
    virtual_user: '',
    virtual_git_name: '',
    recapture_days: 0,
    max_sessions: 3,
    allowed_client_types: [] as string[],
  };
  showForm.value = true;
}

/**
 * 打开编辑账号弹窗
 * @param a 要编辑的账号对象
 */
function openEdit(a: Account) {
  editing.value = a;
  form.value = {
    name: a.name,
    email: a.email,
    auth_type: a.auth_type || 'setup_token',
    setup_token: '',
    access_token: '',
    refresh_token: '',
    expires_at: a.expires_at ? String(a.expires_at) : '',
    proxy_url: a.proxy_url,
    billing_mode: a.billing_mode || 'strip',
    account_uuid: a.account_uuid || '',
    organization_uuid: a.organization_uuid || '',
    subscription_type: a.subscription_type || '',
    concurrency: a.concurrency,
    priority: a.priority,
    auto_telemetry: a.auto_telemetry ?? false,
    rpm_limit: a.rpm_limit || 0,
    identity_mode: a.identity_mode || 'passthrough',
    virtual_user: a.virtual_user || '',
    virtual_git_name: a.virtual_git_name || '',
    recapture_days: a.recapture_days ?? 0,
    max_sessions: a.max_sessions ?? 3,
    allowed_client_types: (a.allowed_client_types || '').split(',').map(s => s.trim()).filter(Boolean),
  };
  showForm.value = true;
}

/** 保存账号（新建或更新） */
async function save() {
  try {
    const expiresAt = form.value.expires_at.trim();
    if (editing.value) {
      if (form.value.auth_type === 'setup_token'
        && !form.value.setup_token.trim()
        && editing.value.auth_type !== 'setup_token') {
        throw new Error('切换到 Setup Token 模式时必须填写 Setup Token');
      }
      if (form.value.auth_type === 'oauth'
        && !form.value.refresh_token.trim()
        && editing.value.auth_type !== 'oauth') {
        throw new Error('切换到 OAuth 模式时必须填写 Refresh Token');
      }
      const updates: Record<string, unknown> = {};
      if (form.value.name) updates.name = form.value.name;
      if (form.value.email) updates.email = form.value.email;
      updates.auth_type = form.value.auth_type;
      if (form.value.setup_token) updates.setup_token = form.value.setup_token;
      if (form.value.access_token) updates.access_token = form.value.access_token;
      if (form.value.refresh_token) updates.refresh_token = form.value.refresh_token;
      if (expiresAt) updates.expires_at = Number(expiresAt);
      updates.proxy_url = form.value.proxy_url;
      updates.billing_mode = form.value.billing_mode;
      updates.account_uuid = form.value.account_uuid || null;
      updates.organization_uuid = form.value.organization_uuid || null;
      updates.subscription_type = form.value.subscription_type || null;
      updates.concurrency = form.value.concurrency;
      updates.priority = form.value.priority;
      updates.auto_telemetry = form.value.auto_telemetry;
      updates.rpm_limit = form.value.rpm_limit || 0;
      updates.identity_mode = form.value.identity_mode;
      updates.virtual_user = form.value.virtual_user;
      updates.virtual_git_name = form.value.virtual_git_name;
      updates.recapture_days = Number(form.value.recapture_days) || 0;
      updates.max_sessions = Math.max(0, Number(form.value.max_sessions) || 0);
      updates.allowed_client_types = form.value.allowed_client_types.join(',');
      await api.updateAccount(editing.value.id, updates);
    } else {
      if (form.value.auth_type === 'setup_token' && !form.value.setup_token.trim()) {
        throw new Error('Setup Token 不能为空');
      }
      if (form.value.auth_type === 'oauth' && !form.value.refresh_token.trim()) {
        throw new Error('Refresh Token 不能为空');
      }
      const payload: Record<string, unknown> = {
        name: form.value.name,
        email: form.value.email,
        auth_type: form.value.auth_type,
        setup_token: form.value.setup_token,
        access_token: form.value.access_token,
        refresh_token: form.value.refresh_token,
        proxy_url: form.value.proxy_url,
        billing_mode: form.value.billing_mode,
        account_uuid: form.value.account_uuid || null,
        organization_uuid: form.value.organization_uuid || null,
        subscription_type: form.value.subscription_type || null,
        concurrency: form.value.concurrency,
        priority: form.value.priority,
        auto_telemetry: form.value.auto_telemetry,
        rpm_limit: form.value.rpm_limit || 0,
        identity_mode: form.value.identity_mode,
        virtual_user: form.value.virtual_user,
        virtual_git_name: form.value.virtual_git_name,
        recapture_days: Number(form.value.recapture_days) || 0,
        max_sessions: Math.max(0, Number(form.value.max_sessions) || 0),
        allowed_client_types: form.value.allowed_client_types.join(','),
      };
      if (expiresAt) payload.expires_at = Number(expiresAt);
      await api.createAccount(payload);
    }
    showForm.value = false;
    await load();
    emit('refresh');
  } catch (e: unknown) {
    toast((e as Error).message || '保存失败');
  }
}

/**
 * 确认删除账号
 * @param id 账号 ID
 */
function confirmDelete(id: number) {
  deleteTargetId.value = id;
  showDeleteConfirm.value = true;
}

/** 执行删除账号 */
async function executeDelete() {
  if (deleteTargetId.value === null) return;
  try {
    await api.deleteAccount(deleteTargetId.value);
    showDeleteConfirm.value = false;
    deleteTargetId.value = null;
    await load();
    emit('refresh');
  } catch (e: unknown) {
    toast((e as Error).message || '删除失败');
  }
}

/**
 * 测试账号连接
 * @param id 账号 ID
 */
async function test(id: number) {
  testing.value = id;
  testResult.value = null;
  try {
    testResult.value = await api.testAccount(id);
    if (testResult.value.status === 'error') {
      toast(testResult.value.message || '测试失败');
    }
  } catch (e: unknown) {
    toast((e as Error).message || '测试请求失败');
  }
  setTimeout(() => { testing.value = null; testResult.value = null; }, 3000);
}

/**
 * 刷新账号用量数据
 * @param id 账号 ID
 */
async function refreshUsage(id: number) {
  refreshingUsage.value = id;
  try {
    const res = await api.refreshUsage(id);
    if (res.status === 'ok' && res.usage) {
      const acc = accounts.value.find(a => a.id === id);
      if (acc) {
        acc.usage_data = res.usage;
        acc.usage_fetched_at = new Date().toISOString();
      }
    } else if (res.status === 'error') {
      toast(res.message || '刷新用量失败');
    }
  } catch (e: unknown) {
    toast((e as Error).message || '刷新用量失败');
  }
  refreshingUsage.value = null;
}

/**
 * 切换账号调度状态（启用/停用）
 * @param a 账号对象
 */
async function toggleScheduling(a: Account) {
  try {
    const isStopped = a.status === 'disabled' || isRateLimited(a);
    const newStatus = isStopped ? 'active' : 'disabled';
    const res = await api.updateAccount(a.id, { status: newStatus });
    a.status = res.status;
    a.disable_reason = res.disable_reason ?? '';
    a.rate_limited_at = res.rate_limited_at;
    a.rate_limit_reset_at = res.rate_limit_reset_at;
    emit('refresh');
  } catch (e: unknown) {
    toast((e as Error).message || '切换调度失败');
  }
}

/**
 * 格式化剩余时间
 * @param resetsAt ISO 时间字符串
 */
function formatTimeLeft(resetsAt: string): string {
  const diff = new Date(resetsAt).getTime() - Date.now();
  if (diff <= 0) return '已重置';
  const days = Math.floor(diff / 86400000);
  const hours = Math.floor((diff % 86400000) / 3600000);
  const minutes = Math.floor((diff % 3600000) / 60000);
  if (days > 0) return `${days}d${hours}h${minutes}m`;
  if (hours > 0) return `${hours}h${minutes}m`;
  return `${minutes}m`;
}

/**
 * 获取用量进度条颜色
 * @param pct 使用百分比 (0-100)
 */
function usageBarColor(pct: number): string {
  if (pct >= 80) return 'bg-red-400';
  if (pct >= 50) return 'bg-amber-400';
  return 'bg-emerald-400';
}

/**
 * 判断账号是否正在被限流
 */
function isRateLimited(a: Account): boolean {
  return !!(a.rate_limit_reset_at && new Date(a.rate_limit_reset_at) > new Date());
}

/**
 * 获取状态徽章样式
 */
function statusStyle(a: Account): { class: string; label: string } {
  if (a.status === 'active' && isRateLimited(a)) {
    return { class: 'bg-amber-50 text-amber-700 border-amber-200', label: '限流中' };
  }
  if (a.status === 'active') return { class: 'bg-emerald-50 text-emerald-700 border-emerald-200', label: '活跃' };
  if (a.status === 'error') return { class: 'bg-red-50 text-red-600 border-red-200', label: '异常' };
  return { class: 'bg-gray-100 text-gray-500 border-gray-200', label: '停用' };
}

/** 格式化 OAuth 过期时间(毫秒时间戳) */
function formatExpiresAt(expiresAt?: number | null): string {
  if (!expiresAt) return '未提供';
  return new Date(expiresAt).toLocaleString('zh-CN');
}

/** 切换认证方式 */
function setAuthType(authType: 'setup_token' | 'oauth') {
  form.value.auth_type = authType;
}

// --- OAuth 授权流程 ---
const showOAuthFlow = ref(false);
const oauthMode = ref<'oauth' | 'setup_token' | 'session_key'>('oauth');
const oauthProxyUrl = ref('');
const oauthSessionId = ref('');
const oauthAuthUrl = ref('');
const oauthCode = ref('');
const oauthSessionKey = ref('');
const oauthLoading = ref(false);
const oauthResult = ref<OAuthExchangeResult | null>(null);
const oauthStep = ref<'generate' | 'exchange' | 'done'>('generate');

/** 打开 OAuth 授权流程弹窗 */
function openOAuthFlow() {
  oauthMode.value = 'oauth';
  oauthProxyUrl.value = '';
  oauthSessionId.value = '';
  oauthAuthUrl.value = '';
  oauthCode.value = '';
  oauthSessionKey.value = '';
  oauthResult.value = null;
  oauthStep.value = 'generate';
  oauthLoading.value = false;
  showOAuthFlow.value = true;
}

/** Session Key 一步录号：粘贴 claude.ai sessionKey，自动完成授权 */
async function exchangeSessionKeyFlow() {
  const key = oauthSessionKey.value.trim();
  if (!key) {
    toast('请粘贴 sessionKey');
    return;
  }
  oauthLoading.value = true;
  try {
    const proxy = oauthProxyUrl.value.trim() || undefined;
    const res = await api.exchangeSessionKey(key, proxy);
    oauthResult.value = res;
    oauthStep.value = 'done';
  } catch (e: unknown) {
    toast((e as Error).message || 'Session Key 授权失败');
  }
  oauthLoading.value = false;
}

/** 生成授权链接 */
async function generateOAuthUrl() {
  oauthLoading.value = true;
  try {
    const proxy = oauthProxyUrl.value.trim() || undefined;
    const res = oauthMode.value === 'oauth'
      ? await api.generateAuthUrl(proxy)
      : await api.generateSetupTokenUrl(proxy);
    oauthSessionId.value = res.session_id;
    oauthAuthUrl.value = res.auth_url;
    oauthStep.value = 'exchange';
  } catch (e: unknown) {
    toast((e as Error).message || '生成授权链接失败');
  }
  oauthLoading.value = false;
}

/** 交换 code */
async function exchangeOAuthCode() {
  const code = oauthCode.value.trim();
  if (!code) {
    toast('请输入授权码');
    return;
  }
  oauthLoading.value = true;
  try {
    const res = oauthMode.value === 'oauth'
      ? await api.exchangeCode(oauthSessionId.value, code)
      : await api.exchangeSetupTokenCode(oauthSessionId.value, code);
    oauthResult.value = res;
    oauthStep.value = 'done';
  } catch (e: unknown) {
    toast((e as Error).message || '交换 Token 失败');
  }
  oauthLoading.value = false;
}

/** 将授权结果填入新建账号表单 */
function applyOAuthResult() {
  const r = oauthResult.value;
  if (!r) return;
  showOAuthFlow.value = false;
  editing.value = null;
  const isSetupToken = oauthMode.value === 'setup_token';
  form.value = {
    name: '',
    email: r.email_address || '',
    auth_type: isSetupToken ? 'setup_token' : 'oauth',
    setup_token: isSetupToken ? r.access_token : '',
    access_token: isSetupToken ? '' : (r.access_token || ''),
    refresh_token: isSetupToken ? '' : (r.refresh_token || ''),
    expires_at: (!isSetupToken && r.expires_at) ? String(r.expires_at * 1000) : '',
    proxy_url: oauthProxyUrl.value || '',
    billing_mode: 'strip',
    account_uuid: r.account_uuid || '',
    organization_uuid: r.organization_uuid || '',
    subscription_type: '',
    concurrency: 3,
    priority: 50,
    auto_telemetry: false,
    rpm_limit: 0,
    identity_mode: 'passthrough',
    virtual_user: '',
    virtual_git_name: '',
    recapture_days: 0,
    max_sessions: 3,
    allowed_client_types: [] as string[],
  };
  showForm.value = true;
}

/** 复制文本到剪贴板（兼容非安全上下文） */
async function copyText(text: string) {
  if (!text) {
    toast('没有可复制的内容');
    return;
  }

  // 1. 优先使用 Clipboard API（仅在安全上下文 HTTPS / localhost 可用）
  if (navigator.clipboard && window.isSecureContext) {
    try {
      await navigator.clipboard.writeText(text);
      toast('已复制');
      return;
    } catch {
      // 失败则继续走降级方案
    }
  }

  // 2. 降级方案：临时 textarea + execCommand('copy')
  try {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'fixed';
    ta.style.top = '0';
    ta.style.left = '0';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    ta.setSelectionRange(0, text.length);
    const ok = document.execCommand('copy');
    document.body.removeChild(ta);
    toast(ok ? '已复制' : '复制失败');
  } catch {
    toast('复制失败');
  }
}
</script>

<template>
  <div class="space-y-4">
    <!-- 标题栏 -->
    <div class="flex justify-between items-center">
      <h2 class="text-lg font-semibold text-[#29261e]">账号管理</h2>
      <div class="flex gap-2">
        <Button
          @click="openOAuthFlow"
          class="bg-[#c4704f] hover:bg-[#b5623f] text-white font-medium rounded-xl transition-all duration-200 hover:shadow-md"
        >
          授权登录
        </Button>
        <Button
          @click="openCreate"
          class="bg-[#c4704f] hover:bg-[#b5623f] text-white font-medium rounded-xl transition-all duration-200 hover:shadow-md"
        >
          添加账号
        </Button>
      </div>
    </div>

    <!-- 账号列表（表格） -->
    <div class="border border-[#e8e2d9] rounded-xl overflow-x-auto bg-white">
      <Table>
        <TableHeader>
          <TableRow class="border-[#f0ebe4] bg-[#faf8f5] hover:bg-[#faf8f5]">
            <TableHead class="text-[#8c8475] text-xs">账号</TableHead>
            <TableHead class="text-[#8c8475] text-xs">状态</TableHead>
            <TableHead class="text-[#8c8475] text-xs">并发</TableHead>
            <TableHead class="text-[#8c8475] text-xs">会话</TableHead>
            <TableHead class="text-[#8c8475] text-xs">RPM</TableHead>
            <TableHead class="text-[#8c8475] text-xs">用量(5h/7d/Son)</TableHead>
            <TableHead class="text-[#8c8475] text-xs">身份/遥测</TableHead>
            <TableHead class="text-[#8c8475] text-xs">配置</TableHead>
            <TableHead class="text-[#8c8475] text-xs text-right">操作</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="a in sortedAccounts"
            :key="a.id"
            class="border-[#f0ebe4] hover:bg-[#faf8f5]/60 align-top"
            :class="(a.status === 'disabled' || isRateLimited(a)) ? 'opacity-60' : ''"
          >
            <!-- 账号 -->
            <TableCell class="py-2.5">
              <div class="flex items-center gap-2 min-w-0 max-w-[220px]">
                <div class="w-7 h-7 rounded-lg bg-[#c4704f]/10 flex items-center justify-center flex-shrink-0">
                  <span class="text-[#c4704f] text-xs font-semibold">{{ (a.name || a.email)[0].toUpperCase() }}</span>
                </div>
                <div class="min-w-0">
                  <p class="text-sm font-medium text-[#29261e] truncate">{{ a.name || a.email }}</p>
                  <p v-if="a.name" class="text-xs text-[#8c8475] truncate">{{ a.email }}</p>
                </div>
              </div>
              <p
                v-if="a.disable_reason && (a.status === 'disabled' || isRateLimited(a))"
                class="text-[11px] mt-1 max-w-[220px] truncate"
                :class="a.status === 'disabled' ? 'text-red-600' : 'text-amber-600'"
              >
                {{ a.disable_reason }}<span v-if="isRateLimited(a)"> · 剩余 {{ formatTimeLeft(a.rate_limit_reset_at!) }}</span>
              </p>
              <p v-if="a.auth_type === 'oauth' && a.auth_error" class="text-[11px] text-red-500 mt-0.5 max-w-[220px] truncate">{{ a.auth_error }}</p>
              <p
                v-if="testing === a.id && testResult"
                class="text-[11px] mt-0.5 font-medium"
                :class="testResult.status === 'ok' ? 'text-emerald-600' : 'text-red-500'"
              >
                {{ testResult.status === 'ok' ? '连接正常' : testResult.message }}
              </p>
            </TableCell>
            <!-- 状态 -->
            <TableCell class="py-2.5">
              <Badge :class="statusStyle(a).class" class="border text-xs font-medium">
                {{ statusStyle(a).label }}
              </Badge>
            </TableCell>
            <!-- 并发 -->
            <TableCell class="py-2.5 text-sm font-medium" :class="(a.current_concurrency || 0) >= a.concurrency ? 'text-red-500' : (a.current_concurrency || 0) > 0 ? 'text-emerald-600' : 'text-[#29261e]'">
              {{ a.current_concurrency || 0 }} / {{ a.concurrency }}
            </TableCell>
            <!-- 会话 -->
            <TableCell class="py-2.5 text-sm font-medium" :class="a.max_sessions && (a.current_sessions || 0) >= a.max_sessions ? 'text-red-500' : (a.current_sessions || 0) > 0 ? 'text-emerald-600' : 'text-[#29261e]'">
              {{ a.current_sessions || 0 }} / {{ a.max_sessions || '∞' }}
            </TableCell>
            <!-- RPM（实时） -->
            <TableCell class="py-2.5">
              <div v-if="a.rpm_limit && a.rpm_limit > 0" class="flex items-center gap-1.5 min-w-[88px]">
                <div class="w-12 bg-[#f0ebe4] rounded-full h-1.5 overflow-hidden flex-shrink-0">
                  <div
                    class="h-full rounded-full transition-all"
                    :class="(a.current_rpm || 0) / a.rpm_limit >= 0.8 ? 'bg-red-500' : (a.current_rpm || 0) / a.rpm_limit >= 0.5 ? 'bg-amber-500' : 'bg-emerald-500'"
                    :style="{ width: Math.min(100, ((a.current_rpm || 0) / a.rpm_limit) * 100) + '%' }"
                  />
                </div>
                <span class="text-xs whitespace-nowrap" :class="(a.current_rpm || 0) > 0 ? 'text-[#29261e] font-medium' : 'text-[#8c8475]'">{{ a.current_rpm || 0 }}/{{ a.rpm_limit }}</span>
              </div>
              <span v-else class="text-sm" :class="(a.current_rpm || 0) > 0 ? 'text-emerald-600 font-medium' : 'text-[#8c8475]'">{{ a.current_rpm || 0 }}</span>
            </TableCell>
            <!-- 用量 5h/7d/Sonnet -->
            <TableCell class="py-2.5">
              <div class="text-[11px] space-y-0.5 min-w-[112px]">
                <div v-for="w in [
                  { label: '5h', d: a.usage_data?.five_hour },
                  { label: '7d', d: a.usage_data?.seven_day },
                  { label: 'Son', d: a.usage_data?.seven_day_sonnet },
                ]" :key="w.label" class="flex items-center gap-1.5">
                  <span class="text-[#b5b0a6] w-6 flex-shrink-0">{{ w.label }}</span>
                  <div class="flex-1 h-1 bg-[#f0ebe4] rounded-full overflow-hidden">
                    <div :class="usageBarColor(w.d ? w.d.utilization : 0)" class="h-full rounded-full" :style="{ width: (w.d ? Math.min(w.d.utilization, 100) : 0) + '%' }" />
                  </div>
                  <span class="text-[#5c5647] font-medium w-8 text-right flex-shrink-0">{{ w.d ? Math.round(w.d.utilization) : 0 }}%</span>
                </div>
              </div>
            </TableCell>
            <!-- 身份/遥测 -->
            <TableCell class="py-2.5">
              <div class="text-[11px] space-y-0.5 max-w-[150px]">
                <p class="truncate" :class="a.identity_mode === 'normalize' ? 'text-emerald-600' : 'text-[#8c8475]'">
                  {{ a.identity_mode === 'normalize' ? '归一化' : '透传' }}
                  <span v-if="a.identity_mode === 'normalize' && a.identity_captured_at" class="text-[#b5b0a6]">v{{ a.canonical_env?.version }}</span>
                  <span v-else-if="a.identity_mode === 'normalize'" class="text-amber-500">待吸取</span>
                </p>
                <p :class="a.auto_telemetry ? 'text-emerald-600' : 'text-[#8c8475]'">
                  遥测{{ a.auto_telemetry ? '开' : '关' }}<span v-if="a.telemetry_count > 0" class="text-[#b5b0a6]"> ·{{ a.telemetry_count }}</span>
                </p>
                <p v-if="a.allowed_client_types" class="text-amber-600 truncate">仅 {{ a.allowed_client_types.split(',').filter(Boolean).join('/') }}</p>
              </div>
            </TableCell>
            <!-- 配置 -->
            <TableCell class="py-2.5">
              <div class="text-[11px] space-y-0.5">
                <p class="text-[#5c5647]">优先级 {{ a.priority }}</p>
                <p :class="a.billing_mode === 'rewrite' ? 'text-amber-600' : 'text-[#8c8475]'">{{ a.billing_mode === 'rewrite' ? '重写' : '清除' }}</p>
              </div>
            </TableCell>
            <!-- 操作 -->
            <TableCell class="py-2.5">
              <div class="flex items-center justify-end gap-0.5">
                <Button variant="ghost" size="sm" @click="toggleScheduling(a)"
                  :class="(a.status === 'disabled' || isRateLimited(a)) ? 'text-emerald-500 hover:text-emerald-600 hover:bg-emerald-50' : 'text-amber-500 hover:text-amber-600 hover:bg-amber-50'"
                  class="h-7 px-2 text-xs">
                  {{ (a.status === 'disabled' || isRateLimited(a)) ? '启用' : '停用' }}
                </Button>
                <Button variant="ghost" size="sm" @click="openEdit(a)" class="text-[#8c8475] hover:text-[#29261e] hover:bg-[#f0ebe4] h-7 px-2 text-xs">编辑</Button>
                <Button variant="ghost" size="sm" @click="refreshUsage(a.id)" :disabled="refreshingUsage === a.id" class="text-[#c4704f] hover:text-[#b5623f] hover:bg-[#c4704f]/5 h-7 px-2 text-xs">
                  {{ refreshingUsage === a.id ? '...' : '用量' }}
                </Button>
                <Button variant="ghost" size="sm" @click="test(a.id)" :disabled="testing === a.id" class="text-[#c4704f] hover:text-[#b5623f] hover:bg-[#c4704f]/5 h-7 px-2 text-xs">
                  {{ testing === a.id ? '...' : '测试' }}
                </Button>
                <Button variant="ghost" size="sm" @click="confirmDelete(a.id)" class="text-red-400 hover:text-red-500 hover:bg-red-50 h-7 px-2 text-xs">删除</Button>
              </div>
            </TableCell>
          </TableRow>
          <!-- 空状态 -->
          <TableRow v-if="sortedAccounts.length === 0" class="hover:bg-transparent border-0">
            <TableCell colspan="9" class="py-16">
              <div class="flex flex-col items-center justify-center text-[#b5b0a6]">
                <div class="w-12 h-12 rounded-xl bg-[#f0ebe4] flex items-center justify-center mb-3">
                  <svg class="w-6 h-6 text-[#c4704f]/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M18 7.5v3m0 0v3m0-3h3m-3 0h-3m-2.25-4.125a3.375 3.375 0 1 1-6.75 0 3.375 3.375 0 0 1 6.75 0ZM3 19.235v-.11a6.375 6.375 0 0 1 12.75 0v.109A12.318 12.318 0 0 1 9.374 21c-2.331 0-4.512-.645-6.374-1.766Z" />
                  </svg>
                </div>
                <p class="text-sm">暂无账号，点击"添加账号"开始</p>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <!-- 分页 -->
    <div v-if="totalPages > 1" class="flex items-center justify-between pt-2">
      <p class="text-sm text-[#8c8475]">共 {{ totalCount }} 个账号</p>
      <div class="flex items-center gap-1">
        <Button
          variant="ghost"
          size="sm"
          :disabled="currentPage <= 1"
          @click="goToPage(currentPage - 1)"
          class="h-8 px-2 text-[#8c8475] hover:text-[#29261e] hover:bg-[#f0ebe4] disabled:opacity-40"
        >
          上一页
        </Button>
        <Button
          v-for="p in visiblePages"
          :key="p"
          variant="ghost"
          size="sm"
          @click="goToPage(p)"
          class="h-8 w-8 p-0 text-sm"
          :class="p === currentPage
            ? 'bg-[#c4704f] text-white hover:bg-[#b5623f]'
            : 'text-[#8c8475] hover:text-[#29261e] hover:bg-[#f0ebe4]'"
        >
          {{ p }}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          :disabled="currentPage >= totalPages"
          @click="goToPage(currentPage + 1)"
          class="h-8 px-2 text-[#8c8475] hover:text-[#29261e] hover:bg-[#f0ebe4] disabled:opacity-40"
        >
          下一页
        </Button>
      </div>
    </div>

    <!-- 新建/编辑账号弹窗 -->
    <Dialog v-model:open="showForm">
      <DialogContent class="bg-white border-[#e8e2d9] rounded-2xl text-[#29261e] sm:max-w-md max-h-[85vh] flex flex-col">
        <DialogHeader class="flex-shrink-0">
          <DialogTitle class="text-[#29261e] text-lg">{{ editing ? '编辑账号' : '添加账号' }}</DialogTitle>
          <DialogDescription class="text-[#8c8475]">
            {{ editing ? '修改账号信息，凭证留空表示不更改' : '填写新账号信息' }}
          </DialogDescription>
        </DialogHeader>

        <form @submit.prevent="save" class="space-y-4 mt-2 overflow-y-auto flex-1 pr-1">
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">备注名（选填）</Label>
            <Input
              v-model="form.name"
              class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
            />
          </div>
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">邮箱 <span class="text-red-500">*</span></Label>
            <Input
              v-model="form.email"
              required
              class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
            />
          </div>
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">认证方式</Label>
            <div class="flex gap-2">
              <button
                type="button"
                @click="setAuthType('setup_token')"
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                :class="form.auth_type === 'setup_token'
                  ? 'bg-[#c4704f]/10 border-[#c4704f] text-[#c4704f]'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-[#c4704f]/40'"
              >
                Setup Token
              </button>
              <button
                type="button"
                @click="setAuthType('oauth')"
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                :class="form.auth_type === 'oauth'
                  ? 'bg-amber-50 border-amber-400 text-amber-600'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-amber-300'"
              >
                OAuth
              </button>
            </div>
          </div>
          <div v-if="form.auth_type === 'setup_token'" class="space-y-2">
            <Label class="text-[#5c5647] text-sm">
              Setup Token (sk-ant-oat01-...) <span v-if="!editing" class="text-red-500">*</span>
            </Label>
            <Textarea
              v-model="form.setup_token"
              :required="!editing"
              :rows="3"
              :placeholder="editing ? '留空保持不变' : ''"
              class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 font-mono text-sm"
            />
          </div>
          <template v-else>
            <div class="space-y-2">
              <Label class="text-[#5c5647] text-sm">Access Token（选填）</Label>
              <Textarea
                v-model="form.access_token"
                :rows="2"
                :placeholder="editing ? '留空保持不变' : '已有 access token 时可直接填写'"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 font-mono text-sm"
              />
            </div>
            <div class="space-y-2">
              <Label class="text-[#5c5647] text-sm">
                Refresh Token <span class="text-red-500">*</span>
              </Label>
              <Textarea
                v-model="form.refresh_token"
                :required="!editing"
                :rows="2"
                :placeholder="editing ? '留空保持不变' : ''"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 font-mono text-sm"
              />
            </div>
            <div class="space-y-2">
              <Label class="text-[#5c5647] text-sm">Expires At（毫秒时间戳，选填）</Label>
              <Input
                v-model="form.expires_at"
                inputmode="numeric"
                placeholder="例如：1743600000000"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 font-mono text-sm"
              />
            </div>
          </template>
          <div v-if="editing && editing.auth_type === 'oauth' && editing.expires_at" class="rounded-lg bg-[#f9f6f1] px-3 py-2 text-xs text-[#8c8475]">
            当前过期时间：{{ formatExpiresAt(editing.expires_at) }}
          </div>
          <div v-if="editing && editing.auth_type === 'oauth' && editing.auth_error" class="rounded-lg bg-red-50 px-3 py-2 text-xs text-red-500">
            最近认证错误：{{ editing.auth_error }}
          </div>
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">代理地址（选填）</Label>
            <Input
              v-model="form.proxy_url"
              placeholder="http:// 或 socks5://"
              class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
            />
          </div>
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">Billing 模式</Label>
            <div class="flex gap-2">
              <button
                type="button"
                @click="form.billing_mode = 'strip'"
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                :class="form.billing_mode === 'strip'
                  ? 'bg-[#c4704f]/10 border-[#c4704f] text-[#c4704f]'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-[#c4704f]/40'"
              >
                清除 (Strip)
              </button>
              <button
                type="button"
                @click="form.billing_mode = 'rewrite'"
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                :class="form.billing_mode === 'rewrite'
                  ? 'bg-amber-50 border-amber-400 text-amber-600'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-amber-300'"
              >
                重写 (Rewrite)
              </button>
            </div>
          </div>
          <!-- 遥测身份（选填） -->
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">订阅类型（选填，强烈推荐）</Label>
            <div class="flex gap-2 flex-wrap">
              <button
                v-for="opt in [
                  { value: '', label: '未设置' },
                  { value: 'max', label: 'Max' },
                  { value: 'pro', label: 'Pro' },
                  { value: 'team', label: 'Team' },
                  { value: 'enterprise', label: 'Enterprise' },
                ]"
                :key="opt.value"
                type="button"
                @click="form.subscription_type = opt.value"
                class="px-3 py-1.5 rounded-lg text-xs font-medium border transition-all duration-200"
                :class="form.subscription_type === opt.value
                  ? 'bg-[#c4704f]/10 border-[#c4704f] text-[#c4704f]'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-[#c4704f]/40'"
              >
                {{ opt.label }}
              </button>
            </div>
          </div>
          <div class="flex gap-4">
            <div class="flex-1 space-y-2">
              <Label class="text-[#5c5647] text-sm">Account UUID（选填）</Label>
              <Input
                v-model="form.account_uuid"
                placeholder="OAuth account UUID"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 font-mono text-sm"
              />
            </div>
            <div class="flex-1 space-y-2">
              <Label class="text-[#5c5647] text-sm">Organization UUID（选填）</Label>
              <Input
                v-model="form.organization_uuid"
                placeholder="OAuth organization UUID"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 font-mono text-sm"
              />
            </div>
          </div>
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">自动遥测</Label>
            <div class="flex gap-2">
              <button
                type="button"
                @click="form.auto_telemetry = false"
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                :class="!form.auto_telemetry
                  ? 'bg-[#f9f6f1] border-[#8c8475] text-[#5c5647]'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-[#8c8475]/40'"
              >
                关闭
              </button>
              <button
                type="button"
                @click="form.auto_telemetry = true"
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                :class="form.auto_telemetry
                  ? 'bg-emerald-50 border-emerald-400 text-emerald-600'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-emerald-300'"
              >
                开启
              </button>
            </div>
            <p class="text-xs text-[#b5b0a6]">开启后由网关代替客户端发送遥测请求</p>
          </div>
          <div class="flex gap-4">
            <div class="flex-1 space-y-2">
              <Label class="text-[#5c5647] text-sm">并发数</Label>
              <Input
                v-model.number="form.concurrency"
                type="number"
                min="1"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
              />
            </div>
            <div class="flex-1 space-y-2">
              <Label class="text-[#5c5647] text-sm">最大并发会话(0=不限)</Label>
              <Input
                v-model.number="form.max_sessions"
                type="number"
                min="0"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
              />
            </div>
            <div class="flex-1 space-y-2">
              <Label class="text-[#5c5647] text-sm">优先级</Label>
              <Input
                v-model.number="form.priority"
                type="number"
                min="1"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
              />
            </div>
          </div>
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">RPM 限制 <span class="text-[#b5b0a6] text-xs">(0 = 不限)</span></Label>
            <Input
              v-model.number="form.rpm_limit"
              type="number"
              min="0"
              placeholder="0"
              class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
            />
          </div>

          <!-- 允许的客户端类型 -->
          <div class="space-y-2">
            <Label class="text-[#5c5647] text-sm">允许的客户端类型 <span class="text-[#b5b0a6] text-xs">(不勾 = 全部放行)</span></Label>
            <div class="flex flex-wrap gap-2">
              <button
                v-for="opt in clientTypeOptions"
                :key="opt.value"
                type="button"
                @click="toggleClientType(opt.value)"
                :class="form.allowed_client_types.includes(opt.value)
                  ? 'bg-[#c4704f] text-white border-[#c4704f]'
                  : 'bg-[#f9f6f1] text-[#5c5647] border-[#e8e2d9]'"
                class="px-2.5 py-1 rounded-md border text-xs transition-colors"
              >{{ opt.label }}</button>
            </div>
            <p class="text-[10px] text-[#b5b0a6]">收紧后,只有勾选的类型能用本账号;其它类型自动换号,全不收则 403。例:只勾 cli = 只许真人终端。</p>
          </div>

          <!-- 身份模拟 -->
          <div class="space-y-2 pt-2 border-t border-[#f0ebe4]">
            <Label class="text-[#5c5647] text-sm">身份模拟</Label>
            <div class="flex gap-2">
              <button
                type="button"
                @click="form.identity_mode = 'passthrough'"
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all"
                :class="form.identity_mode === 'passthrough'
                  ? 'bg-[#c4704f]/10 border-[#c4704f] text-[#c4704f]'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-[#c4704f]/40'"
              >
                透传（单人）
              </button>
              <button
                type="button"
                @click="form.identity_mode = 'normalize'"
                class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all"
                :class="form.identity_mode === 'normalize'
                  ? 'bg-emerald-50 border-emerald-400 text-emerald-600'
                  : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-emerald-300'"
              >
                归一化（多人共号）
              </button>
            </div>
            <p class="text-xs text-[#b5b0a6]">
              {{ form.identity_mode === 'normalize'
                ? '多人共号：把每个用户的 home用户名/git/OS/device_id 统一成下面这套虚拟身份，让一个号始终像同一个人。'
                : '单人：客户端请求原样透传，最高保真（推荐你自己用）。' }}
            </p>

            <!-- normalize 时可编辑虚拟身份 -->
            <div v-if="form.identity_mode === 'normalize'" class="grid grid-cols-2 gap-3 pt-1">
              <div class="space-y-1">
                <Label class="text-[#5c5647] text-xs">虚拟用户名（留空自动派生）</Label>
                <Input
                  v-model="form.virtual_user"
                  placeholder="如 alexc"
                  class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
                />
              </div>
              <div class="space-y-1">
                <Label class="text-[#5c5647] text-xs">虚拟 git 用户名（留空自动派生）</Label>
                <Input
                  v-model="form.virtual_git_name"
                  placeholder="如 Alex Carter"
                  class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
                />
              </div>
            </div>

            <!-- 版本坐标吸取周期 -->
            <div v-if="form.identity_mode === 'normalize'" class="space-y-1 pt-1">
              <Label class="text-[#5c5647] text-xs">版本重新吸取周期（天，0=永久只吸一次）</Label>
              <Input
                v-model="form.recapture_days"
                type="number"
                min="0"
                placeholder="0"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
              />
              <p class="text-[11px] text-[#b5b0a6]">CC 版本/SDK 版本从该号第一个请求吸取并复用；周期到后由下一个请求重吸（模拟升级 CC）。device_id/系统等仍用预设。</p>
            </div>

            <!-- 编辑时显示该账号当前生效的虚拟身份 + 吸取状态 -->
            <div
              v-if="editing && editing.effective_identity"
              class="rounded-lg bg-[#f9f6f1] border border-[#e8e2d9] p-3 text-xs text-[#8c8475] space-y-1"
            >
              <p class="font-medium text-[#5c5647]">当前生效的虚拟身份</p>
              <p>虚拟用户：<span class="font-mono text-[#29261e]">{{ editing.effective_identity.virtual_user }}</span> · git：<span class="font-mono text-[#29261e]">{{ editing.effective_identity.git_name }}</span></p>
              <p>机器：{{ editing.effective_identity.platform }} / {{ editing.effective_identity.arch }} · device_id：<span class="font-mono">{{ editing.effective_identity.device_id.slice(0, 16) }}…</span></p>
              <p v-if="editing.identity_mode === 'normalize'">
                版本吸取：
                <template v-if="editing.identity_captured_at">
                  <span class="text-emerald-600">已吸取</span> · v{{ editing.canonical_env?.version }} · {{ new Date(editing.identity_captured_at).toLocaleString('zh-CN', { month:'2-digit', day:'2-digit', hour:'2-digit', minute:'2-digit' }) }}
                </template>
                <span v-else class="text-amber-500">待吸取（首个请求时种入）</span>
              </p>
            </div>
          </div>

          <DialogFooter class="gap-2 pt-2">
            <Button
              type="button"
              variant="ghost"
              @click="showForm = false"
              class="text-[#8c8475] hover:text-[#29261e] hover:bg-[#f0ebe4]"
            >
              取消
            </Button>
            <Button
              type="submit"
              class="bg-[#c4704f] hover:bg-[#b5623f] text-white font-medium rounded-xl transition-all duration-200"
            >
              保存
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>

    <!-- 删除确认弹窗 -->
    <Dialog v-model:open="showDeleteConfirm">
      <DialogContent class="bg-white border-[#e8e2d9] rounded-2xl text-[#29261e] sm:max-w-sm">
        <DialogHeader>
          <DialogTitle class="text-[#29261e]">确认删除</DialogTitle>
          <DialogDescription class="text-[#8c8475]">
            此操作不可撤销，确认要删除此账号吗？
          </DialogDescription>
        </DialogHeader>
        <DialogFooter class="gap-2 pt-4">
          <Button
            variant="ghost"
            @click="showDeleteConfirm = false"
            class="text-[#8c8475] hover:text-[#29261e] hover:bg-[#f0ebe4]"
          >
            取消
          </Button>
          <Button
            @click="executeDelete"
            class="bg-red-500 hover:bg-red-600 text-white font-medium rounded-xl transition-all duration-200"
          >
            删除
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- OAuth 授权流程弹窗 -->
    <Dialog v-model:open="showOAuthFlow">
      <DialogContent class="bg-white border-[#e8e2d9] rounded-2xl text-[#29261e] sm:max-w-lg max-h-[85vh] flex flex-col">
        <DialogHeader class="flex-shrink-0">
          <DialogTitle class="text-[#29261e] text-lg">OAuth 授权</DialogTitle>
          <DialogDescription class="text-[#8c8475]">
            通过浏览器完成 OAuth 授权，自动获取 Token 和账号信息
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-4 mt-2 overflow-y-auto flex-1 pr-1">
          <!-- 步骤 1：选择模式并生成链接 -->
          <template v-if="oauthStep === 'generate'">
            <div class="space-y-2">
              <Label class="text-[#5c5647] text-sm">授权类型</Label>
              <div class="flex gap-2">
                <button
                  type="button"
                  @click="oauthMode = 'oauth'"
                  class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                  :class="oauthMode === 'oauth'
                    ? 'bg-amber-50 border-amber-400 text-amber-600'
                    : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-amber-300'"
                >
                  OAuth（完整）
                </button>
                <button
                  type="button"
                  @click="oauthMode = 'setup_token'"
                  class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                  :class="oauthMode === 'setup_token'
                    ? 'bg-[#c4704f]/10 border-[#c4704f] text-[#c4704f]'
                    : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-[#c4704f]/40'"
                >
                  Setup Token
                </button>
                <button
                  type="button"
                  @click="oauthMode = 'session_key'"
                  class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-all duration-200"
                  :class="oauthMode === 'session_key'
                    ? 'bg-emerald-50 border-emerald-400 text-emerald-600'
                    : 'bg-[#f9f6f1] border-[#e8e2d9] text-[#8c8475] hover:border-emerald-300'"
                >
                  Session Key
                </button>
              </div>
              <p class="text-xs text-[#b5b0a6]">
                {{ oauthMode === 'oauth' ? '完整 scope，支持 profile、用量查询等'
                  : oauthMode === 'setup_token' ? '仅 user:inference scope，有效期 1 年'
                  : '粘贴 claude.ai 的 sessionKey（sk-ant-sid01-…），自动完成授权，无需浏览器' }}
              </p>
            </div>
            <div class="space-y-2">
              <Label class="text-[#5c5647] text-sm">代理地址（选填）</Label>
              <Input
                v-model="oauthProxyUrl"
                placeholder="http:// 或 socks5://"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20"
              />
            </div>
            <!-- Session Key 一步录号 -->
            <template v-if="oauthMode === 'session_key'">
              <div class="space-y-2">
                <Label class="text-[#5c5647] text-sm">Session Key <span class="text-red-500">*</span></Label>
                <Textarea
                  v-model="oauthSessionKey"
                  :rows="2"
                  placeholder="粘贴 claude.ai 的 sessionKey（sk-ant-sid01-…）"
                  class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 font-mono text-sm"
                />
                <p class="text-xs text-[#b5b0a6]">浏览器登录 claude.ai 后，从 Cookie 里复制 sessionKey 的值</p>
              </div>
              <Button
                @click="exchangeSessionKeyFlow"
                :disabled="oauthLoading || !oauthSessionKey.trim()"
                class="w-full bg-emerald-500 hover:bg-emerald-600 text-white font-medium rounded-xl transition-all duration-200"
              >
                {{ oauthLoading ? '授权中...' : '授权并录号' }}
              </Button>
            </template>
            <Button
              v-else
              @click="generateOAuthUrl"
              :disabled="oauthLoading"
              class="w-full bg-amber-500 hover:bg-amber-600 text-white font-medium rounded-xl transition-all duration-200"
            >
              {{ oauthLoading ? '生成中...' : '生成授权链接' }}
            </Button>
          </template>

          <!-- 步骤 2：显示链接 + 输入 code -->
          <template v-if="oauthStep === 'exchange'">
            <div class="space-y-2">
              <Label class="text-[#5c5647] text-sm">授权链接</Label>
              <div class="relative">
                <Textarea
                  :model-value="oauthAuthUrl"
                  readonly
                  :rows="3"
                  class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] font-mono text-xs pr-16"
                />
                <div class="absolute right-2 top-2 flex gap-1">
                  <button
                    type="button"
                    @click="copyText(oauthAuthUrl)"
                    class="px-2 py-1 text-xs bg-[#c4704f] text-white rounded-md hover:bg-[#b5623f] transition-colors"
                  >
                    复制
                  </button>
                </div>
              </div>
              <a
                :href="oauthAuthUrl"
                target="_blank"
                rel="noopener noreferrer"
                class="inline-flex items-center gap-1 text-xs text-amber-600 hover:text-amber-700 underline"
              >
                点击打开授权页面 ↗
              </a>
            </div>
            <div class="rounded-lg bg-amber-50 border border-amber-200 px-3 py-2 text-xs text-amber-700 space-y-1">
              <p class="font-medium">操作步骤：</p>
              <ol class="list-decimal list-inside space-y-0.5 text-amber-600">
                <li>点击上方链接或复制到浏览器打开</li>
                <li>完成 Claude 登录授权</li>
                <li>授权完成后，从回调页面复制授权码</li>
                <li>将授权码粘贴到下方输入框</li>
              </ol>
            </div>
            <div class="space-y-2">
              <Label class="text-[#5c5647] text-sm">授权码 <span class="text-red-500">*</span></Label>
              <Textarea
                v-model="oauthCode"
                :rows="2"
                placeholder="粘贴授权码（authorization code）"
                class="bg-[#f9f6f1] border-[#e8e2d9] text-[#29261e] placeholder-[#b5b0a6] focus:border-[#c4704f] focus:ring-[#c4704f]/20 font-mono text-sm"
              />
            </div>
            <div class="flex gap-2">
              <Button
                variant="ghost"
                @click="oauthStep = 'generate'"
                class="text-[#8c8475] hover:text-[#29261e] hover:bg-[#f0ebe4]"
              >
                返回
              </Button>
              <Button
                @click="exchangeOAuthCode"
                :disabled="oauthLoading || !oauthCode.trim()"
                class="flex-1 bg-amber-500 hover:bg-amber-600 text-white font-medium rounded-xl transition-all duration-200"
              >
                {{ oauthLoading ? '交换中...' : '交换 Token' }}
              </Button>
            </div>
          </template>

          <!-- 步骤 3：显示结果 -->
          <template v-if="oauthStep === 'done' && oauthResult">
            <div class="rounded-lg bg-emerald-50 border border-emerald-200 px-3 py-2 text-sm text-emerald-700 font-medium">
              授权成功
            </div>
            <div class="space-y-3">
              <div v-if="oauthResult.email_address" class="space-y-1">
                <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">邮箱</p>
                <p class="text-sm text-[#29261e]">{{ oauthResult.email_address }}</p>
              </div>
              <div v-if="oauthResult.account_uuid" class="space-y-1">
                <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">Account UUID</p>
                <p class="font-mono text-xs text-[#5c5647] break-all">{{ oauthResult.account_uuid }}</p>
              </div>
              <div v-if="oauthResult.organization_uuid" class="space-y-1">
                <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">Organization UUID</p>
                <p class="font-mono text-xs text-[#5c5647] break-all">{{ oauthResult.organization_uuid }}</p>
              </div>
              <div class="space-y-1">
                <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">Access Token</p>
                <div class="flex items-center gap-2">
                  <p class="font-mono text-xs text-[#8c8475] truncate flex-1">{{ oauthResult.access_token.slice(0, 30) }}...</p>
                  <button
                    type="button"
                    @click="copyText(oauthResult.access_token)"
                    class="px-2 py-0.5 text-[10px] bg-[#f0ebe4] text-[#5c5647] rounded hover:bg-[#e8e2d9] transition-colors flex-shrink-0"
                  >
                    复制
                  </button>
                </div>
              </div>
              <div v-if="oauthResult.refresh_token" class="space-y-1">
                <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">Refresh Token</p>
                <div class="flex items-center gap-2">
                  <p class="font-mono text-xs text-[#8c8475] truncate flex-1">{{ oauthResult.refresh_token.slice(0, 30) }}...</p>
                  <button
                    type="button"
                    @click="copyText(oauthResult.refresh_token)"
                    class="px-2 py-0.5 text-[10px] bg-[#f0ebe4] text-[#5c5647] rounded hover:bg-[#e8e2d9] transition-colors flex-shrink-0"
                  >
                    复制
                  </button>
                </div>
              </div>
              <div class="space-y-1">
                <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">Scope</p>
                <p class="text-xs text-[#8c8475]">{{ oauthResult.scope || '—' }}</p>
              </div>
              <div class="space-y-1">
                <p class="text-[10px] text-[#b5b0a6] uppercase tracking-wider">过期时间</p>
                <p class="text-xs text-[#8c8475]">{{ new Date(oauthResult.expires_at * 1000).toLocaleString('zh-CN') }}</p>
              </div>
            </div>
            <div class="flex gap-2 pt-2">
              <Button
                variant="ghost"
                @click="showOAuthFlow = false"
                class="text-[#8c8475] hover:text-[#29261e] hover:bg-[#f0ebe4]"
              >
                关闭
              </Button>
              <Button
                @click="applyOAuthResult"
                class="flex-1 bg-[#c4704f] hover:bg-[#b5623f] text-white font-medium rounded-xl transition-all duration-200"
              >
                填入并创建账号
              </Button>
            </div>
          </template>
        </div>
      </DialogContent>
    </Dialog>
  </div>
</template>
