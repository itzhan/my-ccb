# 盾 vs 矛 对照清单

> 左 = Anthropic 的检测点(盾) / 右 = 我们的实现(矛) / 状态 = ✅ 已实现 / ⚠️ 部分 / ❌ 未实现

---

## L1 网络层指纹

| 🛡️ 盾(Anthropic 怎么查) | ⚔️ 矛(我们怎么应对) | 状态 |
|---|---|---|
| TLS JA3:`d871d02cecbde59abbf8f4806134addf` | `craftls` fork + `BUN_CIPHER` 17 项 + `BUN_EXTENSION` 14 项严格顺序 | ✅ |
| TLS 密钥交换:X25519MLKEM768 混合 | `X25519Mlkem768KxGroup` 自实(ML-KEM-768 + X25519 = 1216B pubkey) | ✅ |
| TLS 扩展顺序:`0,23,65281,10,11,35,16,5,13,18,51,45,43,21` | `BUN_EXTENSION` 数组按此顺序排列,`shuffle_extensions=false` | ✅ |
| TLS GREASE(`0x?A?A` cipher / extension) | 未实现,cipher 列表是纯固定 | ❌ |
| **HTTP/2 SETTINGS 帧**(INITIAL_WINDOW_SIZE / MAX_FRAME_SIZE 等) | 依赖 reqwest 默认 h2 crate,未对齐 undici | ❌ |
| **HTTP/2 伪头序**(`:method :path :scheme :authority`) | 依赖 reqwest 默认,未控制 | ❌ |
| HTTP/2 WINDOW_UPDATE / PRIORITY 帧 | 未实现 | ❌ |
| HTTP/1 头序(undici 风格固定顺序) | `CANONICAL_HEADER_ORDER` 显式排序 | ✅ |
| HTTP/1 头大小写(`User-Agent` vs `user-agent`) | `HEADER_WIRE_CASING` 映射还原 | ✅ |
| `authorization` 槽位(必须在 x-app 之前) | `inject_auth_before_xapp` | ✅ |
| `accept-encoding: gzip, deflate, br, zstd` 值序 | `rewrite_headers` 写死该值 | ✅ |
| 传输头位置(host/content-length 在末尾) | reqwest 自动追加(非 undici,**位置无法精确控制**) | ⚠️ |

---

## L2 客户端身份层

| 🛡️ 盾 | ⚔️ 矛 | 状态 |
|---|---|---|
| UA 格式:`claude-cli/2.1.156 (external, cli)` | `parse_cli_version` 解析 + `normalize_os_headers_ordered` 钉死 | ✅ |
| UA 版本 ↔ X-Stainless-Package-Version 互洽 | `extract_captured_coords` 吸取首请求版本三元组 | ✅ |
| X-Stainless-OS(`MacOS/Windows/Linux`) | `stainless_os_from_platform` 把 `darwin→MacOS` 修正 | ✅ |
| X-Stainless-OS ↔ system prompt 里 Platform 互洽 | `normalize_cc_identity` 强制同步 | ✅ |
| 多人共号"一台机器=一个版本" | 注释明确认知 + 实现钉死 | ✅ |
| `anthropic-beta` flag 集合按模型分类 | `beta_header_for_model`(haiku / 非 haiku 两套) | ✅ |
| `anthropic-beta` 主动去掉 `context-1m-2025-08-07` | 已去除,避免订阅号 429 | ✅ |
| `anthropic-dangerous-direct-browser-access: true` | 必发 + 兜底 entry | ✅ |
| `x-app: cli` / `anthropic-version: 2023-06-01` | API 模式写死 | ✅ |
| `x-claude-code-session-id`(UUID v4) | `generate_session_uuid` 设了 version/variant 位 | ✅ |

---

## L3 请求体语义层

