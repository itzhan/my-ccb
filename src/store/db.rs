use sqlx::AnyPool;
use std::path::Path;

pub async fn init_db(driver: &str, dsn: &str) -> Result<AnyPool, sqlx::Error> {
    if driver == "sqlite" {
        if let Some(parent) = Path::new(dsn).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let pool = AnyPool::connect(&format!("sqlite:{}?mode=rwc", dsn)).await?;
        sqlx::query("PRAGMA journal_mode=WAL").execute(&pool).await.ok();
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await.ok();
        Ok(pool)
    } else {
        let pool = AnyPool::connect(dsn).await?;
        Ok(pool)
    }
}

pub async fn migrate(pool: &AnyPool, driver: &str) -> Result<(), sqlx::Error> {
    let schema = if driver == "sqlite" { SQLITE_SCHEMA } else { PG_SCHEMA };
    for stmt in schema.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        sqlx::query(stmt).execute(pool).await?;
    }
    // 增量迁移
    sqlx::query("ALTER TABLE accounts ADD COLUMN billing_mode TEXT NOT NULL DEFAULT 'strip'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN usage_data TEXT NOT NULL DEFAULT '{}'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN usage_fetched_at TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN auth_type TEXT NOT NULL DEFAULT 'setup_token'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN access_token TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN refresh_token TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN oauth_expires_at TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN oauth_refreshed_at TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN auth_error TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN account_uuid TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN organization_uuid TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN subscription_type TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN disable_reason TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN auto_telemetry INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN telemetry_count INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN rpm_limit INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN identity_mode TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN virtual_user TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN virtual_git_name TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN identity_captured_at TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN recapture_days INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN max_sessions INTEGER NOT NULL DEFAULT 3")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN allowed_client_types TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    // 5h 滚动窗口的消费上限(USD);0 表示不限制
    sqlx::query(
        "ALTER TABLE accounts ADD COLUMN window_5h_cost_cap_usd REAL NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await
    .ok();
    // normalize 模式下路径处理:''=回退全局默认 / simulate / passthrough
    sqlx::query("ALTER TABLE accounts ADD COLUMN path_mode TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    // normalize 模式下当前对上游呈现的 session_id(每 15-20min 吸取轮换)+ 吸取时间,展示用
    sqlx::query("ALTER TABLE accounts ADD COLUMN captured_session_id TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE accounts ADD COLUMN captured_session_at TEXT")
        .execute(pool)
        .await
        .ok();
    // session_id 归一化轮换开关(账号级):''/off=关 / rotate=开
    sqlx::query("ALTER TABLE accounts ADD COLUMN session_mode TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();

    // api_tokens 表
    let token_schema = if driver == "sqlite" { SQLITE_TOKENS_SCHEMA } else { PG_TOKENS_SCHEMA };
    for stmt in token_schema.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        sqlx::query(stmt).execute(pool).await?;
    }
    // settings 键值表（运行时可改的全局设置）
    sqlx::query("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL DEFAULT '')")
        .execute(pool)
        .await
        .ok();

    // api_tokens 增量迁移
    sqlx::query("ALTER TABLE api_tokens ADD COLUMN concurrency INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE api_tokens ADD COLUMN expires_at TEXT")
        .execute(pool)
        .await
        .ok();

    // 用量记录表（调用明细 usage_logs + 每日汇总 usage_daily）
    let usage_schema = if driver == "sqlite" { SQLITE_USAGE_SCHEMA } else { PG_USAGE_SCHEMA };
    for stmt in usage_schema.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        sqlx::query(stmt).execute(pool).await?;
    }
    // usage_logs 增量迁移：错误文本 + 详细诊断列
    for col in [
        "error",
        "client_ip",
        "user_agent",
        "path",
        "session_id",
        "user_id",
        "proxy",
        "req_headers",
        "resp_headers",
    ] {
        sqlx::query(&format!(
            "ALTER TABLE usage_logs ADD COLUMN {} TEXT NOT NULL DEFAULT ''",
            col
        ))
        .execute(pool)
        .await
        .ok();
    }
    Ok(())
}

