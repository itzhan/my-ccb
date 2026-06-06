//! 模型计价表 (USD per 1M tokens),价格参考 OpenRouter 公开数据。
//!
//! Anthropic 缓存价格规则:
//!   - 5 分钟缓存写入 = 1.25 × input
//!   - 1 小时缓存写入 = 2.00 × input
//!   - 缓存读取        = 0.10 × input
//!
//! 模型 id 匹配做了模糊处理 —— claude-opus-4-8 / claude-opus-4.8 / 内部别名都能命中。

#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    pub input: f64,
    pub output: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
}

/// 按模型 id 返回价格。未识别的模型按 Sonnet 价兜底。
pub fn pricing_for(model: &str) -> ModelPricing {
    let m = model.to_lowercase().replace('-', ".");

    // --- Opus 4.6 / 4.7 / 4.8 标准档(订阅常见) ---
    if (m.contains("opus.4.8") || m.contains("opus.4.7") || m.contains("opus.4.6"))
        && !m.contains("fast")
    {
        return ModelPricing {
            input: 5.0,
            output: 25.0,
            cache_write_5m: 6.25,
            cache_write_1h: 10.0,
            cache_read: 0.5,
        };
    }

    // --- Opus fast 档(API 付费版) ---
    if m.contains("opus.4.8") && m.contains("fast") {
        return ModelPricing {
            input: 10.0,
            output: 50.0,
            cache_write_5m: 12.5,
            cache_write_1h: 20.0,
            cache_read: 1.0,
        };
    }
    if (m.contains("opus.4.7") || m.contains("opus.4.6")) && m.contains("fast") {
        return ModelPricing {
            input: 30.0,
            output: 150.0,
            cache_write_5m: 37.5,
            cache_write_1h: 60.0,
            cache_read: 3.0,
        };
    }

    // --- Opus 4 / 4.1 / 4.5 老版(已退役) ---
    if m.contains("opus.4.5") || m.contains("opus.4.1") || m.contains("opus.4") {
        return ModelPricing {
            input: 15.0,
            output: 75.0,
            cache_write_5m: 18.75,
            cache_write_1h: 30.0,
            cache_read: 1.5,
        };
    }

    // --- Haiku 4.5 ---
    if m.contains("haiku") {
        return ModelPricing {
            input: 1.0,
            output: 5.0,
            cache_write_5m: 1.25,
            cache_write_1h: 2.0,
            cache_read: 0.1,
        };
    }

    // --- Sonnet 4.x(默认兜底) ---
    ModelPricing {
        input: 3.0,
        output: 15.0,
        cache_write_5m: 3.75,
        cache_write_1h: 6.0,
        cache_read: 0.3,
    }
}

/// 计算单次请求的美元成本。
///
/// `cache_write_5m_tokens` + `cache_write_1h_tokens` 分别按 1.25x / 2x input 计费;
/// 如果 SSE usage 没区分(只有合并的 cache_creation_tokens),
/// 调用方应把它当作 5m 缓存写入。
pub fn calculate_cost(
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_write_5m_tokens: i64,
    cache_write_1h_tokens: i64,
    cache_read_tokens: i64,
) -> f64 {
    let p = pricing_for(model);
    let mtok = |t: i64| (t.max(0) as f64) / 1_000_000.0;
    mtok(input_tokens) * p.input
        + mtok(output_tokens) * p.output
        + mtok(cache_write_5m_tokens) * p.cache_write_5m
        + mtok(cache_write_1h_tokens) * p.cache_write_1h
        + mtok(cache_read_tokens) * p.cache_read
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_48_standard_matches_openrouter() {
        let p = pricing_for("claude-opus-4-8");
        assert_eq!(p.input, 5.0);
        assert_eq!(p.output, 25.0);
        assert_eq!(p.cache_write_5m, 6.25);
        assert_eq!(p.cache_read, 0.5);

        let p2 = pricing_for("claude-opus-4.8");
        assert_eq!(p2.input, 5.0);
    }

    #[test]
    fn sonnet_46_matches() {
        let p = pricing_for("claude-sonnet-4-6");
        assert_eq!(p.input, 3.0);
        assert_eq!(p.output, 15.0);
    }

    #[test]
    fn haiku_45_matches() {
        let p = pricing_for("claude-haiku-4-5");
        assert_eq!(p.input, 1.0);
        assert_eq!(p.output, 5.0);
    }

    #[test]
    fn opus_48_fast_more_expensive() {
        let p = pricing_for("claude-opus-4-8-fast");
        assert_eq!(p.input, 10.0);
        assert_eq!(p.output, 50.0);
    }

    #[test]
    fn unknown_model_falls_back_to_sonnet() {
        let p = pricing_for("claude-future-x-1");
        assert_eq!(p.input, 3.0);
    }

    #[test]
    fn account6_real_consumption() {
        // 账号 6 2026-06-05 2h16min 的真实数据,验证总成本 ≈ $217
        let cost = calculate_cost(
            "claude-opus-4-8",
            19_469,
            239_222,
            33_308_262, // 5m 缓存写入
            0,
            6_380_432,
        );
        assert!((cost - 217.45).abs() < 1.0, "expected ~$217, got ${}", cost);
    }
}
