#!/usr/bin/env python3
"""Sets up Ollama and the model tools/port_agent.py drives.

    python tools/port_agent_model.py                       # from qwen3.8-custom-64k
    python tools/port_agent_model.py --base qwen3.8:27b --name qwen3.8-port

1. Working memory: flash attention on and the KV cache at q8_0 (user
   environment variables OLLAMA_FLASH_ATTENTION=1, OLLAMA_KV_CACHE_TYPE=q8_0),
   which halves the cache, so about twice the context fits; Ollama is
   restarted to pick them up.
2. The model: `--name` from `--base` with the largest context in
   `--sizes` that loads entirely on the GPU (a context spilling to the CPU
   runs several times slower), and temperature 0.6 (Qwen's advice for
   thinking models doing code; the base may use 1.0).

Idempotent: run it again after changing GPUs or models.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

OLLAMA = os.environ.get("OLLAMA_HOST", "http://127.0.0.1:11434").rstrip("/")
ENV = {"OLLAMA_FLASH_ATTENTION": "1", "OLLAMA_KV_CACHE_TYPE": "q8_0"}


def log(msg):
    print(f"[port-model] {msg}", flush=True)


def api(path, body=None, timeout=600):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(f"{OLLAMA}{path}", data=data, method="POST" if data else "GET")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def up():
    try:
        api("/api/version", timeout=3)
        return True
    except OSError:
        return False


def set_env():
    """Persists the variables for the user; True when any changed."""
    changed = False
    for k, v in ENV.items():
        if os.name == "nt":
            cur = subprocess.run(
                ["powershell", "-NoProfile", "-Command", f"[Environment]::GetEnvironmentVariable('{k}','User')"],
                capture_output=True, text=True,
            ).stdout.strip()
            if cur != v:
                subprocess.run(
                    ["powershell", "-NoProfile", "-Command", f"[Environment]::SetEnvironmentVariable('{k}','{v}','User')"],
                    check=True,
                )
                changed = True
        elif os.environ.get(k) != v:
            log(f"set {k}={v} in the environment Ollama's service sees (systemd: Environment=)")
        os.environ[k] = v
    return changed


def restart():
    """Stops Ollama and starts it again with ENV."""
    log("restarting ollama")
    if os.name == "nt":
        for image in ("ollama app.exe", "ollama.exe"):
            subprocess.run(["taskkill", "/F", "/IM", image], capture_output=True)
        time.sleep(2)
        app = os.path.join(os.environ.get("LOCALAPPDATA", ""), "Programs", "Ollama", "ollama app.exe")
        env = dict(os.environ, **ENV)
        if os.path.isfile(app):
            subprocess.Popen([app], env=env, creationflags=subprocess.DETACHED_PROCESS)
        else:
            subprocess.Popen([shutil.which("ollama"), "serve"], env=env,
                             creationflags=subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP)
    else:
        subprocess.run(["pkill", "-f", "ollama serve"], capture_output=True)
        subprocess.Popen(["ollama", "serve"], env=dict(os.environ, **ENV),
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    for _ in range(60):
        time.sleep(1)
        if up():
            return
    sys.exit("ollama didn't come back up")


def create(name, base, ctx, temperature):
    modelfile = f"FROM {base}\nPARAMETER num_ctx {ctx}\nPARAMETER temperature {temperature}\n"
    with tempfile.NamedTemporaryFile("w", suffix=".Modelfile", delete=False) as f:
        f.write(modelfile)
        path = f.name
    try:
        subprocess.run([shutil.which("ollama"), "create", name, "-f", path], check=True,
                       capture_output=True, text=True)
    finally:
        os.unlink(path)


def fits(name):
    """Loads the model; (share of it in VRAM, its size)."""
    api("/api/generate", {"model": name, "prompt": "hi", "stream": False,
                          "options": {"num_predict": 1}, "keep_alive": "2m"})
    for m in api("/api/ps").get("models", []):
        if m["name"].split(":")[0] == name.split(":")[0]:
            return m.get("size_vram", 0) / max(m.get("size", 1), 1), m.get("size", 0)
    return 0.0, 0


def unload(name):
    try:
        api("/api/generate", {"model": name, "keep_alive": 0})
    except OSError:
        pass


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--base", default="qwen3.8-custom-64k")
    ap.add_argument("--name", default="qwen3.8-port")
    ap.add_argument("--sizes", default="131072,114688,98304,81920,65536")
    ap.add_argument("--temperature", type=float, default=0.6)
    ap.add_argument("--no-restart", action="store_true")
    a = ap.parse_args()

    changed = set_env()
    if changed or not up():
        if a.no_restart and up():
            log("environment changed; restart Ollama yourself to use it")
        else:
            restart()
    for size in [int(s) for s in a.sizes.split(",")]:
        log(f"trying {a.name} at num_ctx {size}")
        create(a.name, a.base, size, a.temperature)
        share, total = fits(a.name)
        unload(a.name)
        log(f"  {share:.0%} of {total / 2**30:.1f} GiB on the GPU")
        if share >= 0.999:
            log(f"{a.name}: num_ctx {size}, temperature {a.temperature}, all on the GPU")
            return 0
    log(f"no size fits fully on the GPU; {a.name} is left at the smallest tried")
    return 1


if __name__ == "__main__":
    sys.exit(main())
