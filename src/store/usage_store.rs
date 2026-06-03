use sqlx::{AnyPool, Row};

use crate::model::usage::{UsageLogRow, UsageRecord, UsageStatRow};

/// 批量写入明细 + 同事务累加每日汇总（写入管道调用）。
pub async fn batch_insert_and_rollup(
    pool: &AnyPool,
    records: &[UsageRecord],
) -> Result<(), sqlx::Error> {
    if records.is_empty() {
        return Ok(());
    }
    let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut tx = pool.begin().await?;

    for r in records {
        sqlx::query(
            r#"INSERT INTO usage_logs
                (token_id, account_id, request_id, model, input_tokens, output_tokens,
                 cache_creation_tokens, cache_read_tokens, cache_creation_5m_tokens,
                 cache_creation_1h_tokens, stream, status_code, duration_ms, error,
                 client_ip, user_agent, path, session_id, user_id, proxy, req_headers, resp_headers)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)"#,
        )
        .bind(r.token_id)
        .bind(r.account_id)
        .bind(&r.request_id)
        .bind(&r.model)
        .bind(r.input_tokens)
        .bind(r.output_tokens)
        .bind(r.cache_creation_tokens)
        .bind(r.cache_read_tokens)
        .bind(r.cache_creation_5m_tokens)
        .bind(r.cache_creation_1h_tokens)
        .bind(if r.stream { 1i64 } else { 0i64 })
        .bind(r.status_code as i64)
        .bind(r.duration_ms)
        .bind(&r.error)
        .bind(&r.client_ip)
        .bind(&r.user_agent)
        .bind(&r.path)
        .bind(&r.session_id)
        .bind(&r.user_id)
        .bind(&r.proxy)
        .bind(&r.req_headers)
        .bind(&r.resp_headers)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"INSERT INTO usage_daily
                (day, token_id, account_id, model, input_tokens, output_tokens,
                 cache_creation_tokens, cache_read_tokens, req_count)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,1)
               ON CONFLICT(day, token_id, account_id, model) DO UPDATE SET
                 input_tokens          = usage_daily.input_tokens + excluded.input_tokens,
                 output_tokens         = usage_daily.output_tokens + excluded.output_tokens,
                 cache_creation_tokens = usage_daily.cache_creation_tokens + excluded.cache_creation_tokens,
                 cache_read_tokens     = usage_daily.cache_read_tokens + excluded.cache_read_tokens,
                 req_count             = usage_daily.req_count + 1"#,
        )
        .bind(&day)
        .bind(r.token_id)
        .bind(r.account_id)
        .bind(&r.model)
        .bind(r.input_tokens)
        .bind(r.output_tokens)
        .bind(r.cache_creation_tokens)
        .bind(r.cache_read_tokens)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await
}

