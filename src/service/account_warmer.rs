//! 自动养号执行引擎。
//!
//! 每个 warmup 令牌在一个伪终端(PTY)中拉起真正的交互式 `claude` 客户端(不加 `-p`),
//! 像真人一样往终端里"打字"提问 + 回车,读取 TUI 输出,用静默期启发式判断一轮答完,
//! 再按配置的间隔/总时长/工作-休息节奏发下一题。养号流量经现有网关管线天然带身份/遥测/指纹。

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rand::Rng;
use tokio::sync::{mpsc, Notify, Semaphore};
use tokio::time::Instant;
use tracing::{info, warn};

use crate::config::WarmupRuntime;
use crate::model::api_token::ApiTokenCategory;
use crate::model::warmup::WarmupStatus;
use crate::store::token_store::TokenStore;
use crate::store::warmup_store::WarmupStore;

/// 内置题库(编译期打包),启动时解析一次。
static QUESTIONS_JSON: &str = include_str!("../../assets/warmup_questions.json");

/// 可取消信号:置位 flag 并唤醒所有等待者。
#[derive(Clone)]
struct Cancel {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Cancel {
    fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }
    fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
    fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    /// 可被取消打断的 sleep;返回 true 表示中途被取消。
    async fn sleep(&self, dur: Duration) -> bool {
        if self.is_cancelled() {
            return true;
        }
        tokio::select! {
            _ = tokio::time::sleep(dur) => self.is_cancelled(),
            _ = self.notify.notified() => true,
        }
    }
}

pub struct AccountWarmerService {
    token_store: Arc<TokenStore>,
    warmup_store: Arc<WarmupStore>,
    questions: Arc<Vec<String>>,
    cfg: WarmupRuntime,
    sem: Arc<Semaphore>,
    /// 活跃任务 id -> 取消句柄。
    tasks: Mutex<HashMap<i64, Cancel>>,
    /// 正在被驱动的令牌 id(防止一个令牌被两个任务同时使用)。
    active_tokens: Arc<Mutex<HashSet<i64>>>,
}