| 🛡️ 盾 | ⚔️ 矛 | 状态 |
|---|---|---|
| `metadata.user_id` 三元组结构 | `inject_metadata_user_id` 生成 / `rewrite_metadata_user_id` 改写 | ✅ |
| `device_id` 必须 64 hex | `generate_device_id`(32 字节 hex) | ✅ |
| `session_id` 必须合法 UUID v4 | `generate_session_uuid` 打了 v4/variant 位 | ✅ |
| `account_uuid` 合法 UUID v4 格式 | `derive_account_uuid` **没打 v4/variant 位**,严格校验会 fail | ⚠️ |
| user_id 不能多 email/org 等字段 | `retain` 白名单只留 3 个 key | ✅ |
| `cc_version=X.Y.Z.<3hex>` 指纹戳 | `compute_cch`(SHA256 + salt + 位置 [4,7,20]) | ✅ |
| `cch=<5hex>` 密封戳 | `compute_cch_attestation`(xxh64 seed `0x6E52736AC806831E`)| ✅ |
| `cch` 输入是末条 user 文本(非首条) | `extract_last_user_text` 已对齐真包 | ✅ |
| 改 body 后必须重算 cch | `reattest_cch` 改写后置占位符 + 序列化后填值 | ✅ |
| billing 块独占 system[0],不带 cache_control | `isolate_billing_block` 自动剥离 | ✅ |
| billing 变化不破坏 prompt cache | 已验证 + 测试 `cch_attestation_is_self_consistent` | ✅ |
| system banner 首句必须是 CC 招牌句 | `CLAUDE_CODE_SYSTEM_PROMPT` 注入 | ✅ |
| **banner 后续填充**(环境/工具列表/system-reminder)| 注入模式只有单行,**短得不像真 CC** | ❌ |
| 删 `temperature/top_p/top_k/stop_sequences/tool_choice` | `rewrite_messages` 注入分支主动删 | ✅ |
| `tools` 字段必须存在(可空数组) | `obj.entry("tools").or_insert(...)` | ✅ |
| `stream: true` | 强制 true | ✅ |
| `max_tokens ≤ 32768`(否则降到 16384) | `rewrite_messages` 截断 | ✅ |
| Platform/Shell/OS Version 归一 | `PLATFORM_REGEX / SHELL_REGEX / OS_VERSION_REGEX` | ✅ |
| 工作目录归一(/Users/vuser/<vproj>) | `WORKING_DIR_REGEX` + 兜底 `HOME_ANCHOR_REGEX` | ✅ |
| home slug 归一(`-Users-vuser-…`) | `HOME_SLUG_REGEX` + `PROJECTS_SLUG_REGEX` | ✅ |
| Windows 路径转 Unix(去盘符) | `HOME_WIN_FULL_REGEX` + `WIN_SLUG_REGEX` | ✅ |
| Git user 归一(虚拟身份池) | `scrub_git_user_in_reminders` + `IDENTITY_POOL` | ✅ |
| 虚拟项目按 session_id 稳定派生 | `virtual_project(session_id)` | ✅ |
| **tools[].description / input_schema 里的路径泄漏** | `normalize_cc_identity` 没扫到 tools 数组 | ❌ |
| 空 text 块剥离(真 CC 不会发空块) | `strip_empty_text_blocks` 递归剥(含 tool_result) | ✅ |
| API 模式剥 system 里的 cache_control | `strip_cache_control` | ✅ |

---

## L4 遥测自洽层