/// 明细分页查询（可按令牌/账号/模型/结果/时间区间过滤）。
/// result: Some("error")=仅失败(>=400)，Some("success")=仅成功(<400)，None=全部。
pub async fn list_logs(
    pool: &AnyPool,
    token_id: Option<i64>,
    account_id: Option<i64>,
    model: Option<&str>,
    result: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<(Vec<UsageLogRow>, i64), sqlx::Error> {
    let mut conds: Vec<String> = Vec::new();
    let mut idx = 0;
    if token_id.is_some() {
        idx += 1;
        conds.push(format!("token_id = ${}", idx));
    }
    if account_id.is_some() {
        idx += 1;
        conds.push(format!("account_id = ${}", idx));
    }
    if model.is_some() {
        idx += 1;
        conds.push(format!("model = ${}", idx));
    }
    if start.is_some() {
        idx += 1;
        conds.push(format!("created_at >= ${}", idx));
    }
    if end.is_some() {
        idx += 1;
        conds.push(format!("created_at <= ${}", idx));
    }
    match result {
        Some("error") => conds.push("status_code >= 400".to_string()),
        Some("success") => conds.push("status_code < 400".to_string()),
        _ => {}
    }
    let where_clause = if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    };

    // total
    let count_sql = format!("SELECT COUNT(*) FROM usage_logs {}", where_clause);
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(v) = token_id {
        cq = cq.bind(v);
    }
    if let Some(v) = account_id {
        cq = cq.bind(v);
    }
    if let Some(v) = model {
        cq = cq.bind(v.to_string());
    }
    if let Some(v) = start {
        cq = cq.bind(v.to_string());
    }
    if let Some(v) = end {
        cq = cq.bind(v.to_string());
    }
    let total = cq.fetch_one(pool).await.unwrap_or(0);

    let offset = (page.max(1) - 1) * page_size;
    let list_sql = format!(
        "SELECT id, token_id, account_id, request_id, model, input_tokens, output_tokens, \
         cache_creation_tokens, cache_read_tokens, cache_creation_5m_tokens, \
         cache_creation_1h_tokens, stream, status_code, duration_ms, error, \
         client_ip, user_agent, path, session_id, user_id, proxy, req_headers, resp_headers, created_at \
         FROM usage_logs {} ORDER BY id DESC LIMIT {} OFFSET {}",
        where_clause, page_size, offset
    );
    let mut lq = sqlx::query(&list_sql);
    if let Some(v) = token_id {
        lq = lq.bind(v);
    }
    if let Some(v) = account_id {
        lq = lq.bind(v);
    }
    if let Some(v) = model {
        lq = lq.bind(v.to_string());
    }
    if let Some(v) = start {
        lq = lq.bind(v.to_string());
    }
    if let Some(v) = end {
        lq = lq.bind(v.to_string());
    }
    let rows = lq.fetch_all(pool).await?;
    let out = rows
        .into_iter()
        .map(|row| UsageLogRow {
            id: row.try_get("id").unwrap_or(0),
            token_id: row.try_get("token_id").unwrap_or(0),
            account_id: row.try_get("account_id").unwrap_or(0),
            request_id: row.try_get("request_id").unwrap_or_default(),
            model: row.try_get("model").unwrap_or_default(),
            input_tokens: row.try_get("input_tokens").unwrap_or(0),
            output_tokens: row.try_get("output_tokens").unwrap_or(0),
            cache_creation_tokens: row.try_get("cache_creation_tokens").unwrap_or(0),
            cache_read_tokens: row.try_get("cache_read_tokens").unwrap_or(0),
            cache_creation_5m_tokens: row.try_get("cache_creation_5m_tokens").unwrap_or(0),
            cache_creation_1h_tokens: row.try_get("cache_creation_1h_tokens").unwrap_or(0),
            stream: row.try_get::<i64, _>("stream").unwrap_or(0) != 0,
            status_code: row.try_get("status_code").unwrap_or(0),
            duration_ms: row.try_get("duration_ms").unwrap_or(0),
            error: row.try_get("error").unwrap_or_default(),
            client_ip: row.try_get("client_ip").unwrap_or_default(),
            user_agent: row.try_get("user_agent").unwrap_or_default(),
            path: row.try_get("path").unwrap_or_default(),
            session_id: row.try_get("session_id").unwrap_or_default(),
            user_id: row.try_get("user_id").unwrap_or_default(),
            proxy: row.try_get("proxy").unwrap_or_default(),
            req_headers: row.try_get("req_headers").unwrap_or_default(),
            resp_headers: row.try_get("resp_headers").unwrap_or_default(),
            created_at: row.try_get("created_at").unwrap_or_default(),
        })
        .collect();
    Ok((out, total))
}

/// 聚合统计（读 usage_daily）。group_by: token | account | model | day | total。
pub async fn stats(
    pool: &AnyPool,
    group_by: &str,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<Vec<UsageStatRow>, sqlx::Error> {
    let (key_expr, group_clause) = match group_by {
        "account" => ("CAST(account_id AS TEXT)", "GROUP BY account_id"),
        "model" => ("model", "GROUP BY model"),
        "day" => ("day", "GROUP BY day"),
        "total" => ("''", ""),
        _ => ("CAST(token_id AS TEXT)", "GROUP BY token_id"),
    };
    let mut conds: Vec<String> = Vec::new();
    let mut idx = 0;
    if start.is_some() {
        idx += 1;
        conds.push(format!("day >= ${}", idx));
    }
    if end.is_some() {
        idx += 1;
        conds.push(format!("day <= ${}", idx));
    }
    let where_clause = if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    };
    let sql = format!(
        "SELECT {} AS k, \
         COALESCE(SUM(input_tokens),0) AS i, COALESCE(SUM(output_tokens),0) AS o, \
         COALESCE(SUM(cache_creation_tokens),0) AS cc, COALESCE(SUM(cache_read_tokens),0) AS cr, \
         COALESCE(SUM(req_count),0) AS n \
         FROM usage_daily {} {} ORDER BY i DESC",
        key_expr, where_clause, group_clause
    );
    let mut q = sqlx::query(&sql);
    if let Some(v) = start {
        q = q.bind(v.to_string());
    }
    if let Some(v) = end {
        q = q.bind(v.to_string());
    }
    let rows = q.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|row| UsageStatRow {
            key: row.try_get("k").unwrap_or_default(),
            input_tokens: row.try_get("i").unwrap_or(0),
            output_tokens: row.try_get("o").unwrap_or(0),
            cache_creation_tokens: row.try_get("cc").unwrap_or(0),
            cache_read_tokens: row.try_get("cr").unwrap_or(0),
            req_count: row.try_get("n").unwrap_or(0),
        })
        .collect())
}

/// 清理某时间点之前的明细（汇总表保留）。返回删除行数无关紧要。
pub async fn prune_logs_before(pool: &AnyPool, cutoff_iso: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM usage_logs WHERE created_at < $1")
        .bind(cutoff_iso)
        .execute(pool)
        .await?;
    Ok(())
}
