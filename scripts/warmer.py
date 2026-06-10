#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
独立本地养号运行器（脱离号池，直接本地跑）。

只需要一个网关 URL + 管理员 key：脚本会从网关拉取「养号(warmup)分类」的令牌和题库，
然后在本机为每个令牌拉起一个真正的交互式 `claude` 客户端(伪终端 PTY，不是 `claude -p`)，
像真人一样持续提问，节奏由命令行参数控制。

依赖:  pip install pexpect requests
前提:  本机已安装 `claude` CLI(在 PATH 中)。

示例:
  python warmer.py --url https://gw.example.com --admin-key <ADMIN_PASSWORD> \\
      --interval 60 --total 60m --work 20m --rest 5m --model opus

  # 只看会用哪些养号令牌,不实际运行:
  python warmer.py --url ... --admin-key ... --list
"""

import argparse
import json
import os
import random
import re
import sys
import tempfile
import threading
import time

# 第三方依赖延迟导入,让 --help / --list 在未装 pexpect 时也能用。
requests = None
pexpect = None


def _lazy_imports(need_pexpect: bool):
    global requests, pexpect
    try:
        import requests as _requests
        requests = _requests
    except ImportError:
        sys.exit("缺少依赖: pip install requests")
    if need_pexpect:
        try:
            import pexpect as _pexpect
            pexpect = _pexpect
        except ImportError:
            sys.exit("缺少依赖: pip install pexpect")


# ------------------------- 工具 -------------------------

def parse_duration(s: str) -> int:
    """'90s' / '20m' / '2h' / '3600' -> 秒"""
    s = str(s).strip().lower()
    m = re.fullmatch(r"(\d+)\s*([smh]?)", s)
    if not m:
        raise argparse.ArgumentTypeError(f"无法解析时长: {s}")
    n = int(m.group(1))
    return n * {"": 1, "s": 1, "m": 60, "h": 3600}[m.group(2)]


def jittered(secs: int, jitter_pct: int) -> float:
    if jitter_pct <= 0 or secs <= 0:
        return float(secs)
    span = secs * jitter_pct / 100.0
    return max(1.0, secs + random.uniform(-span, span))


def log(token_label: str, msg: str):
    print(f"[{time.strftime('%H:%M:%S')}] [{token_label}] {msg}", flush=True)


# ------------------------- 网关交互 -------------------------

class Gateway:
    def __init__(self, base_url: str, admin_key: str, verify: bool):
        self.base = base_url.rstrip("/")
        self.headers = {"Authorization": f"Bearer {admin_key}"}
        self.verify = verify

    def _get(self, path: str):
        r = requests.get(self.base + path, headers=self.headers, verify=self.verify, timeout=30)
        if r.status_code == 401:
            sys.exit("管理员 key 无效(401)。--admin-key 应为网关的 ADMIN_PASSWORD。")
        r.raise_for_status()
        return r.json()

    def warmup_tokens(self):
        return self._get("/admin/warmup/tokens").get("data", [])

    def questions(self):
        return self._get("/admin/warmup/questions").get("data", [])


# ------------------------- PTY 驱动单个 claude -------------------------

def seed_claude_home(home: str):
    """预置 .claude.json,尽量跳过首启信任/主题/onboarding 弹窗。"""
    cfg = {
        "hasCompletedOnboarding": True,
        "bypassPermissionsModeAccepted": True,
        "hasTrustDialogAccepted": True,
        "theme": "dark",
        "autoUpdates": False,
        "projects": {
            home: {
                "hasTrustDialogAccepted": True,
                "hasCompletedProjectOnboarding": True,
                "allowedTools": [],
            }
        },
    }
    with open(os.path.join(home, ".claude.json"), "w", encoding="utf-8") as f:
        json.dump(cfg, f)


def wait_idle(child, idle: float, turn_timeout: float, stop: threading.Event) -> str:
    """读 PTY 输出,收到内容后静默 idle 秒判定答完;turn_timeout 兜底。"""
    start = time.time()
    got = False
    while True:
        if stop.is_set():
            return "cancelled"
        if time.time() - start > turn_timeout:
            return "timeout"
        try:
            child.read_nonblocking(size=4096, timeout=idle)
            got = True
        except pexpect.TIMEOUT:
            if got:
                return "done"
        except pexpect.EOF:
            return "eof"


def drain(child, idle: float, max_secs: float, stop: threading.Event):
    """启动后排空输出直到静默(或到最大等待)。"""
    start = time.time()
    while time.time() - start < max_secs:
        if stop.is_set():
            return
        try:
            child.read_nonblocking(size=4096, timeout=idle)
        except pexpect.TIMEOUT:
            return
        except pexpect.EOF:
            return


def session_worker(tok: dict, questions, cfg, stop: threading.Event, sem: threading.Semaphore, counters: dict):
    label = tok.get("name") or f"#{tok.get('id')}"
    if tok.get("allowed_accounts"):
        label += f"→{tok['allowed_accounts']}"

    with sem:
        if stop.is_set():
            return
        home = tempfile.mkdtemp(prefix=f"ccg-warmup-{tok.get('id')}-")
        seed_claude_home(home)

        env = dict(os.environ)
        env["HOME"] = home
        env["ANTHROPIC_BASE_URL"] = cfg.url
        env["ANTHROPIC_API_KEY"] = tok["token"]
        env.pop("ANTHROPIC_AUTH_TOKEN", None)
        env["DISABLE_AUTOUPDATER"] = "1"
        env["TERM"] = "xterm-256color"

        args = ["--bare", "--permission-mode", "bypassPermissions"]
        if cfg.model:
            args += ["--model", cfg.model]

        try:
            child = pexpect.spawn(
                cfg.claude_bin, args, env=env, cwd=home,
                encoding="utf-8", codec_errors="replace",
                timeout=None, dimensions=(40, 120),
            )
        except Exception as e:  # noqa: BLE001
            log(label, f"启动 claude 失败: {e}")
            return

        log(label, "claude 已启动,等待就绪...")
        # 首启稳定:补发回车消除可能的引导弹窗,再等静默。
        if stop.wait(1.5):
            _shutdown(child)
            return
        try:
            child.send("\r")
        except Exception:  # noqa: BLE001
            pass
        drain(child, cfg.idle, 20, stop)

        deadline = time.time() + cfg.total
        worked = 0.0
        sent = 0
        while not stop.is_set() and time.time() < deadline:
            q = random.choice(questions) if questions else "Explain how a hash table works."
            try:
                child.send(q + "\r")
            except Exception as e:  # noqa: BLE001
                log(label, f"写入失败,结束: {e}")
                break

            outcome = wait_idle(child, cfg.idle, cfg.turn_timeout, stop)
            if outcome in ("cancelled", "eof"):
                break
            sent += 1
            with counters["lock"]:
                counters["total"] += 1
            log(label, f"已发 {sent} 条 (本轮:{outcome}) — {q[:40]}")

            if time.time() >= deadline:
                break
            if stop.wait(jittered(cfg.interval, cfg.jitter)):
                break
            worked += cfg.interval

            # 大间隔:工作满 work 秒后长休 rest 秒。
            if cfg.work > 0 and cfg.rest > 0 and worked >= cfg.work:
                log(label, f"进入休息 {cfg.rest}s")
                if stop.wait(cfg.rest):
                    break
                worked = 0.0

        log(label, f"结束,本令牌共发 {sent} 条")
        _shutdown(child)


def _shutdown(child):
    try:
        child.send("/exit\r")
        time.sleep(0.3)
    except Exception:  # noqa: BLE001
        pass
    try:
        child.close(force=True)
    except Exception:  # noqa: BLE001
        pass


# ------------------------- 主流程 -------------------------

def main():
    p = argparse.ArgumentParser(description="独立本地养号运行器(只需 url + admin-key)")
    p.add_argument("--url", required=True, help="网关地址,例如 https://gw.example.com")
    p.add_argument("--admin-key", required=True, help="网关 ADMIN_PASSWORD")
    p.add_argument("--interval", type=int, default=60, help="单条消息间隔(秒),默认 60")
    p.add_argument("--total", type=parse_duration, default="60m", help="总运行时长,如 60m/2h/3600,默认 60m")
    p.add_argument("--work", type=parse_duration, default="0", help="工作时长(到点长休),0=不休息")
    p.add_argument("--rest", type=parse_duration, default="0", help="休息时长")
    p.add_argument("--jitter", type=int, default=20, help="间隔抖动百分比(0-100),默认 20")
    p.add_argument("--model", default="", help="模型别名 opus/sonnet,留空=账号默认")
    p.add_argument("--max-procs", type=int, default=10, help="同时存活的 claude 进程上限,默认 10")
    p.add_argument("--idle", type=float, default=4, help="一轮回答静默判定秒数,默认 4")
    p.add_argument("--turn-timeout", type=float, default=120, help="单轮最大等待秒数,默认 120")
    p.add_argument("--token-ids", default="", help="只养这些令牌 ID(逗号分隔),默认全部 warmup 令牌")
    p.add_argument("--claude-bin", default="claude", help="claude 可执行文件,默认 claude")
    p.add_argument("--insecure", action="store_true", help="跳过 HTTPS 证书校验")
    p.add_argument("--list", action="store_true", help="只列出会用到的养号令牌后退出")
    cfg = p.parse_args()
    cfg.url = cfg.url.rstrip("/")
    _lazy_imports(need_pexpect=not cfg.list)

    gw = Gateway(cfg.url, cfg.admin_key, verify=not cfg.insecure)
    tokens = gw.warmup_tokens()
    if cfg.token_ids:
        wanted = {int(x) for x in cfg.token_ids.split(",") if x.strip().isdigit()}
        tokens = [t for t in tokens if t.get("id") in wanted]
    # 只保留 active 的
    tokens = [t for t in tokens if t.get("status", "active") == "active" and t.get("token")]

    if not tokens:
        sys.exit("没有可用的养号令牌。请到网关「令牌」页创建分类为「养号专用」的令牌。")

    print(f"将养号 {len(tokens)} 个令牌:")
    for t in tokens:
        acc = f" →账号 {t['allowed_accounts']}" if t.get("allowed_accounts") else ""
        print(f"  #{t.get('id')} {t.get('name') or '(未命名)'}{acc}")
    if cfg.list:
        return

    questions = gw.questions()
    print(f"题库 {len(questions)} 题。节奏: 间隔 {cfg.interval}s±{cfg.jitter}% · 总 {cfg.total}s"
          f" · 工作/休息 {cfg.work}s/{cfg.rest}s · 模型 {cfg.model or '默认'}")
    print(f"基址(下发给 claude 的 ANTHROPIC_BASE_URL): {cfg.url}")
    print("按 Ctrl-C 停止。\n")

    stop = threading.Event()
    sem = threading.Semaphore(cfg.max_procs)
    counters = {"total": 0, "lock": threading.Lock()}
    threads = []
    for t in tokens:
        th = threading.Thread(target=session_worker, args=(t, questions, cfg, stop, sem, counters), daemon=True)
        th.start()
        threads.append(th)

    try:
        while any(th.is_alive() for th in threads):
            time.sleep(0.5)
    except KeyboardInterrupt:
        print("\n收到 Ctrl-C,正在停止所有养号进程...", flush=True)
        stop.set()
        for th in threads:
            th.join(timeout=10)

    print(f"\n全部结束。累计发送 {counters['total']} 条消息。")


if __name__ == "__main__":
    main()
