use chrono::{DateTime, Utc};
use sqlx::AnyPool;

use crate::error::AppError;
use crate::model::warmup::WarmupTask;

pub struct WarmupStore {
    pool: AnyPool,
    driver: String,
}

const COLS: &str = "id, name, token_ids, msg_interval_secs, total_duration_secs, work_duration_secs, \
    rest_duration_secs, jitter_pct, max_turns, model, status, error, messages_sent, started_at, ends_at, \
    last_message_at, created_at, updated_at";

impl WarmupStore {
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

    fn fmt_time(&self, t: DateTime<Utc>) -> String {
        t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    fn parse_time(s: Option<String>) -> Option<DateTime<Utc>> {
        s.and_then(|s| s.parse().ok())
    }

    fn row_to_task(&self, row: &sqlx::any::AnyRow) -> WarmupTask {
        use sqlx::Row;
        WarmupTask {
            id: row.get::<i64, _>("id"),
            name: row.get::<String, _>("name"),
            token_ids: row.get::<String, _>("token_ids"),
            msg_interval_secs: row.try_get::<i64, _>("msg_interval_secs").unwrap_or(60),
            total_duration_secs: row.try_get::<i64, _>("total_duration_secs").unwrap_or(3600),
            work_duration_secs: row.try_get::<i64, _>("work_duration_secs").unwrap_or(0),
            rest_duration_secs: row.try_get::<i64, _>("rest_duration_secs").unwrap_or(0),
            jitter_pct: row.try_get::<i64, _>("jitter_pct").unwrap_or(20),
            max_turns: row.try_get::<i64, _>("max_turns").unwrap_or(0),
            model: row.get::<String, _>("model"),
            status: row.get::<String, _>("status").into(),
            error: row.get::<String, _>("error"),
            messages_sent: row.try_get::<i64, _>("messages_sent").unwrap_or(0),
            started_at: Self::parse_time(row.try_get::<Option<String>, _>("started_at").ok().flatten()),
            ends_at: Self::parse_time(row.try_get::<Option<String>, _>("ends_at").ok().flatten()),
            last_message_at: Self::parse_time(
                row.try_get::<Option<String>, _>("last_message_at").ok().flatten(),
            ),
            created_at: row.get::<String, _>("created_at").parse().unwrap_or_else(|_| Utc::now()),
            updated_at: row.get::<String, _>("updated_at").parse().unwrap_or_else(|_| Utc::now()),
        }
    }

    pub async fn create(&self, t: &mut WarmupTask) -> Result<(), AppError> {
        let q = format!(
            "INSERT INTO warmup_tasks (name, token_ids, msg_interval_secs, total_duration_secs, \
             work_duration_secs, rest_duration_secs, jitter_pct, max_turns, model, status, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,{now},{now})",
            now = self.now_expr()
        );
        let result = sqlx::query(&q)
            .bind(&t.name)
            .bind(&t.token_ids)
            .bind(t.msg_interval_secs)
            .bind(t.total_duration_secs)
            .bind(t.work_duration_secs)
            .bind(t.rest_duration_secs)
            .bind(t.jitter_pct)
            .bind(t.max_turns)
            .bind(&t.model)
            .bind(t.status.to_string())
            .execute(&self.pool)
            .await?;
        t.id = result.last_insert_id().unwrap_or(0) as i64;
        Ok(())
    }

    /// 更新任务可编辑字段（名称/令牌/节奏参数）。
    pub async fn update(&self, t: &WarmupTask) -> Result<(), AppError> {
        let q = format!(
            "UPDATE warmup_tasks SET name=$1, token_ids=$2, msg_interval_secs=$3, total_duration_secs=$4, \
             work_duration_secs=$5, rest_duration_secs=$6, jitter_pct=$7, max_turns=$8, model=$9, updated_at={} WHERE id=$10",
            self.now_expr()
        );
        sqlx::query(&q)
            .bind(&t.name)
            .bind(&t.token_ids)
            .bind(t.msg_interval_secs)
            .bind(t.total_duration_secs)
            .bind(t.work_duration_secs)
            .bind(t.rest_duration_secs)
            .bind(t.jitter_pct)
            .bind(t.max_turns)
            .bind(&t.model)
            .bind(t.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 更新运行状态 + 起止时间 + 错误信息。
    pub async fn update_runtime(
        &self,
        id: i64,
        status: &str,
        started_at: Option<DateTime<Utc>>,
        ends_at: Option<DateTime<Utc>>,
        error: &str,
    ) -> Result<(), AppError> {
        let q = format!(
            "UPDATE warmup_tasks SET status=$1, started_at=$2, ends_at=$3, error=$4, updated_at={} WHERE id=$5",
            self.now_expr()
        );
        sqlx::query(&q)
            .bind(status)
            .bind(started_at.map(|e| self.fmt_time(e)))
            .bind(ends_at.map(|e| self.fmt_time(e)))
            .bind(error)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 仅更新状态。
    pub async fn set_status(&self, id: i64, status: &str) -> Result<(), AppError> {
        let q = format!(
            "UPDATE warmup_tasks SET status=$1, updated_at={} WHERE id=$2",
            self.now_expr()
        );
        sqlx::query(&q).bind(status).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// 累加已发消息数并更新 last_message_at（养号 worker 每发一条调用）。
    pub async fn bump_messages(&self, id: i64, delta: i64) -> Result<(), AppError> {
        let q = format!(
            "UPDATE warmup_tasks SET messages_sent = messages_sent + $1, last_message_at={now}, updated_at={now} WHERE id=$2",
            now = self.now_expr()
        );
        sqlx::query(&q).bind(delta).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<(), AppError> {
        sqlx::query("DELETE FROM warmup_tasks WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_by_id(&self, id: i64) -> Result<WarmupTask, AppError> {
        let q = format!("SELECT {} FROM warmup_tasks WHERE id=$1", COLS);
        let row = sqlx::query(&q)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(AppError::NotFound)?;
        Ok(self.row_to_task(&row))
    }

    pub async fn list(&self) -> Result<Vec<WarmupTask>, AppError> {
        let q = format!("SELECT {} FROM warmup_tasks ORDER BY created_at DESC", COLS);
        let rows = sqlx::query(&q).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|r| self.row_to_task(r)).collect())
    }

    /// 列出指定状态的任务（supervisor 扫描 running 用）。
    pub async fn list_by_status(&self, status: &str) -> Result<Vec<WarmupTask>, AppError> {
        let q = format!("SELECT {} FROM warmup_tasks WHERE status=$1", COLS);
        let rows = sqlx::query(&q).bind(status).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|r| self.row_to_task(r)).collect())
    }
}