impl AccountWarmerService {
    pub fn new(
        token_store: Arc<TokenStore>,
        warmup_store: Arc<WarmupStore>,
        cfg: WarmupRuntime,
    ) -> Arc<Self> {
        let questions: Vec<String> = serde_json::from_str(QUESTIONS_JSON).unwrap_or_default();
        info!("warmup: loaded {} questions", questions.len());
        Arc::new(Self {
            token_store,
            warmup_store,
            questions: Arc::new(questions),
            sem: Arc::new(Semaphore::new(cfg.max_processes)),
            cfg,
            tasks: Mutex::new(HashMap::new()),
            active_tokens: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    pub fn questions_count(&self) -> usize {
        self.questions.len()
    }

    /// 完整题库(供独立本地运行器拉取)。
    pub fn questions_arc(&self) -> Arc<Vec<String>> {
        self.questions.clone()
    }

    /// supervisor:进程重启后恢复 DB 中标记为 running 的任务。
    pub async fn run(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let running = match self.warmup_store.list_by_status("running").await {
                Ok(v) => v,
                Err(_) => continue,
            };
            for task in running {
                let already = self.tasks.lock().unwrap().contains_key(&task.id);
                if !already {
                    self.clone().spawn_task_runner(task.id);
                }
            }
        }
    }

    /// 启动一个任务:置 running + 起止时间,然后拉起 runner。
    pub async fn start_task(self: &Arc<Self>, id: i64) -> Result<(), crate::error::AppError> {
        let task = self.warmup_store.get_by_id(id).await?;
        if self.tasks.lock().unwrap().contains_key(&id) {
            return Ok(()); // 已在运行
        }
        let now = chrono::Utc::now();
        let ends = now + chrono::Duration::seconds(task.total_duration_secs.max(1));
        self.warmup_store
            .update_runtime(id, "running", Some(now), Some(ends), "")
            .await?;
        self.clone().spawn_task_runner(id);
        Ok(())
    }

    /// 停止一个任务:取消 worker 并置 stopped。
    pub async fn stop_task(&self, id: i64) -> Result<(), crate::error::AppError> {
        if let Some(c) = self.tasks.lock().unwrap().get(&id).cloned() {
            c.cancel();
        }
        self.warmup_store.set_status(id, "stopped").await.ok();
        Ok(())
    }

    fn spawn_task_runner(self: Arc<Self>, id: i64) {
        let cancel = Cancel::new();
        {
            let mut g = self.tasks.lock().unwrap();
            if g.contains_key(&id) {
                return;
            }
            g.insert(id, cancel.clone());
        }
        tokio::spawn(async move {
            self.run_task(id, cancel).await;
        });
    }

    async fn run_task(self: Arc<Self>, id: i64, cancel: Cancel) {
        let task = match self.warmup_store.get_by_id(id).await {
            Ok(t) => t,
            Err(_) => {
                self.tasks.lock().unwrap().remove(&id);
                return;
            }
        };

        // 计算总截止时间(优先用已记录的 started_at,支持重启续跑)。
        let started = task.started_at.unwrap_or_else(chrono::Utc::now);
        let deadline = started + chrono::Duration::seconds(task.total_duration_secs.max(1));
        let now = chrono::Utc::now();
        let remaining = (deadline - now).num_seconds();
        if remaining <= 0 {
            self.warmup_store.set_status(id, "completed").await.ok();
            self.tasks.lock().unwrap().remove(&id);
            return;
        }
        let deadline_instant = Instant::now() + Duration::from_secs(remaining as u64);

        // 解析并加载 warmup 令牌。
        let mut tokens = Vec::new();
        for tid in task.token_id_list() {
            match self.token_store.get_by_id(tid).await {
                Ok(t) if t.category == ApiTokenCategory::Warmup => tokens.push(t),
                Ok(_) => warn!("warmup task {}: token {} 不是养号分类,跳过", id, tid),
                Err(_) => warn!("warmup task {}: token {} 不存在,跳过", id, tid),
            }
        }
        if tokens.is_empty() {
            self.warmup_store
                .update_runtime(id, "error", task.started_at, None, "没有可用的养号令牌")
                .await
                .ok();
            self.tasks.lock().unwrap().remove(&id);
            return;
        }

        info!(
            "warmup task {} 启动:{} 个令牌,间隔 {}s,总时长 {}s,工作/休息 {}s/{}s",
            id,
            tokens.len(),
            task.msg_interval_secs,
            task.total_duration_secs,
            task.work_duration_secs,
            task.rest_duration_secs
        );

        // 每个令牌一个 session worker。
        let mut handles = Vec::new();
        for token in tokens {
            let svc = self.clone();
            let cancel = cancel.clone();
            let model = task.model.clone();
            let interval = task.msg_interval_secs.max(1) as u64;
            let work = task.work_duration_secs.max(0) as u64;
            let rest = task.rest_duration_secs.max(0) as u64;
            let jitter = task.jitter_pct.clamp(0, 100) as u64;
            let max_turns = task.max_turns.max(0) as u64;
            handles.push(tokio::spawn(async move {
                svc.session_worker(
                    id,
                    token.id,
                    token.token,
                    model,
                    interval,
                    work,
                    rest,
                    jitter,
                    max_turns,
                    deadline_instant,
                    cancel,
                )
                .await;
            }));
        }
        for h in handles {
            let _ = h.await;
        }

        // 收尾:被取消 => 保持 stopped;worker 已置 error => 保留 error;否则 completed。
        if cancel.is_cancelled() {
            self.warmup_store.set_status(id, "stopped").await.ok();
        } else {
            let is_err = self
                .warmup_store
                .get_by_id(id)
                .await
                .map(|t| t.status == WarmupStatus::Error)
                .unwrap_or(false);
            if !is_err {
                self.warmup_store.set_status(id, "completed").await.ok();
            }
        }
        self.tasks.lock().unwrap().remove(&id);
        info!("warmup task {} 结束", id);
    }

    #[allow(clippy::too_many_arguments)]
    async fn session_worker(
        self: Arc<Self>,
        task_id: i64,
        token_id: i64,
        token: String,
        model: String,
        interval_secs: u64,
        work_secs: u64,
        rest_secs: u64,
        jitter_pct: u64,
        max_turns: u64,
        deadline: Instant,
        cancel: Cancel,
    ) {
        // 同一令牌只允许一个 worker。
        {
            let mut g = self.active_tokens.lock().unwrap();
            if g.contains(&token_id) {
                warn!("warmup: 令牌 {} 已在养号中,跳过", token_id);
                return;
            }
            g.insert(token_id);
        }
        let _token_guard = scopeguard::guard(self.active_tokens.clone(), move |s| {
            s.lock().unwrap().remove(&token_id);
        });

        // 获取并发名额(可取消)。
        let _permit = tokio::select! {
            p = self.sem.clone().acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => return,
            },
            _ = cancel.notify.notified() => return,
        };
        if cancel.is_cancelled() {
            return;
        }

        // 独立工作目录 + 隔离 HOME(预置 .claude.json 跳过首启引导)。
        let work_dir = std::env::temp_dir().join(format!("ccg-warmup-{}-{}", task_id, token_id));
        std::fs::create_dir_all(&work_dir).ok();
        seed_claude_config(&work_dir, &token);
        let _dir_guard = scopeguard::guard(work_dir.clone(), |d| {
            std::fs::remove_dir_all(&d).ok();
        });

        // 拉起 PTY 中的交互式 claude。
        let pty = match self.spawn_claude(&token, &model, &work_dir) {
            Ok(p) => p,
            Err(e) => {
                warn!("warmup: 令牌 {} 启动 claude 失败: {}", token_id, e);
                self.warmup_store
                    .update_runtime(task_id, "error", None, None, &format!("启动 claude 失败: {}", e))
                    .await
                    .ok();
                return;
            }
        };
        let PtyHandle {
            _master,
            writer,
            mut rx,
            child,
        } = pty;
        let writer = Arc::new(Mutex::new(writer));
        let child = Arc::new(Mutex::new(child));
        // 收尾时杀掉子进程。
        let child_kill = child.clone();
        let _child_guard = scopeguard::guard((), move |_| {
            if let Ok(mut c) = child_kill.lock() {
                c.kill().ok();
            }
        });

        // 等待首启稳定:补发一次回车消除可能的引导弹窗,再等输出静默。
        let _ = cancel.sleep(Duration::from_millis(1500)).await;
        pty_write(&writer, b"\r");
        drain_until_idle(
            &mut rx,
            Duration::from_secs(self.cfg.idle_secs),
            Duration::from_secs(20),
            &cancel,
        )
        .await;

        let idle = Duration::from_secs(self.cfg.idle_secs);
        let turn_timeout = Duration::from_secs(self.cfg.turn_timeout_secs);
        let mut worked = Duration::ZERO;
        let mut turns_in_session: u64 = 0;

        while !cancel.is_cancelled() && Instant::now() < deadline {
            // 抽题并提交:先发文本,停顿,再单独发回车
            // (TUI 开了括号粘贴模式,文本和回车一起发会被当成粘贴内容,回车不触发提交)。
            let q = self.random_question();
            pty_write(&writer, q.as_bytes());
            tokio::time::sleep(Duration::from_millis(250)).await;
            pty_write(&writer, b"\r");

            // 等这一轮答完(静默判定 + 最大等待兜底)。
            let outcome = wait_turn(&mut rx, idle, turn_timeout, &cancel, deadline).await;
            match outcome {
                TurnOutcome::Cancelled | TurnOutcome::Eof => break,
                TurnOutcome::Deadline => break,
                TurnOutcome::Done | TurnOutcome::Timeout => {
                    self.warmup_store.bump_messages(task_id, 1).await.ok();
                    turns_in_session += 1;
                    // 达到最大轮数:发 /clear 开新对话,清空上下文(避免上下文越滚越大、消费飙升)。
                    if max_turns > 0 && turns_in_session >= max_turns {
                        pty_write(&writer, b"/clear");
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        pty_write(&writer, b"\r");
                        drain_until_idle(&mut rx, idle, Duration::from_secs(15), &cancel).await;
                        info!(
                            "warmup task {} 令牌 {}: 已满 {} 轮,/clear 开新对话",
                            task_id, token_id, max_turns
                        );
                        turns_in_session = 0;
                    }
                }
            }

            if Instant::now() >= deadline {
                break;
            }

            // 计算下一条的间隔(带抖动)。
            let wait = jittered(interval_secs, jitter_pct);
            if cancel.sleep(wait).await {
                break;
            }
            worked += wait;

            // 大间隔:工作满 work_secs 后长休 rest_secs。
            if work_secs > 0 && rest_secs > 0 && worked >= Duration::from_secs(work_secs) {
                info!("warmup task {} 令牌 {}: 进入休息 {}s", task_id, token_id, rest_secs);
                if cancel.sleep(Duration::from_secs(rest_secs)).await {
                    break;
                }
                worked = Duration::ZERO;
            }
        }

        // 优雅退出 claude。
        pty_write(&writer, b"/exit");
        tokio::time::sleep(Duration::from_millis(150)).await;
        pty_write(&writer, b"\r");
        let _ = cancel.sleep(Duration::from_millis(300)).await;
    }

    fn random_question(&self) -> String {
        if self.questions.is_empty() {
            return "Explain how a hash table works.".into();
        }
        let i = rand::thread_rng().gen_range(0..self.questions.len());
        self.questions[i].clone()
    }

    /// 在 PTY 中拉起交互式 claude,返回读写句柄。
    fn spawn_claude(
        &self,
        token: &str,
        model: &str,
        work_dir: &std::path::Path,
    ) -> Result<PtyHandle, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(&self.cfg.claude_bin);
        cmd.arg("--bare");
        cmd.arg("--permission-mode");
        cmd.arg("bypassPermissions");
        if !model.is_empty() {
            cmd.arg("--model");
            cmd.arg(model);
        }
        // 养号只要纯文字问答:禁工具,避免模型进入 Bash/Read 等 agentic 多步循环
        // (否则一个问题会炸出十几次 API 调用,某些账号被狂调、消费飙升)。
        cmd.arg("--append-system-prompt");
        cmd.arg(
            "You are warming up an account by chatting. Answer each question directly in concise \
             plain text only. Never use any tools, never read or write files, never run commands, \
             never browse the web. 仅用简洁纯文字直接回答,绝不使用任何工具、不读写文件、不执行命令、不联网。",
        );
        cmd.arg("--disallowedTools");
        cmd.arg("Bash Edit Write Read Glob Grep WebFetch WebSearch NotebookEdit MultiEdit Task TodoWrite");
        cmd.cwd(work_dir);
        cmd.env("ANTHROPIC_BASE_URL", &self.cfg.base_url);
        cmd.env("ANTHROPIC_API_KEY", token);
        cmd.env("HOME", work_dir);
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
        }
        cmd.env("TERM", "xterm-256color");
        // 关闭自动更新干扰。
        cmd.env("DISABLE_AUTOUPDATER", "1");
        // 容器内以 root 运行时,claude 默认拒绝 bypassPermissions;声明 sandbox 后放行。
        cmd.env("IS_SANDBOX", "1");

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(PtyHandle {
            _master: pair.master,
            writer,
            rx,
            child,
        })
    }
}

