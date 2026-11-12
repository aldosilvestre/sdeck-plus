"""Unit tests for the Discord demo extension. No network, no hyprctl, no real config dirs."""

import importlib.machinery
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import urllib.error
import urllib.request
from pathlib import Path

sys.dont_write_bytecode = True

RUN_PATH = Path(__file__).resolve().parent.parent / "run"
TOKEN = "SECRET-TOKEN-xyz.abc123"
MESSAGE_BODY = "private body text 8841"
SIG = "abc123def456_1700000000_42"


def load_run():
    loader = importlib.machinery.SourceFileLoader("discord_run", str(RUN_PATH))
    spec = importlib.util.spec_from_loader("discord_run", loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


run_mod = load_run()


class FakeRunner:
    def __init__(self, result=None, error=None):
        self.calls = []
        self.result = result or subprocess.CompletedProcess([], 0, "ok", "")
        self.error = error

    def __call__(self, argv, **kwargs):
        self.calls.append((argv, kwargs))
        if self.error:
            raise self.error
        return self.result


class FakeResponse:
    def __init__(self, status=200):
        self.status = status
        self.closed = False

    def getcode(self):
        return self.status

    def close(self):
        self.closed = True

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


class FakeOpener:
    def __init__(self, response=None, error=None):
        self.calls = []
        self.response = response or FakeResponse(200)
        self.error = error

    def open(self, request, timeout=None):
        self.calls.append((request, timeout))
        if self.error:
            raise self.error
        return self.response


def http_error(code):
    return urllib.error.HTTPError(
        "https://discord.com/api/v10/x", code, "Whatever " + TOKEN, {}, io.BytesIO(b"body " + TOKEN.encode())
    )


class Harness(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="discord-test-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.config = Path(self.tmp) / "config"
        self.token_dir = self.config / "sdeck-plus" / "plugins" / "discord"
        self.token_dir.mkdir(parents=True)
        self.token_path = self.token_dir / "token"
        self.runtime = Path(self.tmp) / "runtime"
        (self.runtime / "hypr" / SIG).mkdir(parents=True)
        self.environ = {
            "XDG_CONFIG_HOME": str(self.config),
            "HOME": str(Path(self.tmp) / "home"),
            "XDG_RUNTIME_DIR": str(self.runtime),
        }

    def write_token(self, text=TOKEN + "\n", mode=0o600):
        self.token_path.write_text(text)
        os.chmod(self.token_path, mode)

    def invoke(self, action, settings, runner=None, opener=None, request_id="req-1"):
        request = {
            "protocol_version": 1,
            "type": "invoke",
            "request_id": request_id,
            "action": action,
            "settings": settings,
        }
        return self.raw(json.dumps(request) + "\n", runner, opener)

    def raw(self, line, runner=None, opener=None):
        stdout, stderr = io.StringIO(), io.StringIO()
        code = run_mod.main(
            [],
            io.StringIO(line),
            stdout,
            self.environ,
            run=runner or FakeRunner(),
            opener=opener or FakeOpener(),
            stderr=stderr,
        )
        self.assertEqual(code, 0)
        self.last_stderr = stderr.getvalue()
        out = stdout.getvalue()
        self.assertTrue(out.endswith("\n"))
        self.assertEqual(out.count("\n"), 1, out)
        self.last_stdout = out
        return json.loads(out)

    def assert_no_leak(self):
        for text in (self.last_stdout, self.last_stderr):
            self.assertNotIn(TOKEN, text)
            self.assertNotIn(MESSAGE_BODY, text)
            self.assertNotIn("Bot ", text)


class ShortcutTests(Harness):
    def test_mute_defaults_use_exact_hyprctl_argv(self):
        runner = FakeRunner()
        reply = self.invoke("toggle-mute", {}, runner=runner)
        self.assertTrue(reply["ok"])
        argv, kwargs = runner.calls[0]
        self.assertEqual(
            argv,
            ["hyprctl", "dispatch", "sendshortcut", "CTRL SHIFT,M,class:^(discord|vesktop|legcord)$"],
        )
        self.assertEqual(kwargs["timeout"], 5)
        self.assertNotIn("shell", kwargs)

    def test_deafen_defaults_use_key_d(self):
        runner = FakeRunner()
        self.invoke("toggle-deafen", {}, runner=runner)
        self.assertEqual(runner.calls[0][0][3], "CTRL SHIFT,D,class:^(discord|vesktop|legcord)$")

    def test_custom_shortcut_is_used(self):
        runner = FakeRunner()
        settings = {"mods": "ALT SUPER", "key": "F13", "window": "title:^Discord.*$"}
        self.invoke("toggle-mute", settings, runner=runner)
        self.assertEqual(runner.calls[0][0][3], "ALT SUPER,F13,title:^Discord.*$")

    def test_named_keys_are_allowed(self):
        for key in ("F1", "F24", "SPACE", "RETURN", "ESCAPE", "TAB", "a", "7"):
            runner = FakeRunner()
            self.assertTrue(self.invoke("toggle-mute", {"key": key}, runner=runner)["ok"], key)
            self.assertEqual(len(runner.calls), 1)

    def test_success_message_only_claims_the_shortcut_was_sent(self):
        reply = self.invoke("toggle-deafen", {})
        self.assertIn("sent", reply["message"].lower())
        self.assertNotIn("confirm", reply["message"].lower().replace("not confirmed", ""))
        self.assertLessEqual(len(reply["message"]), 512)

    def test_invalid_values_are_rejected_before_any_call(self):
        bad = [
            {"mods": "CTRL  SHIFT"},
            {"mods": " CTRL"},
            {"mods": "CTRL,SHIFT"},
            {"mods": "META"},
            {"mods": "ctrl"},
            {"mods": "CTRL;ls"},
            {"mods": ""},
            {"key": "MM"},
            {"key": ""},
            {"key": ","},
            {"key": "F25"},
            {"key": "F0"},
            {"key": "ENTER"},
            {"key": "M,class:x"},
            {"window": "class:a,b"},
            {"window": "class:a\nb"},
            {"window": "class:a\rb"},
            {"window": "address:0x1"},
            {"window": "class:"},
            {"window": "discord"},
            {"window": "class:" + "a" * 201},
            {"window": ""},
        ]
        for settings in bad:
            for action in ("toggle-mute", "toggle-deafen"):
                runner, opener = FakeRunner(), FakeOpener()
                reply = self.invoke(action, settings, runner=runner, opener=opener)
                self.assertFalse(reply["ok"], settings)
                self.assertEqual(runner.calls, [], settings)
                self.assertEqual(opener.calls, [], settings)
                self.assertLessEqual(len(reply["error"]), 512)

    def test_non_string_settings_are_rejected(self):
        runner = FakeRunner()
        self.assertFalse(self.invoke("toggle-mute", {"key": 5}, runner=runner)["ok"])
        self.assertEqual(runner.calls, [])

    def test_hyprctl_missing_is_reported(self):
        reply = self.invoke("toggle-mute", {}, runner=FakeRunner(error=FileNotFoundError("hyprctl")))
        self.assertFalse(reply["ok"])
        self.assertIn("hyprctl", reply["error"])

    def test_hyprctl_timeout_is_reported(self):
        error = subprocess.TimeoutExpired(["hyprctl"], 5)
        reply = self.invoke("toggle-mute", {}, runner=FakeRunner(error=error))
        self.assertFalse(reply["ok"])
        self.assertIn("timed out", reply["error"])

    def test_hyprctl_nonzero_is_reported(self):
        result = subprocess.CompletedProcess([], 1, "", "Couldn't connect")
        reply = self.invoke("toggle-mute", {}, runner=FakeRunner(result=result))
        self.assertFalse(reply["ok"])
        self.assertIn("hyprctl", reply["error"])

    def test_hyprctl_error_text_with_zero_exit_is_reported(self):
        result = subprocess.CompletedProcess([], 0, "Invalid dispatcher", "")
        reply = self.invoke("toggle-mute", {}, runner=FakeRunner(result=result))
        self.assertFalse(reply["ok"])
        self.assertLessEqual(len(reply["error"]), 512)


class MessageTests(Harness):
    settings = {"channel_id": "123456789012345678", "message": MESSAGE_BODY}

    def test_sends_exact_request(self):
        self.write_token()
        opener = FakeOpener()
        runner = FakeRunner()
        reply = self.invoke("send-message", dict(self.settings, message="  " + MESSAGE_BODY + " "), runner, opener)
        self.assertTrue(reply["ok"])
        self.assertEqual(runner.calls, [])
        (request, timeout), = opener.calls
        self.assertEqual(timeout, 5)
        self.assertEqual(request.full_url, "https://discord.com/api/v10/channels/123456789012345678/messages")
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(request.get_header("Authorization"), "Bot " + TOKEN)
        self.assertEqual(request.get_header("Content-type"), "application/json")
        self.assertEqual(request.get_header("User-agent"), "sdeck-plus-discord/1")
        self.assertEqual(json.loads(request.data), {"content": MESSAGE_BODY})
        self.assert_no_leak()

    def test_success_response_is_closed(self):
        self.write_token()
        response = FakeResponse(200)
        self.invoke("send-message", self.settings, opener=FakeOpener(response=response))
        self.assertTrue(response.closed)

    def test_invalid_settings_rejected_before_token_or_network(self):
        bad = [
            {"channel_id": "12345678901234", "message": "hi"},
            {"channel_id": "1" * 26, "message": "hi"},
            {"channel_id": "12345678901234a", "message": "hi"},
            {"channel_id": "../../x/12345678", "message": "hi"},
            {"channel_id": "１２３４５６７８９０１２３４５", "message": "hi"},
            {"channel_id": "123456789012345\n", "message": "hi"},
            {"channel_id": "", "message": "hi"},
            {"channel_id": "123456789012345", "message": "   "},
            {"channel_id": "123456789012345", "message": ""},
            {"channel_id": "123456789012345", "message": "x" * 1025},
            {"channel_id": "123456789012345", "message": "a\nb"},
            {"channel_id": "123456789012345"},
            {"message": "hi"},
            {"channel_id": 123456789012345, "message": "hi"},
        ]
        self.write_token()
        for settings in bad:
            opener, runner = FakeOpener(), FakeRunner()
            reply = self.invoke("send-message", settings, runner, opener)
            self.assertFalse(reply["ok"], settings)
            self.assertEqual(opener.calls, [], settings)
            self.assertEqual(runner.calls, [], settings)

    def test_message_at_limits_is_accepted(self):
        self.write_token()
        for text in ("x", "x" * 1024):
            opener = FakeOpener()
            self.assertTrue(self.invoke("send-message", dict(self.settings, message=text), opener=opener)["ok"])
            self.assertEqual(len(opener.calls), 1)

    def test_missing_token_file_is_actionable_without_network(self):
        opener = FakeOpener()
        reply = self.invoke("send-message", self.settings, opener=opener)
        self.assertFalse(reply["ok"])
        self.assertIn(str(self.token_path), reply["error"])
        self.assertEqual(opener.calls, [])
        self.assert_no_leak()

    def test_symlinked_token_is_refused(self):
        real = Path(self.tmp) / "real-token"
        real.write_text(TOKEN)
        os.chmod(real, 0o600)
        os.symlink(real, self.token_path)
        opener = FakeOpener()
        reply = self.invoke("send-message", self.settings, opener=opener)
        self.assertFalse(reply["ok"])
        self.assertEqual(opener.calls, [])
        self.assert_no_leak()

    def test_group_or_other_permissions_are_refused(self):
        for mode in (0o640, 0o604, 0o660, 0o644, 0o602):
            self.write_token(mode=mode)
            opener = FakeOpener()
            reply = self.invoke("send-message", self.settings, opener=opener)
            self.assertFalse(reply["ok"], oct(mode))
            self.assertIn("0600", reply["error"])
            self.assertEqual(opener.calls, [], oct(mode))
            self.assert_no_leak()

    def test_owner_only_modes_are_accepted(self):
        for mode in (0o600, 0o400):
            self.write_token(mode=mode)
            self.assertTrue(self.invoke("send-message", self.settings)["ok"], oct(mode))

    def test_oversized_token_file_is_refused(self):
        self.write_token(text=TOKEN * 40)
        opener = FakeOpener()
        reply = self.invoke("send-message", self.settings, opener=opener)
        self.assertFalse(reply["ok"])
        self.assertEqual(opener.calls, [])
        self.assert_no_leak()

    def test_token_exactly_512_bytes_is_accepted(self):
        self.write_token(text="a" * 512)
        self.assertTrue(self.invoke("send-message", self.settings)["ok"])

    def test_empty_or_whitespace_token_is_refused(self):
        for text in ("", "  \n", "two words", "tab\ttoken"):
            self.write_token(text=text)
            opener = FakeOpener()
            reply = self.invoke("send-message", self.settings, opener=opener)
            self.assertFalse(reply["ok"], repr(text))
            self.assertEqual(opener.calls, [], repr(text))
            self.assertNotIn("two words", self.last_stdout + self.last_stderr)

    def test_directory_token_is_refused(self):
        os.mkdir(self.token_path)
        opener = FakeOpener()
        self.assertFalse(self.invoke("send-message", self.settings, opener=opener)["ok"])
        self.assertEqual(opener.calls, [])

    def test_wrong_owner_is_refused(self):
        self.write_token()
        real_uid = os.getuid
        original = run_mod.os.getuid
        run_mod.os.getuid = lambda: real_uid() + 1
        try:
            opener = FakeOpener()
            reply = self.invoke("send-message", self.settings, opener=opener)
        finally:
            run_mod.os.getuid = original
        self.assertFalse(reply["ok"])
        self.assertEqual(opener.calls, [])
        self.assert_no_leak()

    def test_home_fallback_is_used_without_xdg_config_home(self):
        home = Path(self.tmp) / "home"
        target = home / ".config" / "sdeck-plus" / "plugins" / "discord"
        target.mkdir(parents=True)
        (target / "token").write_text(TOKEN)
        os.chmod(target / "token", 0o600)
        self.environ = {"HOME": str(home)}
        opener = FakeOpener()
        self.assertTrue(self.invoke("send-message", self.settings, opener=opener)["ok"])
        self.assertEqual(opener.calls[0][0].get_header("Authorization"), "Bot " + TOKEN)

    def test_relative_xdg_config_home_is_ignored(self):
        self.environ = {"XDG_CONFIG_HOME": "relative", "HOME": str(Path(self.tmp) / "nohome")}
        reply = self.invoke("send-message", self.settings)
        self.assertFalse(reply["ok"])
        self.assertIn("nohome", reply["error"])

    def test_http_errors_map_to_distinct_short_messages(self):
        self.write_token()
        seen = {}
        for code in (401, 403, 404, 429, 500, 302):
            reply = self.invoke("send-message", self.settings, opener=FakeOpener(error=http_error(code)))
            self.assertFalse(reply["ok"], code)
            self.assertIn(str(code), reply["error"])
            self.assertLessEqual(len(reply["error"]), 512)
            seen[code] = reply["error"]
            self.assert_no_leak()
        self.assertEqual(len(set(seen.values())), len(seen))

    def test_http_error_response_is_closed(self):
        self.write_token()
        error = http_error(404)
        self.invoke("send-message", self.settings, opener=FakeOpener(error=error))
        self.assertTrue(error.fp.closed)

    def test_network_failures_are_reported_without_details_leak(self):
        self.write_token()
        for error in (urllib.error.URLError("dns " + TOKEN), TimeoutError(TOKEN), OSError(TOKEN)):
            reply = self.invoke("send-message", self.settings, opener=FakeOpener(error=error))
            self.assertFalse(reply["ok"])
            self.assert_no_leak()

    def test_no_retry_on_failure(self):
        self.write_token()
        opener = FakeOpener(error=http_error(429))
        self.invoke("send-message", self.settings, opener=opener)
        self.assertEqual(len(opener.calls), 1)

    def test_redirects_are_refused(self):
        handler = run_mod.NoRedirect()
        request = urllib.request.Request("https://discord.com/api/v10/x")
        result = handler.redirect_request(request, io.BytesIO(b""), 302, "Found", {}, "https://evil.example/")
        self.assertIsNone(result)

    def test_default_opener_uses_the_no_redirect_handler(self):
        opener = run_mod.build_opener()
        self.assertTrue(any(isinstance(h, run_mod.NoRedirect) for h in opener.handlers))

    def test_unexpected_exception_does_not_leak(self):
        self.write_token()

        class Boom:
            def open(self, request, timeout=None):
                raise RuntimeError(TOKEN + MESSAGE_BODY)

        reply = self.invoke("send-message", self.settings, opener=Boom())
        self.assertFalse(reply["ok"])
        self.assert_no_leak()


class InstanceTests(Harness):
    def hypr(self):
        return self.runtime / "hypr"

    def run_mute(self, runner=None):
        runner = runner or FakeRunner()
        return self.invoke("toggle-mute", {}, runner=runner), runner

    def test_signature_is_derived_into_child_env_only(self):
        self.environ = {"XDG_RUNTIME_DIR": str(self.runtime), "HOME": "/nonexistent"}
        reply, runner = self.run_mute()
        self.assertTrue(reply["ok"])
        argv, kwargs = runner.calls[0]
        self.assertEqual(kwargs["env"]["HYPRLAND_INSTANCE_SIGNATURE"], SIG)
        self.assertEqual(kwargs["env"]["XDG_RUNTIME_DIR"], str(self.runtime))
        self.assertEqual(
            argv,
            ["hyprctl", "dispatch", "sendshortcut", "CTRL SHIFT,M,class:^(discord|vesktop|legcord)$"],
        )
        self.assertNotIn("HYPRLAND_INSTANCE_SIGNATURE", self.environ)
        self.assertFalse(any(SIG in part for part in argv))

    def test_newest_instance_wins(self):
        old, new = self.hypr() / "old_1_1", self.hypr() / "new_2_2"
        old.mkdir()
        new.mkdir()
        os.utime(self.hypr() / SIG, (1000, 1000))
        os.utime(old, (2000, 2000))
        os.utime(new, (3000, 3000))
        reply, runner = self.run_mute()
        self.assertEqual(runner.calls[0][1]["env"]["HYPRLAND_INSTANCE_SIGNATURE"], "new_2_2")

    def assert_refused(self, expected=None):
        reply, runner = self.run_mute()
        self.assertFalse(reply["ok"])
        self.assertIn("no running Hyprland instance found", reply["error"])
        self.assertEqual(runner.calls, [])
        self.assert_no_leak()

    def test_no_instance_is_refused(self):
        os.rmdir(self.hypr() / SIG)
        self.assert_refused()

    def test_missing_hypr_dir_is_refused(self):
        os.rmdir(self.hypr() / SIG)
        os.rmdir(self.hypr())
        self.assert_refused()

    def test_unset_or_relative_runtime_dir_is_refused(self):
        del self.environ["XDG_RUNTIME_DIR"]
        self.assert_refused()
        self.environ["XDG_RUNTIME_DIR"] = "relative/dir"
        self.assert_refused()

    def test_symlinks_files_and_odd_names_are_ignored(self):
        os.rmdir(self.hypr() / SIG)
        elsewhere = Path(self.tmp) / "elsewhere"
        elsewhere.mkdir()
        os.symlink(elsewhere, self.hypr() / "linked_1_1")
        (self.hypr() / "afile_1_1").write_text("x")
        for name in ("bad-name", "has space", "dot.dot", "x" * 129):
            (self.hypr() / name).mkdir()
        self.assert_refused()

    def test_valid_preset_signature_is_kept(self):
        self.environ["HYPRLAND_INSTANCE_SIGNATURE"] = "preset_9_9"
        del self.environ["XDG_RUNTIME_DIR"]
        reply, runner = self.run_mute()
        self.assertTrue(reply["ok"])
        self.assertEqual(runner.calls[0][1]["env"]["HYPRLAND_INSTANCE_SIGNATURE"], "preset_9_9")

    def test_invalid_or_empty_preset_signature_is_rederived(self):
        for bad in ("", "../evil", "a b", "x" * 129, "a,b"):
            self.environ["HYPRLAND_INSTANCE_SIGNATURE"] = bad
            reply, runner = self.run_mute()
            self.assertTrue(reply["ok"], bad)
            self.assertEqual(runner.calls[0][1]["env"]["HYPRLAND_INSTANCE_SIGNATURE"], SIG)

    def test_invalid_preset_without_instances_is_refused(self):
        self.environ["HYPRLAND_INSTANCE_SIGNATURE"] = "../evil"
        os.rmdir(self.hypr() / SIG)
        self.assert_refused()

    def test_signature_never_in_output_on_success_or_failure(self):
        reply, _ = self.run_mute(FakeRunner(error=FileNotFoundError("hyprctl")))
        self.assertNotIn(SIG, self.last_stdout + self.last_stderr)
        reply, _ = self.run_mute()
        self.assertNotIn(SIG, self.last_stdout + self.last_stderr)

    def test_listing_failure_is_refused(self):
        os.chmod(self.hypr(), 0)
        self.addCleanup(os.chmod, self.hypr(), 0o700)
        if os.access(self.hypr(), os.R_OK):
            self.skipTest("permissions not enforced")
        self.assert_refused()

    def test_real_executable_with_host_style_environment(self):
        bin_dir = Path(self.tmp) / "bin"
        bin_dir.mkdir()
        log = Path(self.tmp) / "argv.log"
        fake = bin_dir / "hyprctl"
        fake.write_text(
            "#!/bin/sh\n"
            f"printf '%s\\n' \"$@\" > '{log}'\n"
            '[ -n "$HYPRLAND_INSTANCE_SIGNATURE" ] || exit 1\n'
            "echo ok\n"
        )
        os.chmod(fake, 0o755)
        os.symlink(sys.executable, bin_dir / "python3")
        env = {
            "PATH": str(bin_dir),
            "HOME": str(Path(self.tmp) / "home"),
            "LANG": "C",
            "XDG_RUNTIME_DIR": str(self.runtime),
            "XDG_CONFIG_HOME": str(self.config),
            "XDG_DATA_HOME": str(Path(self.tmp) / "data"),
            "WAYLAND_DISPLAY": "wayland-test",
            "DBUS_SESSION_BUS_ADDRESS": "unix:path=/nonexistent",
        }
        request = {"protocol_version": 1, "type": "invoke", "request_id": "r9", "action": "toggle-mute", "settings": {}}
        proc = subprocess.run(
            [str(RUN_PATH)], input=json.dumps(request) + "\n", capture_output=True, text=True, timeout=20, env=env
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertTrue(json.loads(proc.stdout)["ok"], proc.stdout)
        self.assertEqual(
            log.read_text().splitlines(),
            ["dispatch", "sendshortcut", "CTRL SHIFT,M,class:^(discord|vesktop|legcord)$"],
        )
        self.assertNotIn(SIG, proc.stdout + proc.stderr)


class AdvisoryTests(Harness):
    def line(self, **changes):
        request = {"protocol_version": 1, "type": "invoke", "request_id": "r", "action": "toggle-mute", "settings": {}}
        request.update(changes)
        return json.dumps(request) + "\n"

    def test_protocol_version_must_be_the_integer_one(self):
        for value in (True, 1.0, "1", None, 2):
            runner = FakeRunner()
            reply = self.raw(self.line(protocol_version=value), runner=runner)
            self.assertFalse(reply["ok"], value)
            self.assertEqual(runner.calls, [], value)

    def test_non_string_action_is_unknown_action(self):
        for value in ([], {}, 5, None, ["toggle-mute"]):
            reply = self.raw(self.line(action=value))
            self.assertFalse(reply["ok"], value)
            self.assertEqual(reply["error"], "unknown action")
            self.assertEqual(self.last_stderr, "")


class FramingTests(Harness):
    def test_response_echoes_request_id(self):
        reply = self.invoke("toggle-mute", {}, request_id="abc.DEF_1-2")
        self.assertEqual(reply["request_id"], "abc.DEF_1-2")
        self.assertEqual(reply["protocol_version"], 1)

    def test_bad_json_is_ok_false(self):
        reply = self.raw("not json\n")
        self.assertFalse(reply["ok"])
        self.assertEqual(reply["protocol_version"], 1)

    def test_empty_stdin_is_ok_false(self):
        self.assertFalse(self.raw("")["ok"])

    def test_non_object_request_is_ok_false(self):
        self.assertFalse(self.raw("[1]\n")["ok"])

    def test_unsupported_protocol_version_is_ok_false(self):
        line = json.dumps({"protocol_version": 2, "type": "invoke", "request_id": "r", "action": "toggle-mute", "settings": {}})
        runner = FakeRunner()
        reply = self.raw(line + "\n", runner=runner)
        self.assertFalse(reply["ok"])
        self.assertEqual(reply["request_id"], "r")
        self.assertEqual(runner.calls, [])

    def test_unknown_action_is_ok_false(self):
        reply = self.invoke("explode", {})
        self.assertFalse(reply["ok"])
        self.assertEqual(reply["request_id"], "req-1")

    def test_unknown_type_is_ok_false(self):
        line = json.dumps({"protocol_version": 1, "type": "ping", "request_id": "r", "action": "toggle-mute", "settings": {}})
        runner = FakeRunner()
        self.assertFalse(self.raw(line + "\n", runner=runner)["ok"])
        self.assertEqual(runner.calls, [])

    def test_unknown_setting_is_ok_false(self):
        runner = FakeRunner()
        reply = self.invoke("toggle-mute", {"token": TOKEN}, runner=runner)
        self.assertFalse(reply["ok"])
        self.assertEqual(runner.calls, [])
        self.assert_no_leak()

    def test_messages_never_exceed_512_chars(self):
        reply = self.invoke("explode" * 200, {})
        self.assertLessEqual(len(reply["error"]), 512)

    def test_main_end_to_end_as_a_subprocess_for_bad_input(self):
        proc = subprocess.run(
            [sys.executable, "-I", str(RUN_PATH)],
            input="garbage\n",
            capture_output=True,
            text=True,
            timeout=20,
            env={"PYTHONDONTWRITEBYTECODE": "1"},
        )
        self.assertEqual(proc.returncode, 0)
        self.assertEqual(proc.stdout.count("\n"), 1)
        self.assertFalse(json.loads(proc.stdout)["ok"])


if __name__ == "__main__":
    unittest.main()
