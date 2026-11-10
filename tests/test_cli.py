"""Exercise the real installer, HTTP requests, installed tools and filesystem changes."""

import hashlib
import http.server
import io
import os
import platform
import subprocess
import tarfile
import tempfile
import threading
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WINDOWS = os.name == "nt"
EXE = ".exe" if WINDOWS else ""
DRIVER = "sco.exe" if WINDOWS else "scoop"
HOST = (
    "x86_64-pc-windows-gnullvm"
    if WINDOWS
    else "aarch64-apple-darwin"
    if platform.system() == "Darwin" and platform.machine() == "arm64"
    else "x86_64-apple-darwin"
    if platform.system() == "Darwin"
    else "x86_64-unknown-linux-gnu"
)
DEFAULT = "x86_64-pc-windows-msvc-ucrt" if WINDOWS else HOST
ALTERNATE = (
    "x86_64-w64-windows-llvm-mingw-ucrt"
    if WINDOWS
    else "x86_64-apple-darwin"
    if "darwin" in HOST
    else "x86_64-unknown-linux-musl"
)
MINGW = "x86_64-w64-windows-llvm-mingw-msvcrt"


class InstallerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = Path(
            os.environ.get("SCOOP_INSTALL_BINARY", ROOT / "target/debug" / ("scoop-install" + EXE))
        ).resolve()
        cls.support = tempfile.TemporaryDirectory(prefix="cli-support-", dir=ROOT / "tmp")
        cls.tool = Path(cls.support.name) / ("tool" + EXE)
        subprocess.run(
            [
                "rustc",
                "--edition",
                "2024",
                "-O",
                str(ROOT / "tests/tool.rs"),
                "-o",
                str(cls.tool),
            ],
            check=True,
        )

    @classmethod
    def tearDownClass(cls):
        cls.support.cleanup()

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(
            prefix="installation with spaces-", dir=ROOT / "tmp"
        )
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / "scoop home"
        self.site = self.root / "site"
        (self.site / "versions").mkdir(parents=True)
        self.requests = []
        owner = self

        class Handler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=owner.site, **kwargs)

            def do_GET(self):
                owner.requests.append(self.path)
                super().do_GET()

            def log_message(self, *args):
                pass

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.environment = {
            **os.environ,
            "SCOOP_HOME": str(self.home),
            "SCOOP_DIST_SERVER": f"http://127.0.0.1:{server.server_port}",
        }
        self.environment.pop("SCOOP_TOOLCHAIN", None)
        self.environment.pop("SCOOP_INSTALL_HOST", None)
        self.release("1.0.0")
        self.release("2.0.0")

    def cli(self, *args, success=True, environment=None, cwd=None, input=None):
        result = subprocess.run(
            [str(self.binary), *args],
            env=environment or self.environment,
            cwd=cwd or self.root,
            input=input,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def archive(self, version, component, files):
        path = self.site / version / f"{component}.tar.xz"
        path.parent.mkdir(exist_ok=True)
        with tarfile.open(path, "w:xz") as archive:
            for name, content in files.items():
                entry = tarfile.TarInfo("./" + name)
                entry.mode = 0o755 if name.startswith("bin/") else 0o644
                if isinstance(content, tuple):
                    entry.type, entry.linkname = content
                    archive.addfile(entry)
                else:
                    entry.size = len(content)
                    archive.addfile(entry, io.BytesIO(content))
        return {
            "path": f"../{version}/{component}.tar.xz",
            "size": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }

    def release(self, version, *, gui=True):
        tool = self.tool.read_bytes()
        files = {
            "VERSION": (version + "\n").encode(),
            f"bin/{DRIVER}": tool,
            "bin/scoopc" + EXE: tool,
            "bin/scoop-lsp" + EXE: tool,
        }
        files["bin/scoop-clang" + EXE] = (
            (tarfile.LNKTYPE, "./bin/scoopc" + EXE) if WINDOWS else (tarfile.SYMTYPE, "scoopc")
        )
        archives = {
            "bin": self.archive(version, "bin", files),
            "sources": self.archive(
                version,
                "sources",
                {
                    "src/scoop.core/Cone.toml": f'[cone]\nname = "scoop.core"\nversion = "{version}"\n'.encode(),
                    "include/scoop_ffi.h": b"public FFI header",
                },
            ),
            "vendor": self.archive(
                version, "vendor", {"vendor/upstream/source.c": b"upstream source"}
            ),
        }
        lines = [
            f'schema = 1\nrelease = "{version}"\nversion = "{version}"\nrevision = "revision-{version}"\nsources = "sources"\nvendor = "vendor"'
        ]
        lines.append(
            f'[hosts."{HOST}"]\ndefault-target = "{DEFAULT}"\ntargets = ["{DEFAULT}", "{ALTERNATE}"]\nbin = "bin"'
        )
        if HOST != "x86_64-pc-windows-gnullvm":
            lines.append(
                f'[hosts.x86_64-pc-windows-gnullvm]\ndefault-target = "{MINGW}"\ntargets = ["{MINGW}"]\nbin = "bin"'
            )
        targets = (
            {DEFAULT, ALTERNATE, MINGW}
            if HOST != "x86_64-pc-windows-gnullvm"
            else {DEFAULT, ALTERNATE}
        )
        for target in targets:
            base = "base-" + target
            archives[base] = self.archive(
                version, base, {f"lib/{target}/scoop.core.slib": b"prebuilt base"}
            )
            lines.append(f'[targets."{target}"]\nbase = "{base}"')
            if gui:
                name = "gui-" + target
                archives[name] = self.archive(
                    version, name, {f"lib/{target}/scoop.window.slib": b"prebuilt GUI"}
                )
                lines.append(f'gui = "{name}"')
            if "mingw" in target:
                name = "native-" + target
                archives[name] = self.archive(
                    version, name, {f"lib/{target}/toolchain/crt.o": b"CRT input"}
                )
                lines.append(f'native = "{name}"')
        for component, archive in archives.items():
            lines.append(f'[archives."{component}"]')
            lines.extend(
                f"{key} = {value if isinstance(value, int) else chr(34) + value + chr(34)}"
                for key, value in archive.items()
            )
        (self.site / "versions" / f"{version}.toml").write_text("\n".join(lines) + "\n")
        (self.site / "latest.toml").write_text(
            f'schema = 1\nrelease = "{version}"\nmanifest = "versions/{version}.toml"\n'
        )

    def install(self, version="1.0.0", *args):
        return self.cli("toolchain", "install", version, "--skip-docs", *args)

    def installed(self, version="1.0.0", host=HOST):
        return self.home / "toolchains" / f"{version}-{host}"

    def state(self, version="1.0.0"):
        return tomllib.loads((self.installed(version) / ".scoop-install/state.toml").read_text())

    def default(self):
        return tomllib.loads((self.home / "settings.toml").read_text())["default"]

    def proxy(self, *args, environment=None, cwd=None):
        return subprocess.run(
            [str(self.home / "bin" / DRIVER), *args],
            env=environment or self.environment,
            cwd=cwd or self.root,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )

    def test_explicit_target_replaces_default_and_reuses_installed_components(self):
        self.install("1.0.0", "--target", ALTERNATE)
        self.assertFalse((self.installed() / "lib" / DEFAULT).exists())
        self.assertTrue((self.installed() / "lib" / ALTERNATE / "scoop.core.slib").is_file())
        self.assertFalse(any("vendor.tar.xz" in path or "gui-" in path for path in self.requests))
        self.assertFalse((self.installed() / "vendor").exists())
        requests = list(self.requests)
        self.install("1.0.0", "--target", ALTERNATE)
        self.assertEqual(self.requests, requests)
        self.assertEqual(self.proxy("--version").stdout.strip(), "1.0.0")
        self.assertEqual(
            self.cli("run", "1.0.0", "scoop-clang", "--version").stdout.strip(), "1.0.0"
        )
        self.assertTrue((self.site / "1.0.0/bin.tar.xz").is_file())

    def test_component_add_remove_retains_unowned_files_and_dependencies(self):
        self.install()
        self.cli(
            "component",
            "add",
            "gui",
            "--target",
            ALTERNATE,
            "--skip-docs",
            success=False,
        )
        self.cli("target", "add", ALTERNATE, "--skip-docs")
        self.cli("component", "add", "gui", "--target", ALTERNATE, "--skip-docs")
        self.cli("component", "add", "vendor", "--skip-docs")
        custom = self.installed() / "lib" / ALTERNATE / "custom.txt"
        custom.write_text("user input")
        self.cli("component", "remove", "sources", success=False)
        self.cli("target", "remove", ALTERNATE)
        self.assertEqual(custom.read_text(), "user input")
        self.assertFalse((custom.parent / "scoop.window.slib").exists())
        self.cli("component", "remove", "vendor")
        self.assertFalse((self.installed() / "vendor/upstream/source.c").exists())
        self.assertEqual(sum(path.endswith("sources.tar.xz") for path in self.requests), 1)
        self.cli("toolchain", "remove", "1.0.0", success=False)

    def test_custom_home_proxies_work_without_an_environment_override(self):
        environment = dict(self.environment)
        environment.pop("SCOOP_HOME")
        environment["USERPROFILE" if WINDOWS else "HOME"] = str(self.root / "user")
        self.cli(
            "--home",
            str(self.home),
            "toolchain",
            "install",
            "1.0.0",
            "--skip-docs",
            environment=environment,
        )
        result = self.proxy("child", environment=environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "1.0.0")
        result = subprocess.run(
            [str(self.home / "bin" / ("scoop-install" + EXE)), "show"],
            env=environment,
            cwd=self.root,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("1.0.0", result.stdout)
        self.assertFalse((self.root / "user" / ".scoop").exists())

    def test_project_environment_override_arguments_child_and_exit_status(self):
        self.install()
        self.install("2.0.0")
        project = self.root / "project"
        nested = project / "nested"
        nested.mkdir(parents=True)
        pin = project / "scoop-toolchain.toml"
        pin.write_text(f'[toolchain]\nversion = "2.0.0"\ntargets = ["{ALTERNATE}"]\n')
        missing = self.proxy("--version", cwd=nested)
        self.assertNotEqual(missing.returncode, 0)
        self.assertIn("target add", missing.stderr)
        self.cli("target", "add", ALTERNATE, "--toolchain", "2.0.0", "--skip-docs")
        self.assertEqual(self.proxy("--version", cwd=nested).stdout.strip(), "2.0.0")
        env = {**self.environment, "SCOOP_TOOLCHAIN": "1.0.0"}
        self.assertEqual(
            self.proxy("--version", environment=env, cwd=nested).stdout.strip(), "1.0.0"
        )
        self.assertEqual(
            self.proxy("+2.0.0", "child", environment=env, cwd=nested).stdout.strip(),
            "2.0.0",
        )
        result = self.proxy("+1.0.0", "hello world", "中文", cwd=nested)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(nested), result.stdout)
        self.assertIn("hello world", result.stdout)
        self.assertEqual(self.proxy("exit").returncode, 37)
        self.assertEqual(
            self.cli("run", "1.0.0", "scoop", "stdin", input="hello\n").stdout,
            "hello\n",
        )

    def test_update_preserves_selection_and_rejects_unavailable_gui_before_downloading(
        self,
    ):
        self.install("1.0.0", "--target", ALTERNATE, "--component", "gui,vendor")
        self.release("2.0.0", gui=False)
        self.cli("update", "--skip-docs", success=False)
        self.assertEqual(self.default(), "1.0.0")
        self.assertFalse(any(path.startswith("/2.0.0/") for path in self.requests))
        self.release("2.0.0")
        self.cli("update", "--skip-docs")
        self.assertEqual(self.default(), "2.0.0")
        components = self.state("2.0.0")["components"]
        self.assertIn("gui-" + ALTERNATE, components)
        self.assertIn("vendor", components)
        self.assertNotIn("base-" + DEFAULT, components)
        self.cli("toolchain", "remove", "1.0.0")
        self.assertFalse(self.installed().exists())

    def test_failed_archive_leaves_old_default_and_no_partial_toolchain(self):
        self.install()
        damaged = self.site / "2.0.0/sources.tar.xz"
        damaged.write_bytes(b"incomplete download")
        self.cli("update", "--skip-docs", success=False)
        self.assertEqual(self.default(), "1.0.0")
        self.assertFalse(self.installed("2.0.0").exists())
        self.assertEqual(self.proxy("--version").stdout.strip(), "1.0.0")
        self.assertEqual(list((self.home / "tmp").iterdir()), [])

    def test_bad_archive_paths_and_component_overlap_do_not_activate(self):
        for name in ("../outside.txt", "VERSION"):
            with self.subTest(name=name):
                archive = self.archive("2.0.0", "sources", {name: b"invalid component file"})
                path = self.site / "versions/2.0.0.toml"
                original = path.read_text()
                manifest = tomllib.loads(original)
                old = manifest["archives"]["sources"]
                updated = original.replace(old["sha256"], archive["sha256"]).replace(
                    f"size = {old['size']}", f"size = {archive['size']}"
                )
                path.write_text(updated)
                self.cli("toolchain", "install", "2.0.0", "--skip-docs", success=False)
                self.assertFalse(self.installed("2.0.0").exists())
                self.assertFalse((self.home / "settings.toml").exists())
                self.assertEqual(list((self.home / "tmp").iterdir()), [])

    def test_concurrent_install_downloads_each_component_once(self):
        command = [str(self.binary), "toolchain", "install", "1.0.0", "--skip-docs"]
        first = subprocess.Popen(
            command,
            env=self.environment,
            cwd=self.root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        second = subprocess.Popen(
            command,
            env=self.environment,
            cwd=self.root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        for process in (first, second):
            output, errors = process.communicate(timeout=30)
            self.assertEqual(process.returncode, 0, output + errors)
        for component in ("bin", "sources", "base-" + DEFAULT):
            self.assertEqual(self.requests.count(f"/1.0.0/{component}.tar.xz"), 1)
        self.assertEqual(self.proxy("--version").stdout.strip(), "1.0.0")

    def test_existing_package_cache_and_custom_commands_are_preserved(self):
        for name in ("packages/cached/Cone.toml", "commands/custom-command", "local-notes.txt"):
            path = self.home / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("user data")
        self.install()
        for name in ("packages/cached/Cone.toml", "commands/custom-command", "local-notes.txt"):
            self.assertEqual((self.home / name).read_text(), "user data")

    @unittest.skipIf(WINDOWS, "covered by the native host's MinGW target test")
    def test_mingw_target_automatically_installs_native(self):
        self.cli(
            "--host",
            "x86_64-pc-windows-gnullvm",
            "toolchain",
            "install",
            "1.0.0",
            "--target",
            MINGW,
            "--skip-docs",
        )
        root = self.installed(host="x86_64-pc-windows-gnullvm")
        self.assertEqual((root / "lib" / MINGW / "toolchain/crt.o").read_bytes(), b"CRT input")
        self.assertTrue(any(("native-" + MINGW) in path for path in self.requests))


if __name__ == "__main__":
    unittest.main()
