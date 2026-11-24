#!/usr/bin/env python3
"""Ports a mod's client renderers to the Torch mod API with a local coding
agent (Qwen Code on Ollama), one renderer per session, behind guardrails.

    python tools/port_agent.py create                  # next 3 rows to do
    python tools/port_agent.py create --all            # every row left
    python tools/port_agent.py createaddition --all    # a mod with no port yet
    python tools/port_agent.py create --targets create:millstone --apply --install
    python tools/port_agent.py create --inventory      # list renderers only

A run:

1. makes sure Ollama serves the model, and reads its context size;
2. decompiles the mod's jar once (pinned Vineflower, cached under
   tools/port_agent_work/<modid>/java, git-ignored);
3. takes the renderer list from the Java itself (Registrate chains,
   NeoForge's registerBlockEntityRenderer, vanilla's
   BlockEntityRenderers.register) and adds missing ones to PORTED.md; a mod
   with no port gets ports/<modid>/ from a skeleton first;
4. snapshots your tree (uncommitted work too) into a git worktree;
5. per renderer, a fresh Qwen Code session given the renderer's Java, the
   NBT its block entity sends, the partial models it names and the
   block's properties up front; file edits allowed, shell limited to
   cargo, rg, ls and read-only git;
6. gates each renderer: changes only in ports/<modid>/, no new
   dependencies or `unsafe`, rustfmt, tests and wasm build without
   warnings, a new test for a ported renderer, and torch-port-check
   (the plugin run over every block state, every model, block, sprite and
   item it names checked against the assets). A failed gate gets a fix
   session with the errors (--fixes), then the renderer is rolled back and
   recorded in tools/port_agent_work/<modid>/attempts.json, and skipped
   next time (--retry to try again);
7. commits each renderer that passes in the worktree, and writes one patch
   of them all; --apply applies it to your tree, --install puts the
   plugin into the mod's .torchmod packages.

Paths can be moved for a standalone install (crates/torch-converter): see the
TORCH_PORT_* variables below. --events prints progress lines (`@@{json}`)
for a front end.

Logs and reports: tools/port_agent_work/logs/. The agent's rules:
docs/porting/AGENT_GUIDE.md. Review the patch before committing: the gates
catch broken and made-up output, not every mismatch with the Java.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile
from pathlib import Path

def _path(var, default):
    return Path(os.environ[var]).resolve() if os.environ.get(var) else default


# The git checkout holding docs/, crates/torch-mod-api and ports/.
ROOT = _path("TORCH_PORT_ROOT", Path(__file__).resolve().parent.parent)
WORK = _path("TORCH_PORT_WORK", ROOT / "tools" / "port_agent_work")
# The mod jars, and the converted packages.
MODS = _path("TORCH_PORT_MODS", ROOT / "tools" / "moddump" / "Mods")
CONVERTED = _path("TORCH_PORT_PACKAGES", ROOT / "tools" / "moddump" / "converted")
# Vanilla's assets (`.../assets/minecraft`), for torch-port-check.
VANILLA = _path("TORCH_PORT_VANILLA", ROOT / "reference" / "minecraft-26.3" / "assets" / "minecraft")
CHECKER = ROOT / "crates" / "torch-port-check"
# A prebuilt torch-port-check, else it's built from CHECKER.
CHECKER_EXE = _path("TORCH_PORT_CHECK", None) if os.environ.get("TORCH_PORT_CHECK") else None
# The client's mods folder `--install` also writes to; "" for none.
CLIENT_MODS = os.environ.get(
    "TORCH_PORT_CLIENT_MODS",
    str(Path(os.environ["LOCALAPPDATA"]) / "torch-client" / "config" / "mods") if os.environ.get("LOCALAPPDATA") else "",
)

OLLAMA = os.environ.get("OLLAMA_HOST", "http://127.0.0.1:11434").rstrip("/")
DEFAULT_MODEL = "qwen3.8-port"

VINEFLOWER = (
    "https://repo1.maven.org/maven2/org/vineflower/vineflower/1.12.0/vineflower-1.12.0.jar",
    "1dfcfe974395734fa467ce620661c7623d05ba83670de0529b1fbd63ff548b9d",
)

# Shell commands the agent may run (prefix match); anything else is
# refused in headless mode.
ALLOWED_SHELL = ["cargo", "rg", "ls", "git diff", "git status", "git log"]
# The tools a port needs; the rest only cost context.
CORE_TOOLS = ["read_file", "edit", "write_file", "grep_search", "glob", "run_shell_command"]
# Java put into a prompt, at most (lines).
JAVA_LINES = 260


class Fail(Exception):
    pass


EVENTS = False


def log(msg):
    print(f"[port-agent] {msg}", flush=True)


def event(ev, **fields):
    """A progress line for a front end (`--events`): stage, step, command
    or done, as torch-convert --progress writes them."""
    if EVENTS:
        print("@@" + json.dumps({"ev": ev, **fields}), flush=True)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def run(cmd, cwd=None, check=True, **kw):
    r = subprocess.run(
        cmd, cwd=cwd, text=True, encoding="utf-8", errors="replace", capture_output=True, **kw
    )
    if check and r.returncode != 0:
        raise Fail(f"{' '.join(map(str, cmd))}: exit {r.returncode}\n{r.stdout}{r.stderr}")
    return r


# --- Ollama -----------------------------------------------------------------


def ollama_up():
    try:
        with urllib.request.urlopen(f"{OLLAMA}/api/version", timeout=3) as r:
            return json.load(r).get("version")
    except OSError:
        return None


def ensure_ollama(model):
    """Starts Ollama if needed; the model's context size."""
    if not ollama_up():
        exe = shutil.which("ollama")
        if not exe:
            raise Fail("ollama isn't installed (https://ollama.com)")
        log("starting ollama serve")
        flags = 0
        if os.name == "nt":
            flags = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
        subprocess.Popen(
            [exe, "serve"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=flags
        )
        for _ in range(60):
            time.sleep(1)
            if ollama_up():
                break
        else:
            raise Fail("ollama didn't come up")
    with urllib.request.urlopen(f"{OLLAMA}/api/tags", timeout=10) as r:
        names = {m["name"] for m in json.load(r)["models"]}
    if model not in names and f"{model}:latest" not in names:
        hint = " (tools/port_agent_model.py makes it)" if model == DEFAULT_MODEL else ""
        raise Fail(f"ollama has no model {model}{hint}; have {sorted(names)}")
    req = urllib.request.Request(
        f"{OLLAMA}/api/show", data=json.dumps({"model": model}).encode(), method="POST"
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        params = json.load(r).get("parameters", "")
    ctx = re.search(r"num_ctx\s+(\d+)", params)
    ctx = int(ctx.group(1)) if ctx else 8192
    if ctx < 32768:
        log(f"warning: {model} has num_ctx {ctx}; a port needs 32k+ (tools/port_agent_model.py)")
    return ctx


# --- The mod's jar and Java ---------------------------------------------------


def mods_toml(jar):
    try:
        with zipfile.ZipFile(jar) as z:
            return z.read("META-INF/neoforge.mods.toml").decode("utf-8", "replace")
    except (KeyError, zipfile.BadZipFile):
        return ""


def mod_tables(toml):
    """The parts of a mods.toml that describe the jar's own mods: the
    top level (an inline `mods = [{...}]`) and `[[mods]]` tables, never
    `[[dependencies...]]` ones, which name other mods."""
    chunks = re.split(r"^\s*(?=\[)", toml, flags=re.M)
    return [c for c in chunks if not c.lstrip().startswith("[") or c.lstrip().startswith("[[mods]]")]


def toml_values(text, key):
    """Every `key = "v"` or `key = 'v'` in `text`."""
    return re.findall(rf"\b{key}\s*=\s*[\"']([^\"']+)[\"']", text)


def declared_mods(toml):
    """(modId, version, licence) of each of the jar's own mods."""
    licence = toml_values(re.split(r"^\s*(?=\[)", toml, flags=re.M)[0], "license")
    out = []
    for table in mod_tables(toml):
        ids, versions = toml_values(table, "modId"), toml_values(table, "version")
        for i, mid in enumerate(ids):
            out.append((mid, versions[i] if i < len(versions) else "?", licence[0] if licence else "?"))
    return out


def find_jar(modid, jar):
    if jar:
        return Path(jar).resolve()
    hits = [p for p in sorted(MODS.glob("*.jar")) if any(m[0] == modid for m in declared_mods(mods_toml(p)))]
    if not hits:
        raise Fail(f"no jar in {MODS} declares mod id {modid}; pass --jar")
    return hits[-1]


def vineflower():
    path = WORK / "vineflower-1.12.0.jar"
    url, expected = VINEFLOWER
    if path.is_file() and sha256(path) == expected:
        return path
    path.parent.mkdir(parents=True, exist_ok=True)
    log(f"downloading {url}")
    part = path.with_name(path.name + ".part")
    subprocess.run(["curl", "-fsSL", "-o", str(part), url], check=True)
    if sha256(part) != expected:
        part.unlink()
        raise Fail(f"{url}: sha256 mismatch")
    part.replace(path)
    return path


def decompiled(modid, jar):
    """The jar's classes as Java, decompiled once per jar hash."""
    out = WORK / modid / "java"
    stamp = WORK / modid / "java.sha256"
    digest = sha256(jar)
    if out.is_dir() and stamp.is_file() and stamp.read_text().strip() == digest:
        return out
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    with tempfile.TemporaryDirectory() as tmp:
        classes = Path(tmp) / "classes.jar"
        with zipfile.ZipFile(jar) as z, zipfile.ZipFile(classes, "w") as o:
            for name in z.namelist():
                if name.endswith(".class"):
                    o.writestr(name, z.read(name))
        log(f"decompiling {jar.name} (once; a big mod takes minutes)")
        subprocess.run(
            ["java", "-jar", str(vineflower()), "-log=WARN", "--skip-extra-files=1",
             "--folder", str(classes), str(out)],
            check=True, stdout=subprocess.DEVNULL,
        )
    stamp.write_text(digest)
    return out


class Java:
    """The decompiled source, indexed by top-level class name."""

    def __init__(self, root):
        self.root = root
        self.files = {}
        for path in sorted(root.rglob("*.java")):
            self.files.setdefault(path.stem, path)
        self._text = {}
        self._partials = None

    def text(self, name):
        path = self.files.get(name)
        if path is None:
            return ""
        if name not in self._text:
            self._text[name] = path.read_text(encoding="utf-8", errors="replace")
        return self._text[name]

    def supers(self, name):
        """The classes and interfaces `name` extends or implements."""
        m = re.search(
            rf"\b(?:class|interface)\s+{re.escape(name)}\b(?:<[^{{]*?>)?([^{{]*)\{{", self.text(name)
        )
        if not m:
            return []
        head = m.group(1)
        # Drop type arguments (nested ones too).
        while re.search(r"<[^<>]*>", head):
            head = re.sub(r"<[^<>]*>", "", head)
        return re.findall(r"\b([A-Z][A-Za-z0-9_]*)\b", head)

    def inventory(self, modid):
        """(block entity type id, renderer class) for every renderer the
        mod registers."""
        found = {}
        field_ids = {}
        texts = {name: self.text(name) for name in self.files}
        for text in texts.values():
            # Registrate: FIELD = ...blockEntity("id", ...)....renderer(() -> X::new)...;
            for m in re.finditer(r"\b([A-Z][A-Z0-9_]*)\s*=\s*[^;=]*?\bblockEntity\(\s*\"([a-z0-9_/.-]+)\"([^;]*);", text, flags=re.S):
                field, ident, rest = m.groups()
                field_ids[field] = ident
                # `X::new` or a factory (`GaugeRenderer::speed`).
                r = re.search(r"\.renderer\(\s*\(\)\s*->\s*(\w+)::\w+", rest)
                if r:
                    found[f"{modid}:{ident}"] = r.group(1)
            # DeferredRegister: FIELD = X.register("id", ...).
            for field, ident in re.findall(
                r"\b([A-Z][A-Z0-9_]*)\s*=\s*[\w.]+\.register\(\s*\"([a-z0-9_/.-]+)\"", text
            ):
                field_ids.setdefault(field, ident)
        for text in texts.values():
            # NeoForge / vanilla: register...Renderer(...FIELD.get(), X::new).
            for field, renderer in re.findall(
                r"(?:registerBlockEntityRenderer|BlockEntityRenderers\.register)\(\s*"
                r"(?:\([^)]*\)\s*)?[\w.]*?\b([A-Z][A-Z0-9_]*)(?:\.get\(\))?\s*,\s*"
                r"(?:\w+\s*->\s*new\s+)?(\w+)(?:::new)?",
                text,
            ):
                ident = field_ids.get(field)
                if ident:
                    found.setdefault(f"{modid}:{ident}", renderer)
        return dict(sorted(found.items()))

    def partials(self):
        """Partial model constants to model ids (`ENCASED_FAN_INNER` to
        `{ns}:block/encased_fan/propeller`), from `NAME = block("path")`
        style helpers or full ids."""
        if self._partials is not None:
            return self._partials
        out = {}
        for name in self.files:
            text = self.text(name)
            if "PartialModel" not in text:
                continue
            for const, helper, lit in re.findall(
                r"\b([A-Z][A-Z0-9_]+)\s*=\s*(\w+)\(\s*\"([a-z0-9_/.:-]+)\"\s*\)", text
            ):
                if ":" in lit:
                    out[const] = lit
                elif helper in ("block", "entity", "item"):
                    out[const] = f"{{ns}}:{helper}/{lit}"
        self._partials = out
        return out


def strip_java(text):
    """Java with imports, the package line and blank lines dropped."""
    return "\n".join(
        l for l in text.splitlines() if not l.startswith(("import ", "package ")) and l.strip()
    )


def clip(text, lines=JAVA_LINES):
    rows = text.splitlines()
    if len(rows) <= lines:
        return text
    return "\n".join(rows[:lines]) + f"\n// ... {len(rows) - lines} more lines: read the file by range"


NBT_LINE = re.compile(
    r"\.(put|get)(Int|Float|Double|Boolean|String|Long|Byte|Short|Compound|List|IntArray|UUID)?\(\s*\""
    r"|\b(write|read|saveAdditional|loadAdditional|getUpdateTag|writeSafe)\s*\("
)


def context_pack(java, modid, renderer, kind, blocks):
    """What a session needs up front: the renderer, its block entity's
    NBT, the partial models it names, the block's properties."""
    out = []
    src = java.text(renderer)
    if src:
        rel = java.files[renderer].relative_to(java.root).as_posix()
        out.append(f"### {renderer} ({java.root.as_posix()}/{rel})\n```java\n{clip(strip_java(src))}\n```")
    else:
        out.append(f"(No class {renderer} in the decompiled source: find it with rg.)")
    # Its block entity: the renderer's type argument, and its supers.
    be = re.search(rf"class\s+{re.escape(renderer)}\b[^{{]*?<\s*(\w+)", src)
    seen, queue, nbt = set(), [be.group(1)] if be else [], []
    while queue and len(seen) < 5:
        cls = queue.pop(0)
        if cls in seen or cls not in java.files:
            continue
        seen.add(cls)
        found = [l.strip() for l in java.text(cls).splitlines() if NBT_LINE.search(l)]
        if found:
            nbt.append(f"// {cls}\n" + "\n".join(found[:50]))
        queue += [s for s in java.supers(cls) if s.endswith("BlockEntity")]
    if nbt:
        out.append("### NBT its block entity writes and reads (lines only; open the class for more)\n"
                   f"```java\n{chr(10).join(nbt)}\n```")
    partials = java.partials()
    named = sorted(set(re.findall(r"\b([A-Z][A-Z0-9_]{2,})\b", src)) & set(partials))
    if named:
        lines = "\n".join(f"- {c} = {partials[c].replace('{ns}', modid)}" for c in named)
        out.append(f"### Partial models the renderer names (model ids for `Part::model`)\n{lines}")
    carriers = [b for b in blocks.get("blocks", []) if b.get("block_entity") == kind]
    if carriers:
        lines = "\n".join(
            f"- {b['name']}: "
            + (", ".join(f"{p[0]}={'|'.join(p[1])}" for p in b.get("properties", [])) or "no properties")
            for b in carriers[:8]
        )
        out.append(f"### Blocks with this block entity, and their state properties\n{lines}")
    return "\n\n".join(out)


def package(modid):
    hits = sorted(CONVERTED.glob(f"{modid}-*.torchmod"))
    return hits[-1] if hits else None


def package_blocks(pkg):
    if not pkg:
        return {}
    with zipfile.ZipFile(pkg) as z:
        try:
            return json.loads(z.read("blocks.json"))
        except KeyError:
            return {}


# --- PORTED.md ----------------------------------------------------------------

STATUSES = ("done", "partial", "skipped", "not yet", "next")


def rows(ported):
    """(type id, Java class, status, notes) of each renderer row."""
    out = []
    if not ported.is_file():
        return out
    for line in ported.read_text(encoding="utf-8").splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4:
            continue
        ids = [c.strip("`") for c in cells[:2] if re.fullmatch(r"`[a-z0-9_.-]+:[a-z0-9_./-]+`", c)]
        if not ids:
            continue
        kind = ids[0]
        java = next(c.strip("`") for c in cells[:2] if c.strip("`") != kind)
        out.append((kind, java, cells[2], cells[3]))
    return out


def row_status(ported, kind):
    return next((r[2] for r in rows(ported) if r[0] == kind), None)


def todo(ported, skip):
    """Rows still to do, "next" ones first, minus those to skip."""
    left = [r for r in rows(ported) if r[2] in ("not yet", "next") and r[0] not in skip]
    left.sort(key=lambda r: r[2] != "next")
    return [(r[0], r[1], r[3]) for r in left]


def add_rows(ported, found):
    """Adds inventory rows PORTED.md lacks, as "not yet"."""
    known = {r[0] for r in rows(ported)}
    new = [(k, cls) for k, cls in found.items() if k not in known]
    if not new:
        return 0
    text = ported.read_text(encoding="utf-8").rstrip("\n").splitlines()
    last = max(i for i, l in enumerate(text) if l.startswith("|"))
    text[last + 1:last + 1] = [f"| `{k}` | `{cls}` | not yet |  |" for k, cls in new]
    ported.write_text("\n".join(text) + "\n", encoding="utf-8")
    return len(new)


SKELETON_LIB = '''//! {name} {version}'s client renderers, ported to the Torch mod API
//! (`docs/porting/AGENT_GUIDE.md`). What's done and skipped: `PORTED.md`.

#![allow(dead_code)]

mod dir;
mod pose;

use torch_mod_api::{{BlockEntityView, Plugin, Registration, Rig, export_plugin}};

/// Block entity types drawn here.
const HANDLED: &[&str] = &[];

/// Types whose block model the Java leaves out of the terrain (drawn by
/// the renderer instead).
const REPLACES_BLOCK_MODEL: &[&str] = &[];

#[derive(Default)]
struct Port;

impl Plugin for Port {{
    fn init(&mut self) -> Registration {{
        Registration {{
            block_entities: HANDLED.iter().map(|s| (*s).to_owned()).collect(),
            replaces_block_model: REPLACES_BLOCK_MODEL.iter().map(|s| (*s).to_owned()).collect(),
            sprites: Vec::new(),
        }}
    }}

    fn block_entity(&mut self, view: &BlockEntityView) -> Rig {{
        #[allow(clippy::match_single_binding)]
        let parts = match view.kind.as_str() {{
            _ => Vec::new(),
        }};
        Rig {{ parts }}
    }}
}}

export_plugin!(Port);
'''


def bootstrap(tree, modid, jar, found):
    """A new port: the example crate renamed, a skeleton lib.rs, the
    Create port's general helpers (directions, PoseStack), and PORTED.md
    with the inventory."""
    port = tree / "ports" / modid
    shutil.copytree(tree / "ports" / "example", port, ignore=shutil.ignore_patterns("target", "Cargo.lock"))
    cargo = (port / "Cargo.toml").read_text(encoding="utf-8")
    cargo = cargo.replace('name = "torch-port-example"', f'name = "torch-port-{modid}"')
    cargo = re.sub(r'(?m)^description = ".*"$', f'description = "{modid} client renderers ported to the Torch mod API"', cargo)
    (port / "Cargo.toml").write_text(cargo, encoding="utf-8")
    info = next((m for m in declared_mods(mods_toml(jar)) if m[0] == modid), (modid, "?", "?"))
    (port / "src" / "lib.rs").write_text(SKELETON_LIB.format(name=modid, version=info[1]), encoding="utf-8")
    for helper in ("dir.rs", "pose.rs"):
        shutil.copy(tree / "ports" / "create" / "src" / helper, port / "src" / helper)
    table = "\n".join(f"| `{k}` | `{cls}` | not yet |  |" for k, cls in found.items())
    (port / "PORTED.md").write_text(
        f"""# {modid} port

- Mod: {modid} {info[1]} (`{jar.name}`, sha256 {sha256(jar)[:16]}...)
- Licence: {info[2]} (check before sharing the port: it derives from the mod's code)
- API: 1
- Source: decompiled with Vineflower 1.12.0

## Renderers

| Block entity type | Java renderer | Status | Notes |
|---|---|---|---|
{table}

## Needs from the API

""",
        encoding="utf-8",
    )


# --- Worktree -----------------------------------------------------------------


def snapshot():
    """A commit of the working tree as it is (tracked, changed and new
    files, minus ignored ones), made without touching the index or a
    branch."""
    with tempfile.TemporaryDirectory() as tmp:
        env = dict(os.environ, GIT_INDEX_FILE=str(Path(tmp) / "index"))
        run(["git", "read-tree", "HEAD"], cwd=ROOT, env=env)
        run(["git", "add", "-A"], cwd=ROOT, env=env)
        tree = run(["git", "write-tree"], cwd=ROOT, env=env).stdout.strip()
    return run(["git", "commit-tree", tree, "-p", "HEAD", "-m", "port-agent snapshot"], cwd=ROOT).stdout.strip()


def git(tree, *args, check=True):
    return run(["git", "-c", "user.name=port-agent", "-c", "user.email=port-agent@localhost", *args],
               cwd=tree, check=check)


def commit(tree, message):
    git(tree, "add", "-A", "--", ".", ":!.qwen")
    git(tree, "commit", "-q", "--allow-empty", "-m", message)
    return git(tree, "rev-parse", "HEAD").stdout.strip()


# --- The agent ----------------------------------------------------------------

PREAMBLE = """You are porting a Minecraft mod's client renderers (`{modid}`) to the
Torch mod API, in Rust. You work alone and unattended: nobody answers
questions, so decide and continue.

Your context is small. Read only what you need: find things with `rg -n`
and read files over 200 lines by line range (read_file offset/limit),
never whole. Write code as soon as you know what to write.

What a port is: the client never runs the mod's Java. A plugin answers a
block entity's state (block id, properties, NBT) with a `Rig`: parts,
each a geometry (a model, a block, a cuboid or an item), a static
transform and an animation the client plays. Per-frame Java becomes
"state in, parts plus motion out". No `unsafe`, no threads, no I/O, no new
dependencies.

The API and how Java maps onto it: docs/porting/JAVA_TO_API.md (read it),
and docs/MOD_API.md (read the section "What a plugin answers: `Rig`").
"""

RULES = """Rules:
- Edit only files under `ports/{modid}/`. Never edit docs/, src/, crates/,
  and never Cargo.toml.
- Don't fake what the API can't express: mark the row partial (saying
  what's missing) or skipped (saying why), and add a line under "Needs
  from the API" in PORTED.md.
- Keep comments short: name the Java class and method a function follows.
- Every ported renderer gets a unit test checking its parts, transforms
  and animation numbers against the Java formulas.
- When the code is written, run in `ports/{modid}/`:
    cargo test
    cargo build --release --target wasm32-unknown-unknown
  and fix every error and warning.
- Shell: only cargo, rg, ls, git diff, git status, git log.
- Finish with one line: what you ported or skipped, and why.
"""


def helpers(tree, modid):
    """The port's existing public functions, one line each."""
    r = run(["rg", "-n", "--no-heading", r"^pub fn ", str(tree / "ports" / modid / "src")], check=False)
    lines = []
    for l in r.stdout.splitlines():
        m = re.match(r"(.*?):(\d+):(.*)", l)
        if m:
            lines.append(f"- {Path(m.group(1)).name}:{m.group(2)}: {m.group(3).strip().rstrip('{').strip()}")
    return "\n".join(lines[:60]) or "(none yet)"


def port_prompt(tree, modid, java, target, pack):
    kind, cls, notes = target
    return (
        PREAMBLE.format(modid=modid)
        + f"""
Task: port ONE block entity renderer, `{kind}` (Java `{cls}`{', note: ' + notes if notes else ''}).

1. Read the Java below; open other classes it uses with `rg -l "class Name"
   {java.root.as_posix()}` and read them by range.
2. Write the port: a function in a module in `ports/{modid}/src/` (a new
   module, or one that fits), `{kind}` added to `HANDLED` in
   `ports/{modid}/src/lib.rs` with a dispatch arm, and a unit test.
3. Set its row in `ports/{modid}/PORTED.md`: done, partial (what's missing)
   or skipped (why), with a short note.

Existing helpers in the port you can reuse (file:line: signature):
{helpers(tree, modid)}

{pack}

"""
        + RULES.format(modid=modid)
    )


def fix_prompt(modid, java, target, problems):
    kind, cls, _ = target
    return (
        PREAMBLE.format(modid=modid)
        + f"""
Task: a previous session ported `{kind}` (Java `{cls}`), but the checks
failed. See its work with `git diff HEAD` (allowed), then fix every
problem below. The Java is under {java.root.as_posix()} (read-only).

Problems:
{problems}

"""
        + RULES.format(modid=modid)
    )


def qwen_settings(tree, model, ctx):
    """Project settings for the run: the model as an Ollama provider with
    its real context window, so Qwen compacts before it overflows."""
    settings = {
        "model": {"name": model},
        "modelProviders": {"openai": [{
            "id": model, "name": f"{model} (Ollama)", "baseUrl": f"{OLLAMA}/v1",
            "envKey": "OLLAMA_API_KEY", "generationConfig": {"contextWindowSize": ctx},
        }]},
        "security": {"auth": {"selectedType": "openai", "baseUrl": f"{OLLAMA}/v1"}},
        "context": {"autoCompactThreshold": 0.7},
    }
    path = Path(tree) / ".qwen" / "settings.json"
    path.parent.mkdir(exist_ok=True)
    path.write_text(json.dumps(settings, indent=2), encoding="utf-8")


def qwen(tree, java, text, a, logfile, modid):
    exe = shutil.which("qwen")
    if not exe:
        raise Fail("Qwen Code isn't installed (npm install -g @qwen-code/qwen-code)")
    cmd = [
        exe, "--model", a.model, "--approval-mode", "auto-edit",
        "--include-directories", str(java.root),
        "--max-wall-time", f"{a.minutes}m", "--max-tool-calls", str(a.max_tool_calls),
        "--output-format", "stream-json", "--chat-recording",
        # No personal extensions, skills or context files: a run depends
        # only on the repo.
        "--safe-mode",
        "-p", "Do the task described above.",
    ]
    for c in ALLOWED_SHELL:
        cmd += ["--allowed-tools", f"run_shell_command({c})"]
    for t in CORE_TOOLS:
        cmd += ["--core-tools", t]
    env = dict(os.environ, CARGO_TARGET_DIR=str(WORK / "target" / modid), OLLAMA_API_KEY="ollama")
    result = None
    with open(logfile, "a", encoding="utf-8") as out:
        p = subprocess.Popen(cmd, cwd=tree, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, text=True, encoding="utf-8",
                             errors="replace", env=env)
        p.stdin.write(text)
        p.stdin.close()
        for line in p.stdout:
            out.write(line)
            out.flush()
            try:
                parsed = json.loads(line)
            except ValueError:
                continue
            summary = describe(ev_obj := event_obj(line))
            if summary:
                log(summary)
                if ev_obj.get("type") == "assistant":
                    event("command", cmd=summary.strip())
            if parsed.get("type") == "result":
                result = parsed
        code = p.wait()
    return code, (result or {}).get("result") or ""


def event_obj(line):
    try:
        return json.loads(line)
    except ValueError:
        return {}


def describe(event):
    kind = event.get("type")
    if kind == "assistant":
        for block in event.get("message", {}).get("content", []):
            if block.get("type") == "tool_use":
                args = json.dumps(block.get("input", {}))[:110]
                return f"    {block.get('name')} {args}"
    if kind == "result":
        return f"    session over: {event.get('subtype', '')}, {event.get('num_turns', '?')} turns"
    return None


# --- Gates ----------------------------------------------------------------------


def checker():
    if CHECKER_EXE:
        return CHECKER_EXE
    exe = CHECKER / "target" / "release" / ("torch-port-check.exe" if os.name == "nt" else "torch-port-check")
    if not exe.is_file():
        log("building torch-port-check")
        run(["cargo", "build", "--release", "--quiet"], cwd=CHECKER)
    return exe


def wasm_path(modid):
    return WORK / "target" / modid / "wasm32-unknown-unknown" / "release" / f"torch_port_{modid.replace('-', '_')}.wasm"


def count_tests(port):
    return sum(f.read_text(encoding="utf-8", errors="replace").count("#[test]") for f in (port / "src").rglob("*.rs"))


def tail(text, lines=30):
    return "\n".join(text.strip().splitlines()[-lines:])


def gate(tree, modid, base, kind, tests_before):
    """Problems with the renderer's change since `base`, as text lines."""
    port = tree / "ports" / modid
    problems = []
    env = dict(os.environ, CARGO_TARGET_DIR=str(WORK / "target" / modid))
    run(["cargo", "fmt"], cwd=port, check=False)
    git(tree, "add", "-A", "--", ".", ":!.qwen")
    changed = git(tree, "diff", "--cached", "--name-only", base).stdout.split()
    outside = [f for f in changed if not f.startswith(f"ports/{modid}/")]
    if outside:
        problems.append(f"- files changed outside ports/{modid}/ (not allowed; restore them): {', '.join(outside)}")
    if f"ports/{modid}/Cargo.toml" in changed:
        problems.append("- Cargo.toml changed (no new dependencies allowed): restore it")
    diff = git(tree, "diff", "--cached", base, "--", f"ports/{modid}/src").stdout
    if re.search(r"^\+.*\bunsafe\b", diff, flags=re.M):
        problems.append("- `unsafe` added: not allowed")
    status = row_status(port / "PORTED.md", kind)
    if status not in STATUSES or status in ("not yet", "next"):
        problems.append(f"- its PORTED.md row says \"{status}\": set done, partial or skipped, with a note")
    lib = (port / "src" / "lib.rs").read_text(encoding="utf-8")
    if status in ("done", "partial"):
        if f'"{kind}"' not in lib:
            problems.append(f"- `{kind}` isn't in HANDLED and dispatched in src/lib.rs")
        if count_tests(port) <= tests_before:
            problems.append("- no new unit test for it")
    t = run(["cargo", "test", "--quiet"], cwd=port, check=False, env=env)
    if t.returncode != 0:
        problems.append("- cargo test fails:\n" + tail(t.stdout + t.stderr))
    w = run(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown"], cwd=port, check=False, env=env)
    warns = [l for l in (w.stdout + w.stderr).splitlines() if l.startswith("warning") and "generated" not in l]
    if w.returncode != 0:
        problems.append("- the wasm build fails:\n" + tail(w.stdout + w.stderr))
    elif warns:
        problems.append("- the wasm build warns (fix every warning):\n" + tail(w.stdout + w.stderr))
    pkg = package(modid)
    if w.returncode == 0 and pkg and status in ("done", "partial"):
        cmd = [str(checker()), str(wasm_path(modid)), str(pkg), "--types", kind]
        if VANILLA.exists():
            cmd += ["--vanilla", str(VANILLA)]
        c = run(cmd, check=False)
        if c.returncode != 0:
            errors = [l for l in c.stdout.splitlines() if l.startswith("error")]
            problems.append("- torch-port-check (the plugin run over every block state; names checked "
                            "against the mod's assets) found:\n" + "\n".join(errors[:20] or [tail(c.stdout + c.stderr)]))
    return problems, status


def install(modid, wasm, ported):
    """Swaps plugin.wasm and PORTED.md into the mod's packages (a dev
    shortcut; torch-convert --ports does it from scratch)."""
    places = [CONVERTED] + ([Path(CLIENT_MODS)] if CLIENT_MODS else [])
    swap = {"plugin.wasm": Path(wasm).read_bytes(), "PORTED.md": Path(ported).read_bytes()}
    for place in places:
        for pkg in place.glob(f"{modid}-*.torchmod"):
            tmp = pkg.with_suffix(".tmp")
            with zipfile.ZipFile(pkg) as z, zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as o:
                for info in z.infolist():
                    if info.filename not in swap:
                        o.writestr(info, z.read(info.filename))
                for name, data in swap.items():
                    o.writestr(name, data)
            tmp.replace(pkg)
            log(f"installed into {pkg}")


# --- Main -------------------------------------------------------------------------


def attempts_file(modid):
    return WORK / modid / "attempts.json"


def load_attempts(modid):
    f = attempts_file(modid)
    return json.loads(f.read_text(encoding="utf-8")) if f.is_file() else {}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("modid")
    ap.add_argument("--jar", help="the mod jar (default: the one in tools/moddump/Mods declaring modid)")
    ap.add_argument("--targets", help="comma-separated block entity type ids")
    ap.add_argument("--batch", type=int, default=3, help="renderers this run (one session each)")
    ap.add_argument("--all", action="store_true", help="every renderer left")
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--minutes", type=int, default=40, help="wall-clock budget per session")
    ap.add_argument("--max-tool-calls", type=int, default=150, help="per session")
    ap.add_argument("--fixes", type=int, default=2, help="fix sessions per renderer when a gate fails")
    ap.add_argument("--max-attempts", type=int, default=2, help="runs a renderer may fail before it's skipped")
    ap.add_argument("--retry", action="store_true", help="try renderers that failed before too")
    ap.add_argument("--inventory", action="store_true", help="list the renderers the Java registers, then stop")
    ap.add_argument("--apply", action="store_true", help="apply the passing renderers to your tree")
    ap.add_argument("--install", action="store_true", help="put the built plugin into the packages")
    ap.add_argument("--keep", action="store_true", help="keep the worktree afterwards")
    ap.add_argument("--dry-run", action="store_true", help="print the first prompt and stop")
    ap.add_argument("--events", action="store_true", help="progress lines (@@json) for a front end")
    a = ap.parse_args()
    modid = a.modid
    global EVENTS
    EVENTS = a.events

    (WORK / "logs").mkdir(parents=True, exist_ok=True)
    jar = find_jar(modid, a.jar)
    event("stage", id="source", label=f"{modid}: decompiling and listing renderers", total=2)
    java = Java(decompiled(modid, jar))
    event("step", stage="source", label="Decompiled", detail=f"{jar.name}: {len(java.files)} classes", done=1, total=2)
    found = java.inventory(modid)
    event("step", stage="source", label="Renderers", detail=f"{len(found)} registered", done=2, total=2)
    event("done", stage="source", summary=f"{len(found)} renderer(s) in {jar.name}")
    if a.inventory:
        for k, cls in found.items():
            print(f"{k}\t{cls}")
        log(f"{len(found)} renderer(s) registered in {jar.name}")
        return 0
    ported = ROOT / "ports" / modid / "PORTED.md"
    fresh = not ported.is_file()
    if fresh and not found:
        raise Fail(f"{jar.name} registers no block entity renderers: nothing to port")
    attempts = load_attempts(modid)
    skip = set() if a.retry else {k for k, v in attempts.items() if v.get("failures", 0) >= a.max_attempts}

    ctx = None if a.dry_run else ensure_ollama(a.model)
    if ctx:
        event("step", stage="source", label="Model", detail=f"{a.model}, {ctx // 1024}k context", done=2, total=2)
    name = f"{modid}-{time.strftime('%Y%m%d-%H%M%S')}"
    base = snapshot()
    tree = WORK / "worktrees" / name
    run(["git", "worktree", "add", "--detach", str(tree), base], cwd=ROOT)
    logfile = WORK / "logs" / f"{name}.jsonl"
    outcomes = []
    try:
        port = tree / "ports" / modid
        if fresh:
            bootstrap(tree, modid, jar, found)
            log(f"new port ports/{modid}/ with {len(found)} renderer(s) to do")
        else:
            added = add_rows(port / "PORTED.md", found)
            if added:
                log(f"PORTED.md: {added} renderer(s) the Java registers added as not yet")
        good = commit(tree, f"port-agent: {modid} inventory")
        if ctx:
            qwen_settings(tree, a.model, ctx)

        if a.targets:
            known = {r[0]: (r[0], r[1], r[3]) for r in rows(port / "PORTED.md")}
            targets = [known.get(t.strip(), (t.strip(), found.get(t.strip(), "?"), "")) for t in a.targets.split(",")]
        else:
            targets = todo(port / "PORTED.md", skip)
            if not a.all:
                targets = targets[: a.batch]
        if skip:
            log(f"skipping {len(skip)} renderer(s) that failed {a.max_attempts}x before (--retry to try)")
        if not targets:
            log("nothing to port")
            return 0
        blocks = package_blocks(package(modid))
        if a.dry_run:
            t = targets[0]
            print(port_prompt(tree, modid, java, t, context_pack(java, modid, t[1], t[0], blocks)))
            return 0

        event("stage", id="port", label=f"{modid}: porting {len(targets)} renderer(s)", total=len(targets))
        for n, target in enumerate(targets, 1):
            kind = target[0]
            log(f"== {kind} ({target[1]})")
            event("step", stage="port", label=kind, detail=f"session: {target[1]}", done=n - 1, total=len(targets))
            tests_before = count_tests(port)
            pack = context_pack(java, modid, target[1], kind, blocks)
            _, summary = qwen(tree, java, port_prompt(tree, modid, java, target, pack), a, logfile, modid)
            problems, status = gate(tree, modid, good, kind, tests_before)
            fixes = 0
            while problems and fixes < a.fixes:
                fixes += 1
                log(f"{kind}: {len(problems)} problem(s); fix session {fixes}")
                event("step", stage="port", label=kind,
                      detail=f"fix session {fixes}: " + "; ".join(p.splitlines()[0][2:] for p in problems)[:300],
                      done=n - 1, total=len(targets))
                for p in problems:
                    log("  " + p.splitlines()[0])
                _, summary = qwen(tree, java, fix_prompt(modid, java, target, "\n".join(problems)), a, logfile, modid)
                problems, status = gate(tree, modid, good, kind, tests_before)
            record = attempts.setdefault(kind, {"failures": 0})
            if problems:
                record["failures"] += 1
                record["last_problems"] = problems
                git(tree, "reset", "-q", "--hard", good)
                git(tree, "clean", "-q", "-fd", "-e", ".qwen")
                log(f"{kind}: FAILED after {fixes} fix session(s); rolled back")
            else:
                good = commit(tree, f"port-agent: {kind} {status}")
                record["failures"] = 0
                record.pop("last_problems", None)
                log(f"{kind}: {status}, passed the gates")
            record["last_run"] = name
            attempts_file(modid).write_text(json.dumps(attempts, indent=2), encoding="utf-8")
            event("step", stage="port", label=kind,
                  detail=("FAILED: " + "; ".join(p.splitlines()[0][2:] for p in problems)[:300]) if problems
                  else f"{status}, passed the checks", done=n, total=len(targets))
            outcomes.append({"target": kind, "status": "failed" if problems else status,
                             "problems": problems, "agent_summary": summary, "fixes": fixes})

        patch = git(tree, "diff", "--binary", base, good).stdout
        patch_file = WORK / "logs" / f"{name}.patch"
        patch_file.write_text(patch, encoding="utf-8")
        passed = [o for o in outcomes if o["status"] != "failed"]
        report = {"run": name, "model": a.model, "outcomes": outcomes, "patch": str(patch_file),
                  "commits": git(tree, "log", "--oneline", f"{base}..{good}").stdout.splitlines()}
        (WORK / "logs" / f"{name}.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        log(f"{len(passed)}/{len(outcomes)} renderer(s) passed; patch {patch_file}")
        event("done", stage="port", summary=f"{modid}: {len(passed)}/{len(outcomes)} renderer(s) passed")
        for o in outcomes:
            log(f"  {o['target']}: {o['status']}")
        if patch.strip() and a.apply:
            run(["git", "apply", str(patch_file)], cwd=ROOT)
            log("applied to your tree")
        if passed and a.install:
            env = dict(os.environ, CARGO_TARGET_DIR=str(WORK / "target" / modid))
            run(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown", "--quiet"], cwd=port, env=env)
            install(modid, wasm_path(modid), port / "PORTED.md")
        return 0 if passed else 1
    finally:
        if not a.keep:
            run(["git", "worktree", "remove", "--force", str(tree)], cwd=ROOT, check=False)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Fail as e:
        log(f"error: {e}")
        sys.exit(2)
