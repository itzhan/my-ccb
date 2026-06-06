// 模型计价(USD per 1M tokens),与后端 src/model/pricing.rs 保持一致。
// 用于在调用记录里实时估算单次请求的扣费。

interface ModelPricing {
  input: number;
  output: number;
  cacheWrite5m: number;
  cacheWrite1h: number;
  cacheRead: number;
}

function pricingFor(model: string): ModelPricing {
  const m = (model || '').toLowerCase().replace(/-/g, '.');

  // Opus 4.6/4.7/4.8 标准档(订阅常见)
  if ((m.includes('opus.4.8') || m.includes('opus.4.7') || m.includes('opus.4.6')) && !m.includes('fast'))
    return { input: 5, output: 25, cacheWrite5m: 6.25, cacheWrite1h: 10, cacheRead: 0.5 };

  // Opus fast 档(API 付费版)
  if (m.includes('opus.4.8') && m.includes('fast'))
    return { input: 10, output: 50, cacheWrite5m: 12.5, cacheWrite1h: 20, cacheRead: 1 };
  if ((m.includes('opus.4.7') || m.includes('opus.4.6')) && m.includes('fast'))
    return { input: 30, output: 150, cacheWrite5m: 37.5, cacheWrite1h: 60, cacheRead: 3 };

  // Opus 4 / 4.1 / 4.5 老版
  if (m.includes('opus.4.5') || m.includes('opus.4.1') || m.includes('opus.4'))
    return { input: 15, output: 75, cacheWrite5m: 18.75, cacheWrite1h: 30, cacheRead: 1.5 };

  // Haiku 4.5
  if (m.includes('haiku'))
    return { input: 1, output: 5, cacheWrite5m: 1.25, cacheWrite1h: 2, cacheRead: 0.1 };

  // Sonnet 4.x(默认兜底)
  return { input: 3, output: 15, cacheWrite5m: 3.75, cacheWrite1h: 6, cacheRead: 0.3 };
}

/** 计算单次请求的美元成本。 */
export function calculateCost(
  model: string,
  inputTokens: number,
  outputTokens: number,
  cacheWrite5mTokens: number,
  cacheWrite1hTokens: number,
  cacheReadTokens: number,
): number {
  const p = pricingFor(model);
  const mtok = (t: number) => Math.max(0, t || 0) / 1_000_000;
  return (
    mtok(inputTokens) * p.input +
    mtok(outputTokens) * p.output +
    mtok(cacheWrite5mTokens) * p.cacheWrite5m +
    mtok(cacheWrite1hTokens) * p.cacheWrite1h +
    mtok(cacheReadTokens) * p.cacheRead
  );
}

/** 格式化美元成本:$0.0123 / $1.23,过小显示 <$0.0001,零显示 -。 */
export function fmtCost(usd: number): string {
  if (!usd || usd <= 0) return '-';
  if (usd < 0.0001) return '<$0.0001';
  return '$' + usd.toFixed(usd < 1 ? 4 : 2);
}
