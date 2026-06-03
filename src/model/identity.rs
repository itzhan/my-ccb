use rand::Rng;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use super::account::{CanonicalEnvData, CanonicalProcessData, CanonicalPromptEnvData};

fn env_presets() -> Vec<CanonicalEnvData> {
    vec![
        CanonicalEnvData {
            platform: "darwin".into(),
            platform_raw: "darwin".into(),
            arch: "arm64".into(),
            node_version: "v24.3.0".into(),
            terminal: "iTerm2.app".into(),
            package_managers: "npm,pnpm".into(),
            runtimes: "node".into(),
            is_claude_ai_auth: true,
            version: "2.1.156".into(),
            version_base: "2.1.156".into(),
            build_time: "2026-03-20T21:26:18Z".into(),
            deployment_environment: "unknown-darwin".into(),
            vcs: "git".into(),
            ..Default::default()
        },
        CanonicalEnvData {
            platform: "darwin".into(),
            platform_raw: "darwin".into(),
            arch: "x64".into(),
            node_version: "v22.15.0".into(),
            terminal: "Terminal".into(),
            package_managers: "npm,yarn".into(),
            runtimes: "node".into(),
            is_claude_ai_auth: true,
            version: "2.1.156".into(),
            version_base: "2.1.156".into(),
            build_time: "2026-03-20T21:26:18Z".into(),
            deployment_environment: "unknown-darwin".into(),
            vcs: "git".into(),
            ..Default::default()
        },
        CanonicalEnvData {
            platform: "linux".into(),
            platform_raw: "linux".into(),
            arch: "x64".into(),
            node_version: "v24.3.0".into(),
            terminal: "xterm-256color".into(),
            package_managers: "npm,pnpm".into(),
            runtimes: "node".into(),
            is_claude_ai_auth: true,
            version: "2.1.156".into(),
            version_base: "2.1.156".into(),
            build_time: "2026-03-20T21:26:18Z".into(),
            deployment_environment: "unknown-linux".into(),
            vcs: "git".into(),
            ..Default::default()
        },
    ]
}

fn prompt_presets() -> HashMap<&'static str, CanonicalPromptEnvData> {
    let mut m = HashMap::new();
    m.insert(
        "darwin",
        CanonicalPromptEnvData {
            platform: "darwin".into(),
            shell: "zsh".into(),
            os_version: "Darwin 24.4.0".into(),
            working_dir: "/Users/user/projects".into(),
        },
    );
    m.insert(
        "linux",
        CanonicalPromptEnvData {
            platform: "linux".into(),
            shell: "bash".into(),
            os_version: "Linux 6.5.0-generic".into(),
            working_dir: "/home/user/projects".into(),
        },
    );
    m
}

static MEMORY_PRESETS: &[i64] = &[
    17_179_869_184, // 16GB
    34_359_738_368, // 32GB
    68_719_476_736, // 64GB
];

/// 生成随机的 64 字符十六进制字符串。
pub fn generate_device_id() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    hex::encode(bytes)
}

/// 每账号稳定的虚拟身份（多人共号时归一化"是谁"，让一个号始终像同一个人）。
pub struct VirtualIdentity {
    /// 虚拟用户名（home 目录名）
    pub user: String,
    /// 虚拟 git 用户名
    pub git_name: String,
}

/// 真实感的(用户名, 全名)池，按账号稳定取一个。
const IDENTITY_POOL: &[(&str, &str)] = &[
    ("alexc", "Alex Carter"), ("jlee", "Jordan Lee"), ("samp", "Sam Patel"),
    ("cwong", "Chris Wong"), ("tmiller", "Taylor Miller"), ("mgarcia", "Morgan Garcia"),
    ("cbrooks", "Casey Brooks"), ("rkim", "Riley Kim"), ("jnguyen", "Jamie Nguyen"),
    ("drowe", "Drew Rowe"), ("cmorales", "Cameron Morales"), ("qsmith", "Quinn Smith"),
    ("bhayes", "Blake Hayes"), ("areed", "Avery Reed"), ("rfoster", "Reese Foster"),
    ("sjordan", "Skylar Jordan"), ("dlong", "Dakota Long"), ("lcole", "Logan Cole"),
    ("pgray", "Parker Gray"), ("hbennett", "Hayden Bennett"), ("eward", "Emerson Ward"),
    ("fhughes", "Finley Hughes"), ("rsimmons", "Rowan Simmons"), ("sshaw", "Sawyer Shaw"),
];

/// 由账号种子(email 或 id)稳定派生一套虚拟身份。
pub fn virtual_identity(seed: &str) -> VirtualIdentity {
    let s = if seed.is_empty() { "account-default" } else { seed };
    let h = Sha256::digest(s.as_bytes());
    let idx = (u16::from_be_bytes([h[0], h[1]]) as usize) % IDENTITY_POOL.len();
    let (user, name) = IDENTITY_POOL[idx];
    VirtualIdentity {
        user: user.to_string(),
        git_name: name.to_string(),
    }
}

/// 为新账号生成全部规范化身份字段。
pub fn generate_canonical_identity() -> (String, Value, Value, Value) {
    let device_id = generate_device_id();
    let mut rng = rand::thread_rng();

    let presets = env_presets();
    let preset = &presets[rng.gen_range(0..presets.len())];
    let env_json = serde_json::to_value(preset).expect("env preset serialize");

    let prompts = prompt_presets();
    let prompt_env = prompts
        .get(preset.platform.as_str())
        .expect("prompt preset");
    let prompt_json = serde_json::to_value(prompt_env).expect("prompt preset serialize");

    let mem = MEMORY_PRESETS[rng.gen_range(0..MEMORY_PRESETS.len())];
    let process = CanonicalProcessData {
        constrained_memory: mem,
        rss_range: [300_000_000, 500_000_000],
        heap_total_range: [40_000_000, 80_000_000],
        heap_used_range: [100_000_000, 200_000_000],
    };
    let process_json = serde_json::to_value(&process).expect("process serialize");

    (device_id, env_json, prompt_json, process_json)
}
