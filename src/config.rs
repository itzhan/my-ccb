use std::env;
use std::time::Duration;

#[derive(Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub redis: Option<RedisConfig>,
    pub admin: AdminConfig,
    pub log_level: String,
    pub usage_poll_interval: Duration,
    /// 客户端限制级别：off / ua / strict（原始字符串，由 service::client_guard 解析）。
    pub client_restriction: String,
    /// 身份模式：passthrough（默认，单人，原样透传）/ normalize（多人共号，归一化为账号虚拟身份）。
    pub identity_mode: String,
    /// normalize 模式下的路径处理全局默认：simulate（默认，改写真实路径用户名）/ passthrough（真实路径原样透传）。
    pub path_mode: String,
    /// 自动养号运行时配置。
    pub warmup: WarmupRuntime,
}

#[derive(Clone)]
pub struct WarmupRuntime {
    /// claude 可执行文件（默认 "claude"，需在 PATH 中）。
    pub claude_bin: String,
    /// 养号子进程回连的网关地址（默认 http://127.0.0.1:<port>，TLS 部署需显式设置）。
    pub base_url: String,
    /// 同时存活的 claude 子进程上限。
    pub max_processes: usize,
    /// 一轮回答的静默判定秒数（连续这么久无新输出即认为答完）。
    pub idle_secs: u64,
    /// 单轮回答的最大等待秒数（兜底）。
    pub turn_timeout_secs: u64,
}

#[derive(Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
}

#[derive(Clone)]
pub struct DatabaseConfig {
    pub driver: Option<String>,
    pub dsn: Option<String>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub dbname: String,
}

#[derive(Clone)]
pub struct RedisConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
    pub db: i64,
}

#[derive(Clone)]
pub struct AdminConfig {
    pub password: String,
}

impl DatabaseConfig {
    pub fn driver(&self) -> String {
        self.driver.clone().unwrap_or_else(|| "sqlite".into())
    }

    pub fn dsn(&self) -> String {
        if let Some(dsn) = &self.dsn {
            return dsn.clone();
        }
        if self.driver() == "sqlite" {
            return "data/claude-code-gateway.db".into();
        }
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode=disable",
            self.user, self.password, self.host, self.port, self.dbname
        )
    }
}

impl Config {
    pub fn load() -> Self {
        dotenvy::dotenv().ok();

        let server_port: u16 = env::var("SERVER_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5674);
        let warmup = WarmupRuntime {
            claude_bin: env::var("WARMUP_CLAUDE_BIN").unwrap_or_else(|_| "claude".into()),
            base_url: env::var("WARMUP_BASE_URL")
                .unwrap_or_else(|_| format!("http://127.0.0.1:{}", server_port)),
            max_processes: env::var("WARMUP_MAX_PROCESSES")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&v| v > 0)
                .unwrap_or(10),
            idle_secs: env::var("WARMUP_IDLE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&v| v > 0)
                .unwrap_or(4),
            turn_timeout_secs: env::var("WARMUP_TURN_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&v| v > 0)
                .unwrap_or(120),
        };

        let redis = env::var("REDIS_HOST").ok().map(|host| RedisConfig {
            host,
            port: env::var("REDIS_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(6379),
            password: env::var("REDIS_PASSWORD").unwrap_or_default(),
            db: env::var("REDIS_DB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
        });

        Config {
            server: ServerConfig {
                port: server_port,
                host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".into()),
                tls_cert: env::var("TLS_CERT_FILE").ok(),
                tls_key: env::var("TLS_KEY_FILE").ok(),
            },
            database: DatabaseConfig {
                driver: env::var("DATABASE_DRIVER").ok(),
                dsn: env::var("DATABASE_DSN").ok(),
                host: env::var("DATABASE_HOST").unwrap_or_else(|_| "localhost".into()),
                port: env::var("DATABASE_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5432),
                user: env::var("DATABASE_USER").unwrap_or_else(|_| "postgres".into()),
                password: env::var("DATABASE_PASSWORD").unwrap_or_default(),
                dbname: env::var("DATABASE_DBNAME").unwrap_or_else(|_| "claude_code_gateway".into()),
            },
            redis,
            admin: AdminConfig {
                password: env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "admin".into()),
            },
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
            usage_poll_interval: Duration::from_secs(
                env::var("USAGE_POLL_INTERVAL_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(300),
            ),
            client_restriction: env::var("CLIENT_RESTRICTION").unwrap_or_else(|_| "off".into()),
            identity_mode: env::var("IDENTITY_MODE").unwrap_or_else(|_| "passthrough".into()),
            path_mode: env::var("PATH_MODE").unwrap_or_else(|_| "simulate".into()),
            warmup,
        }
    }
}