struct PtyHandle {
    _master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

enum TurnOutcome {
    Done,      // 输出后静默,视为答完
    Timeout,   // 达到单轮最大等待
    Cancelled, // 被取消
    Eof,       // 子进程退出
    Deadline,  // 到总截止时间
}

/// 等待一轮回答结束:收到输出后静默 idle 即判定答完;turn_timeout 兜底。
async fn wait_turn(
    rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    idle: Duration,
    turn_timeout: Duration,
    cancel: &Cancel,
    deadline: Instant,
) -> TurnOutcome {
    let start = Instant::now();
    let mut got_output = false;
    loop {
        if cancel.is_cancelled() {
            return TurnOutcome::Cancelled;
        }
        if Instant::now() >= deadline {
            return TurnOutcome::Deadline;
        }
        if start.elapsed() >= turn_timeout {
            return TurnOutcome::Timeout;
        }
        match tokio::time::timeout(idle, rx.recv()).await {
            Ok(Some(_)) => got_output = true,
            Ok(None) => return TurnOutcome::Eof,
            Err(_) => {
                if got_output {
                    return TurnOutcome::Done;
                }
            }
        }
    }
}

/// 启动后排空输出直到静默(或到最大等待)。
async fn drain_until_idle(
    rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    idle: Duration,
    max: Duration,
    cancel: &Cancel,
) {
    let start = Instant::now();
    loop {
        if cancel.is_cancelled() || start.elapsed() >= max {
            return;
        }
        match tokio::time::timeout(idle, rx.recv()).await {
            Ok(Some(_)) => continue,
            Ok(None) => return,
            Err(_) => return, // 静默
        }
    }
}

/// 向 PTY 写入原始字节(不自动追加回车)。
fn pty_write(writer: &Arc<Mutex<Box<dyn Write + Send>>>, bytes: &[u8]) {
    if let Ok(mut w) = writer.lock() {
        let _ = w.write_all(bytes);
        let _ = w.flush();
    }
}

/// 给间隔加 ±jitter_pct 的抖动。
fn jittered(secs: u64, jitter_pct: u64) -> Duration {
    if jitter_pct == 0 || secs == 0 {
        return Duration::from_secs(secs);
    }
    let span = (secs * jitter_pct) / 100;
    if span == 0 {
        return Duration::from_secs(secs);
    }
    let delta = rand::thread_rng().gen_range(0..=(span * 2)) as i64 - span as i64;
    let v = (secs as i64 + delta).max(1) as u64;
    Duration::from_secs(v)
}

/// 预置一份 .claude.json,跳过首启信任/主题/onboarding 弹窗,并预批准本 token
/// 对应的 ANTHROPIC_API_KEY(否则交互模式会弹「是否使用此 API key」卡住)。
fn seed_claude_config(home: &std::path::Path, token: &str) {
    let mut projects = serde_json::Map::new();
    projects.insert(
        home.to_string_lossy().to_string(),
        serde_json::json!({
            "hasTrustDialogAccepted": true,
            "hasCompletedProjectOnboarding": true,
            "allowedTools": []
        }),
    );
    // claude 用 key 的末 20 位标识已批准的自定义 API key。
    let approved = if token.len() > 20 {
        token[token.len() - 20..].to_string()
    } else {
        token.to_string()
    };
    let cfg = serde_json::json!({
        "hasCompletedOnboarding": true,
        "bypassPermissionsModeAccepted": true,
        "hasTrustDialogAccepted": true,
        "theme": "dark",
        "autoUpdates": false,
        "customApiKeyResponses": { "approved": [approved], "rejected": [] },
        "projects": projects,
    });
    std::fs::write(
        home.join(".claude.json"),
        serde_json::to_string_pretty(&cfg).unwrap_or_default(),
    )
    .ok();
}