| 🛡️ 盾 | ⚔️ 矛 | 状态 |
|---|---|---|
| `/event_logging/batch` 里 device_id 一致 | `rewrite_event_batch` 改写 | ✅ |
| `email / account_uuid / organization_uuid` 一致 | 全部改写 | ✅ |
| 清除 `baseUrl / base_url / gateway`(代理痕迹) | 主动 remove | ✅ |
| `env` 字段(platform/arch/node_version/terminal…) | `build_canonical_env_map` 全套构造 | ✅ |
| `process` 字段(rss/heap/constrainedMemory) | `rewrite_process`,rss/heap 区间随机,memory 账号绑定 | ✅ |
| `process` 是 base64 嵌套 JSON | 自动 decode→改→encode | ✅ |
| `additional_metadata`(base64 嵌套)清 baseUrl | `rewrite_additional_metadata` | ✅ |
| `user_attributes`(GrowthBook JSON 字符串) | `rewrite_user_attributes_json` | ✅ |
| `/api/eval/{key}` attributes 一致性 | `rewrite_growthbook_eval` | ✅ |
| **杀手字段 `apiBaseUrlHost`**(暴露代理地址) | 主动 remove | ✅ |
| `subscriptionType` 对齐账号 | 主动写入 | ✅ |
| `platform / appVersion` 对齐 env | 主动写入 | ✅ |
| 通用兜底:body 任意位置的 device_id/email | `rewrite_generic_identity` | ✅ |

---

## L5 行为层

| 🛡️ 盾 | ⚔️ 矛 | 状态 |
|---|---|---|
| 同账号多 device_id 并发 | sticky session 把单会话钉死在一个账号 | ✅ |
| sticky 哈希维度(UA + 内容) | `generate_session_hash` 含 UA + 首条内容 | ✅ |
| **sticky 跨小时漂移**(`hour_window` 维度) | 每小时换 hash → 同会话换号 → device_id 漂移 | ❌ |
| sticky TTL 24h | `STICKY_SESSION_TTL` | ✅ |
| 5h 配额超限主动释放 sticky | `select_account` 检查 5h cost | ✅ |
| 429 触发隔离 | `FALLBACK_QUARANTINE 5h` | ✅ |
| **单账号并发上限对齐"真人合理值"** | 有 slot 控制,但默认值无依据 | ⚠️ |
| **OAuth refresh 节奏错峰/抖动** | 没看到主动错峰 | ❌ |
| **prompt 缓存命中率监控** | 没追踪 cache_read / cache_creation 比例 | ❌ |
| **跨大区 IP 切换速度检测** | 无对应防御(IP 由出网代理决定) | ❌ |
| 账号优先级 + 活着优先排序 | 已实现 | ✅ |

---

## 📊 总分

| 层 | 已实现 | 部分 | 未实现 | 完成度 |
|---|---|---|---|---|
| L1 网络层 | 7 | 1 | 4 | 58% |
| L2 客户端身份 | 10 | 0 | 0 | **100%** |
| L3 请求体语义 | 22 | 1 | 2 | 88% |
| L4 遥测 | 13 | 0 | 0 | **100%** |
| L5 行为层 | 5 | 1 | 4 | 50% |
| **合计** | **57** | **3** | **10** | **82%** |

---

## ❌ 未实现清单(10 项,按优先级排序)

### 🔴 高优先级(单点击穿风险)
1. **HTTP/2 SETTINGS 帧对齐 undici**
2. **HTTP/2 伪头序对齐**
3. **sticky 跨小时 device_id 漂移**(去掉 hour_window 或绑账号变 session_id)
4. **banner 后续填充**(造长成真 CC 完整结构)

### 🟡 中优先级
5. **tools[].description 路径 scrub**(扩 `normalize_cc_identity` 扫描范围)
6. **TLS GREASE**(抓真包确认是否需要补)
7. **HTTP/2 WINDOW_UPDATE / PRIORITY 帧**

### 🟢 低优先级(长期才发酵)
8. **OAuth refresh 节奏错峰/抖动**
9. **单账号并发上限对齐真人画像**
10. **prompt 缓存命中率监控告警**

### ⚠️ 部分实现需修正(3 项)
- `derive_account_uuid` 补 UUID v4 version/variant 位
- 传输头位置(reqwest 自动追加,无法精确控制 — 和 #1/#2 一起解决)
- 并发上限设依据(和 #9 一起)
