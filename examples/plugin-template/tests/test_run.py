"""Unit tests for the starter plugin. They call main() with in-memory streams: no process, no
network, no real environment."""

import importlib.machinery
import importlib.util
import io
import json
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True

RUN_PATH = Path(__file__).resolve().parent.parent / "run"


def load_run():
    loader = importlib.machinery.SourceFileLoader("hello_run", str(RUN_PATH))
    spec = importlib.util.spec_from_loader("hello_run", loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


run_mod = load_run()


def request(settings=None, **overrides):
    body = {
        "protocol_version": 1,
        "type": "invoke",
        "request_id": "req-1",
        "action": "say-hello",
        "settings": {} if settings is None else settings,
    }
    body.update(overrides)
    return json.dumps(body) + "\n"


def call(text):
    """Runs main() on `text`; returns (exit code, raw stdout, parsed response)."""
    stdout = io.StringIO()
    code = run_mod.main([], io.StringIO(text), stdout, {})
    raw = stdout.getvalue()
    return code, raw, json.loads(raw)


class FramingTests(unittest.TestCase):
    def test_success_is_one_json_line_with_the_request_id(self):
        code, raw, response = call(request({"name": "Ada"}))
        self.assertEqual(code, 0)
        self.assertEqual(raw.count("\n"), 1)
        self.assertTrue(raw.endswith("\n"))
        self.assertEqual(
            response,
            {"protocol_version": 1, "request_id": "req-1", "ok": True, "message": "Hello, Ada!"},
        )

    def test_failures_are_ok_false_with_exit_code_zero(self):
        code, raw, response = call("not json\n")
        self.assertEqual(code, 0)
        self.assertEqual(raw.count("\n"), 1)
        self.assertFalse(response["ok"])
        self.assertEqual(response["request_id"], "unknown")

    def test_the_request_id_is_echoed_on_failure_too(self):
        _, _, response = call(request(action="nope", request_id="abc.1"))
        self.assertEqual((response["ok"], response["request_id"]), (False, "abc.1"))

    def test_a_malformed_request_id_is_not_echoed(self):
        _, _, response = call(request(request_id="bad id!"))
        self.assertEqual(response["request_id"], "unknown")

    def test_wrong_protocol_version_and_type_are_refused(self):
        self.assertFalse(call(request(protocol_version=2))[2]["ok"])
        self.assertFalse(call(request(protocol_version=True))[2]["ok"])
        self.assertFalse(call(request(type="ping"))[2]["ok"])

    def test_a_non_object_request_is_refused(self):
        self.assertFalse(call("[1]\n")[2]["ok"])


class SettingsTests(unittest.TestCase):
    def test_defaults_are_applied(self):
        self.assertEqual(call(request({}))[2]["message"], "Hello, world!")

    def test_shout_uppercases_the_greeting(self):
        self.assertEqual(call(request({"name": "Ada", "shout": True}))[2]["message"], "HELLO, ADA!")

    def test_unknown_settings_are_rejected(self):
        response = call(request({"nme": "Ada"}))[2]
        self.assertFalse(response["ok"])
        self.assertIn("unknown setting", response["error"])

    def test_wrong_types_are_rejected(self):
        for settings in ({"name": 5}, {"name": "  "}, {"shout": 1}, {"shout": "yes"}):
            with self.subTest(settings=settings):
                self.assertFalse(call(request(settings))[2]["ok"])

    def test_limits_and_control_characters_are_rejected(self):
        self.assertTrue(call(request({"name": "x" * 32}))[2]["ok"])
        self.assertFalse(call(request({"name": "x" * 33}))[2]["ok"])
        self.assertFalse(call(request({"name": "a\nb"}))[2]["ok"])

    def test_settings_must_be_an_object(self):
        self.assertFalse(call(request(settings=[1]))[2]["ok"])


class ErrorTextTests(unittest.TestCase):
    def test_an_unexpected_exception_is_answered_without_its_text(self):
        original = run_mod.ACTIONS["say-hello"]

        def boom(_settings):
            raise RuntimeError("secret detail")

        run_mod.ACTIONS["say-hello"] = boom
        try:
            code, raw, response = call(request())
        finally:
            run_mod.ACTIONS["say-hello"] = original
        self.assertEqual(code, 0)
        self.assertFalse(response["ok"])
        self.assertNotIn("secret detail", raw)


if __name__ == "__main__":
    unittest.main()