/// 读取一个全局设置项。
pub async fn get_setting(pool: &AnyPool, key: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key=$1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

/// 写入一个全局设置项（upsert）。
pub async fn set_setting(pool: &AnyPool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ($1,$2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

const SQLITE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL DEFAULT '',
    email           TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active',
    token           TEXT NOT NULL,
    auth_type       TEXT NOT NULL DEFAULT 'setup_token',
    access_token    TEXT NOT NULL DEFAULT '',
    refresh_token   TEXT NOT NULL DEFAULT '',
    oauth_expires_at    TEXT,
    oauth_refreshed_at  TEXT,
    auth_error      TEXT NOT NULL DEFAULT '',
    proxy_url       TEXT NOT NULL DEFAULT '',
    device_id       TEXT NOT NULL,
    canonical_env   TEXT NOT NULL DEFAULT '{}',
    canonical_prompt_env TEXT NOT NULL DEFAULT '{}',
    canonical_process    TEXT NOT NULL DEFAULT '{}',
    billing_mode    TEXT NOT NULL DEFAULT 'strip',
    concurrency     INTEGER NOT NULL DEFAULT 3,
    priority        INTEGER NOT NULL DEFAULT 50,
    rate_limited_at      TEXT,
    rate_limit_reset_at  TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

"#;

const PG_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id              BIGSERIAL PRIMARY KEY,
    name            TEXT NOT NULL DEFAULT '',
    email           TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active',
    token           TEXT NOT NULL,
    auth_type       TEXT NOT NULL DEFAULT 'setup_token',
    access_token    TEXT NOT NULL DEFAULT '',
    refresh_token   TEXT NOT NULL DEFAULT '',
    oauth_expires_at    TIMESTAMPTZ,
    oauth_refreshed_at  TIMESTAMPTZ,
    auth_error      TEXT NOT NULL DEFAULT '',
    proxy_url       TEXT NOT NULL DEFAULT '',
    device_id       TEXT NOT NULL,
    canonical_env   JSONB NOT NULL DEFAULT '{}',
    canonical_prompt_env JSONB NOT NULL DEFAULT '{}',
    canonical_process    JSONB NOT NULL DEFAULT '{}',
    billing_mode    TEXT NOT NULL DEFAULT 'strip',
    concurrency     INT NOT NULL DEFAULT 3,
    priority        INT NOT NULL DEFAULT 50,
    rate_limited_at      TIMESTAMPTZ,
    rate_limit_reset_at  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

"#;

const SQLITE_TOKENS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS api_tokens (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT NOT NULL DEFAULT '',
    token               TEXT NOT NULL UNIQUE,
    allowed_accounts    TEXT NOT NULL DEFAULT '',
    blocked_accounts    TEXT NOT NULL DEFAULT '',
    status              TEXT NOT NULL DEFAULT 'active',
    concurrency         INTEGER NOT NULL DEFAULT 0,
    expires_at          TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
)
"#;

const PG_TOKENS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS api_tokens (
    id                  BIGSERIAL PRIMARY KEY,
    name                TEXT NOT NULL DEFAULT '',
    token               TEXT NOT NULL UNIQUE,
    allowed_accounts    TEXT NOT NULL DEFAULT '',
    blocked_accounts    TEXT NOT NULL DEFAULT '',
    status              TEXT NOT NULL DEFAULT 'active',
    concurrency         INTEGER NOT NULL DEFAULT 0,
    expires_at          TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
)
"#;

const SQLITE_USAGE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS usage_logs (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    token_id                 INTEGER NOT NULL DEFAULT 0,
    account_id               INTEGER NOT NULL DEFAULT 0,
    request_id               TEXT NOT NULL DEFAULT '',
    model                    TEXT NOT NULL DEFAULT '',
    input_tokens             INTEGER NOT NULL DEFAULT 0,
    output_tokens            INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens    INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens        INTEGER NOT NULL DEFAULT 0,
    cache_creation_5m_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_1h_tokens INTEGER NOT NULL DEFAULT 0,
    stream                   INTEGER NOT NULL DEFAULT 0,
    status_code              INTEGER NOT NULL DEFAULT 0,
    duration_ms              INTEGER NOT NULL DEFAULT 0,
    error                    TEXT NOT NULL DEFAULT '',
    client_ip                TEXT NOT NULL DEFAULT '',
    user_agent               TEXT NOT NULL DEFAULT '',
    path                     TEXT NOT NULL DEFAULT '',
    session_id               TEXT NOT NULL DEFAULT '',
    user_id                  TEXT NOT NULL DEFAULT '',
    proxy                    TEXT NOT NULL DEFAULT '',
    req_headers              TEXT NOT NULL DEFAULT '',
    resp_headers             TEXT NOT NULL DEFAULT '',
    created_at               TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_usage_logs_token_created ON usage_logs(token_id, created_at);
CREATE INDEX IF NOT EXISTS idx_usage_logs_account_created ON usage_logs(account_id, created_at);
CREATE INDEX IF NOT EXISTS idx_usage_logs_created ON usage_logs(created_at);
CREATE TABLE IF NOT EXISTS usage_daily (
    day                      TEXT NOT NULL,
    token_id                 INTEGER NOT NULL DEFAULT 0,
    account_id               INTEGER NOT NULL DEFAULT 0,
    model                    TEXT NOT NULL DEFAULT '',
    input_tokens             INTEGER NOT NULL DEFAULT 0,
    output_tokens            INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens    INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens        INTEGER NOT NULL DEFAULT 0,
    req_count                INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, token_id, account_id, model)
);
"#;

const PG_USAGE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS usage_logs (
    id                       BIGSERIAL PRIMARY KEY,
    token_id                 BIGINT NOT NULL DEFAULT 0,
    account_id               BIGINT NOT NULL DEFAULT 0,
    request_id               TEXT NOT NULL DEFAULT '',
    model                    TEXT NOT NULL DEFAULT '',
    input_tokens             BIGINT NOT NULL DEFAULT 0,
    output_tokens            BIGINT NOT NULL DEFAULT 0,
    cache_creation_tokens    BIGINT NOT NULL DEFAULT 0,
    cache_read_tokens        BIGINT NOT NULL DEFAULT 0,
    cache_creation_5m_tokens BIGINT NOT NULL DEFAULT 0,
    cache_creation_1h_tokens BIGINT NOT NULL DEFAULT 0,
    stream                   BIGINT NOT NULL DEFAULT 0,
    status_code              BIGINT NOT NULL DEFAULT 0,
    duration_ms              BIGINT NOT NULL DEFAULT 0,
    error                    TEXT NOT NULL DEFAULT '',
    client_ip                TEXT NOT NULL DEFAULT '',
    user_agent               TEXT NOT NULL DEFAULT '',
    path                     TEXT NOT NULL DEFAULT '',
    session_id               TEXT NOT NULL DEFAULT '',
    user_id                  TEXT NOT NULL DEFAULT '',
    proxy                    TEXT NOT NULL DEFAULT '',
    req_headers              TEXT NOT NULL DEFAULT '',
    resp_headers             TEXT NOT NULL DEFAULT '',
    created_at               TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_usage_logs_token_created ON usage_logs(token_id, created_at);
CREATE INDEX IF NOT EXISTS idx_usage_logs_account_created ON usage_logs(account_id, created_at);
CREATE INDEX IF NOT EXISTS idx_usage_logs_created ON usage_logs(created_at);
CREATE TABLE IF NOT EXISTS usage_daily (
    day                      TEXT NOT NULL,
    token_id                 BIGINT NOT NULL DEFAULT 0,
    account_id               BIGINT NOT NULL DEFAULT 0,
    model                    TEXT NOT NULL DEFAULT '',
    input_tokens             BIGINT NOT NULL DEFAULT 0,
    output_tokens            BIGINT NOT NULL DEFAULT 0,
    cache_creation_tokens    BIGINT NOT NULL DEFAULT 0,
    cache_read_tokens        BIGINT NOT NULL DEFAULT 0,
    req_count                BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, token_id, account_id, model)
);
"#;
