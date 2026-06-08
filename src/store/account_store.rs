use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::Value;
use sqlx::any::AnyRow;
use sqlx::AnyPool;
use sqlx::Row;

use crate::error::AppError;
use crate::model::account::{Account, AccountStatus};

pub struct AccountStore {
    pool: AnyPool,
    driver: String,
}

impl AccountStore {
    pub fn new(pool: AnyPool, driver: String) -> Self {
        Self { pool, driver }
    }

    fn now_expr(&self) -> &str {
        if self.driver == "sqlite" {
            "strftime('%Y-%m-%dT%H:%M:%SZ','now')"
        } else {
            "NOW()"
        }
    }

    fn is_pg(&self) -> bool {
        self.driver == "postgres"
    }

    fn fmt_time(&self, t: DateTime<Utc>) -> String {
        t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    /// Returns `$N` for SQLite or `$N::TIMESTAMPTZ` for Postgres
    fn ts(&self, n: u32) -> String {
        if self.is_pg() {
            format!("${}::TIMESTAMPTZ", n)
        } else {
            format!("${}", n)
        }
    }

    fn parse_time(row: &AnyRow, col: &str) -> DateTime<Utc> {
        // SQLite returns string, Postgres returns native
        if let Ok(s) = row.try_get::<String, _>(col) {
            NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%SZ")
                .map(|n| n.and_utc())
                .unwrap_or_default()
        } else {
            Utc::now()
        }
    }

    fn parse_optional_time(row: &AnyRow, col: &str) -> Option<DateTime<Utc>> {
        if let Ok(s) = row.try_get::<Option<String>, _>(col) {
            s.and_then(|s| {
                NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%SZ")
                    .map(|n| n.and_utc())
                    .ok()
            })
        } else {
            None
        }
    }

    fn parse_json(row: &AnyRow, col: &str) -> Value {
        if let Ok(s) = row.try_get::<String, _>(col) {
            serde_json::from_str(&s).unwrap_or_else(|_| Value::Object(Default::default()))
        } else {
            Value::Object(Default::default())
        }
    }

    fn row_to_account(row: &AnyRow) -> Account {
        Account {
            id: row.try_get::<i64, _>("id").unwrap_or_default(),
            name: row.try_get::<String, _>("name").unwrap_or_default(),
            email: row.try_get::<String, _>("email").unwrap_or_default(),
            status: row
                .try_get::<String, _>("status")
                .unwrap_or_else(|_| "active".into())
                .into(),
            auth_type: row
                .try_get::<String, _>("auth_type")
                .unwrap_or_else(|_| "setup_token".into())
                .into(),
            setup_token: row.try_get::<String, _>("token").unwrap_or_default(),
            access_token: row.try_get::<String, _>("access_token").unwrap_or_default(),
            refresh_token: row.try_get::<String, _>("refresh_token").unwrap_or_default(),
            expires_at: Self::parse_optional_time(row, "oauth_expires_at"),
            oauth_refreshed_at: Self::parse_optional_time(row, "oauth_refreshed_at"),
            auth_error: row.try_get::<String, _>("auth_error").unwrap_or_default(),
            proxy_url: row.try_get::<String, _>("proxy_url").unwrap_or_default(),
            device_id: row.try_get::<String, _>("device_id").unwrap_or_default(),
            canonical_env: Self::parse_json(row, "canonical_env"),
            canonical_prompt: Self::parse_json(row, "canonical_prompt_env"),
            canonical_process: Self::parse_json(row, "canonical_process"),
            billing_mode: row
                .try_get::<String, _>("billing_mode")
                .unwrap_or_else(|_| "strip".into())
                .into(),
            account_uuid: row.try_get::<Option<String>, _>("account_uuid").unwrap_or(None),
            organization_uuid: row.try_get::<Option<String>, _>("organization_uuid").unwrap_or(None),
            subscription_type: row.try_get::<Option<String>, _>("subscription_type").unwrap_or(None),
            concurrency: row.try_get::<i32, _>("concurrency").unwrap_or(3),
            priority: row.try_get::<i32, _>("priority").unwrap_or(50),
            rate_limited_at: Self::parse_optional_time(row, "rate_limited_at"),
            rate_limit_reset_at: Self::parse_optional_time(row, "rate_limit_reset_at"),
            disable_reason: row.try_get::<String, _>("disable_reason").unwrap_or_default(),
            auto_telemetry: row.try_get::<i32, _>("auto_telemetry").unwrap_or(0) != 0,
            telemetry_count: row.try_get::<i64, _>("telemetry_count").unwrap_or(0),
            rpm_limit: {
                let v = row.try_get::<i32, _>("rpm_limit").unwrap_or(0);
                if v > 0 { Some(v) } else { None }
            },
            usage_data: Self::parse_json(row, "usage_data"),
            usage_fetched_at: Self::parse_optional_time(row, "usage_fetched_at"),
            identity_mode: row.try_get::<String, _>("identity_mode").unwrap_or_default(),
            virtual_user: row.try_get::<String, _>("virtual_user").unwrap_or_default(),
            virtual_git_name: row.try_get::<String, _>("virtual_git_name").unwrap_or_default(),
            path_mode: row.try_get::<String, _>("path_mode").unwrap_or_default(),
            session_mode: row.try_get::<String, _>("session_mode").unwrap_or_default(),
            device_quota: row.try_get::<i32, _>("device_quota").unwrap_or(10),
            session_quota: row.try_get::<i32, _>("session_quota").unwrap_or(20),
            identity_captured_at: Self::parse_optional_time(row, "identity_captured_at"),
            captured_session_id: row
                .try_get::<String, _>("captured_session_id")
                .unwrap_or_default(),
            captured_session_at: Self::parse_optional_time(row, "captured_session_at"),
            recapture_days: row.try_get::<i64, _>("recapture_days").unwrap_or(0),
            max_sessions: row.try_get::<i32, _>("max_sessions").unwrap_or(3),
            allowed_client_types: row
                .try_get::<String, _>("allowed_client_types")
                .unwrap_or_default(),
            window_5h_cost_cap_usd: {
                let v = row.try_get::<f64, _>("window_5h_cost_cap_usd").unwrap_or(0.0);
                if v > 0.0 { Some(v) } else { None }
            },
            created_at: Self::parse_time(row, "created_at"),
            updated_at: Self::parse_time(row, "updated_at"),
        }
    }

    pub async fn create(&self, a: &mut Account) -> Result<(), AppError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) as cnt FROM accounts WHERE email=$1",
        )
        .bind(&a.email)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        if count > 0 {
            return Err(AppError::BadRequest(format!(
                "email {} already exists",
                a.email
            )));
        }

        let env_str = serde_json::to_string(&a.canonical_env).unwrap_or_else(|_| "{}".into());
        let prompt_str =
            serde_json::to_string(&a.canonical_prompt).unwrap_or_else(|_| "{}".into());
        let process_str =
            serde_json::to_string(&a.canonical_process).unwrap_or_else(|_| "{}".into());
        let expires_at = a.expires_at.map(|t| self.fmt_time(t));
        let oauth_refreshed_at = a.oauth_refreshed_at.map(|t| self.fmt_time(t));

        let auto_telemetry_int: i32 = if a.auto_telemetry { 1 } else { 0 };
        let rpm_limit_val: i32 = a.rpm_limit.unwrap_or(0);
        let window_5h_cap_val: f64 = a.window_5h_cost_cap_usd.unwrap_or(0.0);
        let q = format!(
            r#"INSERT INTO accounts (name, email, status, token, proxy_url,
                auth_type, access_token, refresh_token, oauth_expires_at, oauth_refreshed_at, auth_error,
                device_id, canonical_env, canonical_prompt_env, canonical_process,
                billing_mode, account_uuid, organization_uuid, subscription_type,
                concurrency, priority, auto_telemetry, rpm_limit,
                identity_mode, virtual_user, virtual_git_name, recapture_days, max_sessions,
                allowed_client_types, window_5h_cost_cap_usd, path_mode, session_mode,
                device_quota, session_quota)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,{},{},{},$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34)
            RETURNING id, created_at, updated_at"#,
            self.ts(9), self.ts(10), "$11"
        );
        let row: AnyRow = sqlx::query(&q)
        .bind(&a.name)
        .bind(&a.email)
        .bind(a.status.to_string())
        .bind(&a.setup_token)
        .bind(&a.proxy_url)
        .bind(a.auth_type.to_string())
        .bind(&a.access_token)
        .bind(&a.refresh_token)
        .bind(expires_at)
        .bind(oauth_refreshed_at)
        .bind(&a.auth_error)
        .bind(&a.device_id)
        .bind(&env_str)
        .bind(&prompt_str)
        .bind(&process_str)
        .bind(a.billing_mode.to_string())
        .bind(&a.account_uuid)
        .bind(&a.organization_uuid)
        .bind(&a.subscription_type)
        .bind(a.concurrency)
        .bind(a.priority)
        .bind(auto_telemetry_int)
        .bind(rpm_limit_val)
        .bind(&a.identity_mode)
        .bind(&a.virtual_user)
        .bind(&a.virtual_git_name)
        .bind(a.recapture_days)
        .bind(a.max_sessions)
        .bind(&a.allowed_client_types)
        .bind(window_5h_cap_val)
        .bind(&a.path_mode)
        .bind(&a.session_mode)
        .bind(a.device_quota)
        .bind(a.session_quota)
        .fetch_one(&self.pool)
        .await?;

        a.id = row.try_get::<i64, _>("id").unwrap_or_default();
        a.created_at = Self::parse_time(&row, "created_at");
        a.updated_at = Self::parse_time(&row, "updated_at");
        Ok(())
    }

    pub async fn update(&self, a: &Account) -> Result<(), AppError> {
        let expires_at = a.expires_at.map(|t| self.fmt_time(t));
        let oauth_refreshed_at = a.oauth_refreshed_at.map(|t| self.fmt_time(t));
        let auto_telemetry_int: i32 = if a.auto_telemetry { 1 } else { 0 };
        let rpm_limit_val: i32 = a.rpm_limit.unwrap_or(0);
        let window_5h_cap_val: f64 = a.window_5h_cost_cap_usd.unwrap_or(0.0);
        let q = format!(
            r#"UPDATE accounts SET name=$1, email=$2, status=$3, token=$4,
                auth_type=$5, access_token=$6, refresh_token=$7, oauth_expires_at={}, oauth_refreshed_at={},
                auth_error=$10, proxy_url=$11, billing_mode=$12,
                account_uuid=$13, organization_uuid=$14, subscription_type=$15,
                concurrency=$16, priority=$17, auto_telemetry=$18, rpm_limit=$19,
                identity_mode=$20, virtual_user=$21, virtual_git_name=$22, recapture_days=$23,
                max_sessions=$24, allowed_client_types=$25, window_5h_cost_cap_usd=$26, path_mode=$27,
                session_mode=$28, device_quota=$29, session_quota=$30, updated_at={}
            WHERE id=$31"#,
            self.ts(8), self.ts(9), self.now_expr()
        );
        sqlx::query(&q)
            .bind(&a.name)
            .bind(&a.email)
            .bind(a.status.to_string())
            .bind(&a.setup_token)
            .bind(a.auth_type.to_string())
            .bind(&a.access_token)
            .bind(&a.refresh_token)
            .bind(expires_at)
            .bind(oauth_refreshed_at)
            .bind(&a.auth_error)
            .bind(&a.proxy_url)
            .bind(a.billing_mode.to_string())
            .bind(&a.account_uuid)
            .bind(&a.organization_uuid)
            .bind(&a.subscription_type)
            .bind(a.concurrency)
            .bind(a.priority)
            .bind(auto_telemetry_int)
            .bind(rpm_limit_val)
            .bind(&a.identity_mode)
            .bind(&a.virtual_user)
            .bind(&a.virtual_git_name)
            .bind(a.recapture_days)
            .bind(a.max_sessions)
            .bind(&a.allowed_client_types)
            .bind(window_5h_cap_val)
            .bind(&a.path_mode)
            .bind(&a.session_mode)
            .bind(a.device_quota)
            .bind(a.session_quota)
            .bind(a.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 持久化吸取到的版本坐标:更新 canonical_env(version/node_version/package_version)
    /// 与 identity_captured_at。
    pub async fn update_captured_identity(
        &self,
        id: i64,
        canonical_env: &serde_json::Value,
        captured_at: &chrono::DateTime<Utc>,
    ) -> Result<(), AppError> {
        let env_str = serde_json::to_string(canonical_env).unwrap_or_else(|_| "{}".into());
        let at_str = self.fmt_time(*captured_at);
        let q = format!(
            "UPDATE accounts SET canonical_env=$1, identity_captured_at={} WHERE id=$3",
            self.ts(2)
        );
        sqlx::query(&q)
            .bind(&env_str)
            .bind(at_str)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 更新当前对上游呈现的 session_id（每 15-20min 轮换吸取时调用）及其吸取时间。展示用。
    pub async fn update_captured_session(
        &self,
        id: i64,
        session_id: &str,
    ) -> Result<(), AppError> {
        let at_str = self.fmt_time(Utc::now());
        let q = format!(
            "UPDATE accounts SET captured_session_id=$1, captured_session_at={} WHERE id=$3",
            self.ts(2)
        );
        sqlx::query(&q)
            .bind(session_id)
            .bind(at_str)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_oauth_tokens(
        &self,
        id: i64,
        access_token: &str,
        refresh_token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let q = format!(
            r#"UPDATE accounts SET access_token=$1, refresh_token=$2, oauth_expires_at={},
                oauth_refreshed_at={}, auth_error='', updated_at={}
            WHERE id=$5"#,
            self.ts(3), self.ts(4), self.now_expr()
        );
        sqlx::query(&q)
            .bind(access_token)
            .bind(refresh_token)
            .bind(self.fmt_time(expires_at))
            .bind(self.fmt_time(Utc::now()))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_auth_error(&self, id: i64, auth_error: &str) -> Result<(), AppError> {
        let q = format!(
            "UPDATE accounts SET auth_error=$1, updated_at={} WHERE id=$2",
            self.now_expr()
        );
        sqlx::query(&q)
            .bind(auth_error)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status(
        &self,
        id: i64,
        status: AccountStatus,
    ) -> Result<(), AppError> {
        let q = format!(
            "UPDATE accounts SET status=$1, updated_at={} WHERE id=$2",
            self.now_expr()
        );
        sqlx::query(&q)
            .bind(status.to_string())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_rate_limit(
        &self,
        id: i64,
        reset_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let q = format!(
            "UPDATE accounts SET rate_limited_at={}, rate_limit_reset_at={}, updated_at={} WHERE id=$3",
            self.ts(1), self.ts(2), self.now_expr()
        );
        sqlx::query(&q)
            .bind(self.fmt_time(Utc::now()))
            .bind(self.fmt_time(reset_at))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn disable_account(
        &self,
        id: i64,
        status: AccountStatus,
        reason: &str,
        rate_limit_reset_at: Option<DateTime<Utc>>,
    ) -> Result<(), AppError> {
        let q = format!(
            r#"UPDATE accounts SET status=$1, disable_reason=$2,
                rate_limited_at={}, rate_limit_reset_at={}, updated_at={}
            WHERE id=$5"#,
            self.ts(3), self.ts(4), self.now_expr()
        );
        let limited_str = rate_limit_reset_at.map(|_| self.fmt_time(Utc::now()));
        let reset_str = rate_limit_reset_at.map(|t| self.fmt_time(t));
        sqlx::query(&q)
            .bind(status.to_string())
            .bind(reason)
            .bind(limited_str)
            .bind(reset_str)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn enable_account(&self, id: i64) -> Result<(), AppError> {
        let q = format!(
            r#"UPDATE accounts SET status='active', disable_reason='',
                rate_limited_at=NULL, rate_limit_reset_at=NULL, updated_at={}
            WHERE id=$1"#,
            self.now_expr()
        );
        sqlx::query(&q).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn clear_rate_limit(&self, id: i64) -> Result<(), AppError> {
        let q = format!(
            "UPDATE accounts SET rate_limited_at=NULL, rate_limit_reset_at=NULL, updated_at={} WHERE id=$1",
            self.now_expr()
        );
        sqlx::query(&q)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<(), AppError> {
        sqlx::query("DELETE FROM accounts WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Account, AppError> {
        let row: AnyRow = sqlx::query(
            &format!("SELECT {} FROM accounts WHERE id=$1", ACCOUNT_COLS),
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(Self::row_to_account(&row))
    }

    pub async fn list(&self) -> Result<Vec<Account>, AppError> {
        let rows: Vec<AnyRow> = sqlx::query(
            &format!(
                "SELECT {} FROM accounts ORDER BY priority ASC, id ASC",
                ACCOUNT_COLS
            ),
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_account).collect())
    }

    pub async fn count(&self) -> Result<i64, AppError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        Ok(count)
    }

    pub async fn list_paged(&self, page: i64, page_size: i64) -> Result<Vec<Account>, AppError> {
        let offset = (page - 1) * page_size;
        // 活着的(active)优先,异常(error)次之,停用(disabled)垫底;同级按优先级、id。
        // CASE on status 对 SQLite / PostgreSQL 均有效。
        let q = format!(
            "SELECT {} FROM accounts \
             ORDER BY (CASE status WHEN 'active' THEN 0 WHEN 'error' THEN 1 WHEN 'disabled' THEN 2 ELSE 3 END), \
             priority ASC, id ASC LIMIT $1 OFFSET $2",
            ACCOUNT_COLS
        );
        let rows: Vec<AnyRow> = sqlx::query(&q)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(Self::row_to_account).collect())
    }

    pub async fn update_usage(&self, id: i64, usage_data: &str) -> Result<(), AppError> {
        let q = format!(
            "UPDATE accounts SET usage_data=$1, usage_fetched_at={}, updated_at={} WHERE id=$3",
            self.ts(2), self.now_expr()
        );
        sqlx::query(&q)
            .bind(usage_data)
            .bind(self.fmt_time(Utc::now()))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn increment_telemetry_count(&self, id: i64, delta: i64) -> Result<(), AppError> {
        let q = format!(
            "UPDATE accounts SET telemetry_count = telemetry_count + $1, updated_at={} WHERE id=$2",
            self.now_expr()
        );
        sqlx::query(&q)
            .bind(delta)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 计算指定账号"当前 5 小时窗口"内的累计消费(USD)。
    ///
    /// 窗口起始时间用 Anthropic 官方 `usage_data.five_hour.resets_at` 对齐
    /// (resets_at - 5h)。这与 Anthropic 的真实窗口模型一致:首次请求触发 5h
    /// 计时,期间累计,reset 后下一次请求才启动新窗口。
    ///
    /// 兜底:
    /// - 没拉过 usage(resets_at 缺失) → 回退到滚动 5h
    /// - resets_at 已过期 → 当前窗口未启动,返回 0
    pub async fn compute_5h_cost_usd(&self, account_id: i64) -> Result<f64, AppError> {
        // 取 usage_data 推断窗口起始
        let usage_data_str: Option<String> =
            sqlx::query_scalar("SELECT usage_data FROM accounts WHERE id=$1")
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();
        let now = Utc::now();
        let window_start = match window_start_from_usage_data(usage_data_str.as_deref(), now) {
            WindowStart::Aligned(t) => t,
            WindowStart::NewWindowNotStarted => return Ok(0.0),
            WindowStart::FallbackRolling => now - chrono::Duration::hours(5),
        };
        let window_start_str = self.fmt_time(window_start);

        let q = format!(
            r#"SELECT model,
                       COALESCE(SUM(input_tokens),0)             AS in_t,
                       COALESCE(SUM(output_tokens),0)            AS out_t,
                       COALESCE(SUM(cache_creation_5m_tokens),0) AS cw5,
                       COALESCE(SUM(cache_creation_1h_tokens),0) AS cw1,
                       COALESCE(SUM(cache_read_tokens),0)        AS cr_t,
                       COALESCE(SUM(cache_creation_tokens),0)    AS cc_total
                  FROM usage_logs
                 WHERE account_id = $1
                   AND status_code = 200
                   AND created_at >= {}
                 GROUP BY model"#,
            self.ts(2)
        );
        let rows: Vec<AnyRow> = sqlx::query(&q)
            .bind(account_id)
            .bind(window_start_str)
            .fetch_all(&self.pool)
            .await?;
        let mut total = 0.0;
        for row in rows.iter() {
            let model: String = row.try_get("model").unwrap_or_default();
            let in_t: i64 = row.try_get("in_t").unwrap_or(0);
            let out_t: i64 = row.try_get("out_t").unwrap_or(0);
            let cw5: i64 = row.try_get("cw5").unwrap_or(0);
            let cw1: i64 = row.try_get("cw1").unwrap_or(0);
            let cr_t: i64 = row.try_get("cr_t").unwrap_or(0);
            let cc_total: i64 = row.try_get("cc_total").unwrap_or(0);
            // 兜底:老数据只有合并的 cache_creation_tokens,把它当作 5m 缓存写入
            let cw5_effective = if cw5 == 0 && cw1 == 0 { cc_total } else { cw5 };
            total += crate::model::pricing::calculate_cost(
                &model, in_t, out_t, cw5_effective, cw1, cr_t,
            );
        }
        Ok(total)
    }

    pub async fn list_schedulable(&self) -> Result<Vec<Account>, AppError> {
        let q = format!(
            r#"SELECT {} FROM accounts
            WHERE status='active'
              AND (rate_limit_reset_at IS NULL OR rate_limit_reset_at < {})
            ORDER BY priority ASC, id ASC"#,
            ACCOUNT_COLS,
            self.now_expr()
        );
        let rows: Vec<AnyRow> = sqlx::query(&q).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(Self::row_to_account).collect())
    }
}

/// 5h 窗口起始时间的判定结果。
enum WindowStart {
    /// 用 usage_data.five_hour.resets_at - 5h 算出对齐起始
    Aligned(chrono::DateTime<Utc>),
    /// 上一个窗口已 reset,新窗口未启动 → 当前 cost = 0
    NewWindowNotStarted,
    /// 没拉过 usage,回退到滚动 5h
    FallbackRolling,
}

/// 从账号的 usage_data JSON 字符串解析 five_hour.resets_at,
/// 推断当前 5h 窗口的起始时间。
fn window_start_from_usage_data(
    usage_data_json: Option<&str>,
    now: chrono::DateTime<Utc>,
) -> WindowStart {
    let Some(s) = usage_data_json else {
        return WindowStart::FallbackRolling;
    };
    let v: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return WindowStart::FallbackRolling,
    };
    let resets_at_str = v
        .get("five_hour")
        .and_then(|x| x.get("resets_at"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if resets_at_str.is_empty() {
        return WindowStart::FallbackRolling;
    }
    match chrono::DateTime::parse_from_rfc3339(resets_at_str) {
        Ok(resets) => {
            let resets_utc = resets.with_timezone(&Utc);
            if resets_utc > now {
                WindowStart::Aligned(resets_utc - chrono::Duration::hours(5))
            } else {
                // resets_at 已过去,意味着上个窗口已结束、新窗口未启动(下一次请求触发)
                WindowStart::NewWindowNotStarted
            }
        }
        Err(_) => WindowStart::FallbackRolling,
    }
}

const ACCOUNT_COLS: &str = r#"id, name, email, status, token, auth_type, access_token, refresh_token,
    oauth_expires_at, oauth_refreshed_at, auth_error, proxy_url, device_id,
    canonical_env, canonical_prompt_env, canonical_process,
    billing_mode, account_uuid, organization_uuid, subscription_type,
    concurrency, priority, rate_limited_at, rate_limit_reset_at,
    disable_reason, auto_telemetry, telemetry_count, rpm_limit,
    usage_data, usage_fetched_at, identity_mode, virtual_user, virtual_git_name,
    identity_captured_at, recapture_days, max_sessions, allowed_client_types,
    window_5h_cost_cap_usd, path_mode, captured_session_id, captured_session_at, session_mode,
    device_quota, session_quota,
    created_at, updated_at"#;

#[cfg(test)]
mod tests {
    use super::*;

    async fn make_store(driver: &str) -> AccountStore {
        sqlx::any::install_default_drivers();
        let tmp = std::env::temp_dir().join(format!("ccgw_unit_{}.db", rand::random::<u64>()));
        let dsn = format!("sqlite:{}?mode=rwc", tmp.display());
        let pool = AnyPool::connect(&dsn).await.expect("pool");
        AccountStore {
            pool,
            driver: driver.to_string(),
        }
    }

    use chrono::TimeZone;

    #[test]
    fn window_aligned_when_resets_at_future() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 6, 5, 11, 0, 0).unwrap();
        let json = r#"{"five_hour":{"utilization":70.0,"resets_at":"2026-06-05T13:40:00+00:00"}}"#;
        match window_start_from_usage_data(Some(json), now) {
            WindowStart::Aligned(t) => {
                // resets 13:40 - 5h = 08:40
                let expected = chrono::Utc.with_ymd_and_hms(2026, 6, 5, 8, 40, 0).unwrap();
                assert_eq!(t, expected);
            }
            _ => panic!("expected Aligned"),
        }
    }

    #[test]
    fn window_zero_when_resets_at_past() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 6, 5, 14, 0, 0).unwrap();
        let json = r#"{"five_hour":{"utilization":70.0,"resets_at":"2026-06-05T13:40:00+00:00"}}"#;
        assert!(matches!(
            window_start_from_usage_data(Some(json), now),
            WindowStart::NewWindowNotStarted
        ));
    }

    #[test]
    fn window_rolling_when_usage_missing() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 6, 5, 14, 0, 0).unwrap();
        assert!(matches!(
            window_start_from_usage_data(None, now),
            WindowStart::FallbackRolling
        ));
        assert!(matches!(
            window_start_from_usage_data(Some("{}"), now),
            WindowStart::FallbackRolling
        ));
        assert!(matches!(
            window_start_from_usage_data(Some(r#"{"five_hour":{}}"#), now),
            WindowStart::FallbackRolling
        ));
    }

    #[tokio::test]
    async fn test_ts_sqlite_plain_placeholder() {
        let store = make_store("sqlite").await;
        assert_eq!(store.ts(1), "$1");
        assert_eq!(store.ts(5), "$5");
        assert_eq!(store.ts(10), "$10");
    }

    #[tokio::test]
    async fn test_ts_postgres_casts_to_timestamptz() {
        let store = make_store("postgres").await;
        assert_eq!(store.ts(1), "$1::TIMESTAMPTZ");
        assert_eq!(store.ts(5), "$5::TIMESTAMPTZ");
        assert_eq!(store.ts(10), "$10::TIMESTAMPTZ");
    }

    #[tokio::test]
    async fn test_is_pg() {
        assert!(make_store("postgres").await.is_pg());
        assert!(!make_store("sqlite").await.is_pg());
    }

    #[tokio::test]
    async fn test_now_expr_sqlite() {
        let store = make_store("sqlite").await;
        assert_eq!(store.now_expr(), "strftime('%Y-%m-%dT%H:%M:%SZ','now')");
    }

    #[tokio::test]
    async fn test_now_expr_postgres() {
        let store = make_store("postgres").await;
        assert_eq!(store.now_expr(), "NOW()");
    }

    #[tokio::test]
    async fn test_fmt_time_iso8601() {
        let store = make_store("sqlite").await;
        let t = chrono::NaiveDate::from_ymd_opt(2026, 4, 9)
            .unwrap()
            .and_hms_opt(12, 30, 45)
            .unwrap()
            .and_utc();
        assert_eq!(store.fmt_time(t), "2026-04-09T12:30:45Z");
    }
}
