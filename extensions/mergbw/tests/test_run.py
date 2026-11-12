"""Unit tests for the MeRGBW demo extension. No busctl, no Bluetooth, no real config dirs."""

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
from pathlib import Path

sys.dont_write_bytecode = True

RUN_PATH = Path(__file__).resolve().parent.parent / "run"
ADDRESS = "AA:BB:CC:DD:EE:FF"
DEV = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"
CHAR = DEV + "/service0010/char0011"
OTHER_CHAR = DEV + "/service0010/char0013"
UUID = "0000fff3-0000-1000-8000-00805f9b34fb"
SECRET = "RAWSECRET-output-77"


def load_run():
    loader = importlib.machinery.SourceFileLoader("mergbw_run", str(RUN_PATH))
    spec = importlib.util.spec_from_loader("mergbw_run", loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


run_mod = load_run()


def objects_json(connected=True, resolved=True, with_char=True, address=ADDRESS,
                 dev=DEV, char=CHAR, uuid=UUID, with_device=True):
    data = {}
    if with_device:
        data[dev] = {
            "org.bluez.Device1": {
                "Address": {"type": "s", "data": address},
                "Connected": {"type": "b", "data": connected},
                "ServicesResolved": {"type": "b", "data": resolved},
            }
        }
    if with_char:
        data[OTHER_CHAR] = {
            "org.bluez.GattCharacteristic1": {"UUID": {"type": "s", "data": "0000fff4-0000-1000-8000-00805f9b34fb"}}
        }
        data[char] = {"org.bluez.GattCharacteristic1": {"UUID": {"type": "s", "data": uuid}}}
    return json.dumps({"type": "a{oa{sa{sv}}}", "data": [data]})


class Clock:
    def __init__(self, step=0.0):
        self.now = 100.0
        self.step = step
        self.sleeps = []

    def monotonic(self):
        self.now += self.step
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += seconds


class FakeBus:
    """Scripted busctl: `reads` are consecutive GetManagedObjects stdouts (last one repeats)."""

    def __init__(self, reads, fail_on=None, raises=None, stderr=""):
        self.reads = list(reads)
        self.calls = []
        self.fail_on = fail_on
        self.raises = raises
        self.stderr = stderr

    def __call__(self, argv, **kwargs):
        self.calls.append((argv, kwargs))
        if self.raises:
            raise self.raises
        if "GetManagedObjects" in argv:
            out = self.reads.pop(0) if len(self.reads) > 1 else self.reads[0]
            return subprocess.CompletedProcess(argv, 0, out, "")
        if self.fail_on and self.fail_on in argv:
            return subprocess.CompletedProcess(argv, 1, "", self.stderr)
        return subprocess.CompletedProcess(argv, 0, "", "")

    def argvs(self):
        return [call[0] for call in self.calls]


def invoke(action, settings, runner, clock=None, request=None, environ=None):
    clock = clock or Clock()
    body = {"protocol_version": 1, "request_id": "req-1", "type": "invoke",
            "action": action, "settings": settings}
    body.update(request or {})
    stdin, stdout, stderr = io.StringIO(json.dumps(body) + "\n"), io.StringIO(), io.StringIO()
    code = run_mod.main([], stdin, stdout, environ or {}, run=runner, stderr=stderr,
                        monotonic=clock.monotonic, sleep=clock.sleep)
    lines = stdout.getvalue().splitlines()
    assert code == 0 and len(lines) == 1, (code, stdout.getvalue())
    return json.loads(lines[0])


def write_argv(data):
    return ["busctl", "--system", "call", "org.bluez", CHAR, "org.bluez.GattCharacteristic1",
            "WriteValue", "aya{sv}", str(len(data)), *[str(b) for b in data], "0"]


GET = ["busctl", "--system", "--json=short", "call", "org.bluez", "/",
       "org.freedesktop.DBus.ObjectManager", "GetManagedObjects"]
CONNECT = ["busctl", "--system", "call", "org.bluez", DEV, "org.bluez.Device1", "Connect"]


class PacketTests(unittest.TestCase):
    # Vectors 1-4 are from the task statement (reference); 5-7 were derived by hand from the
    # documented algorithm, not taken from the reference.
    def test_power_on(self):
        self.assertEqual(run_mod.power_packet(True), [0x55, 0x01, 0xFF, 0x06, 0x01, 0xA2])

    def test_power_off(self):
        self.assertEqual(run_mod.power_packet(False), [0x55, 0x01, 0xFF, 0x06, 0x00, 0xA3])

    def test_red(self):
        self.assertEqual(run_mod.color_packet(0, 1000),
                         [0x55, 0x03, 0xFF, 0x09, 0x00, 0x00, 0x03, 0xE8, 0xB2])

    def test_brightness_max(self):
        self.assertEqual(run_mod.brightness_packet(1000), [0x55, 0x05, 0xFF, 0x07, 0x03, 0xE8, 0xB2])

    def test_derived_green_half_saturation(self):
        # sum 0x2CD -> fold 0xCF -> ~ = 0x30
        self.assertEqual(run_mod.color_packet(120, 500),
                         [0x55, 0x03, 0xFF, 0x09, 0x00, 0x78, 0x01, 0xF4, 0x30])

    def test_derived_scene_pair(self):
        # scene 2: sum 0x163 -> 0x64 -> 0x9B; second packet 0x3200: sum 0x19C -> 0x9D -> 0x62
        self.assertEqual(run_mod.scene_packets(2), [
            [0x55, 0x06, 0xFF, 0x07, 0x00, 0x02, 0x9B],
            [0x55, 0x0F, 0xFF, 0x07, 0x32, 0x00, 0x62],
        ])

    def test_checksum_without_fold(self):
        self.assertEqual(run_mod.checksum([0x01]), 0xFE)

    def test_checksum_single_fold(self):
        self.assertEqual(run_mod.checksum([0xFF, 0x01]), 0xFE)  # 0x100 -> 1

    def test_checksum_double_fold(self):
        self.assertEqual(run_mod.checksum([0xFF, 0xFF, 0x01]), 0xFE)  # 0x1FF -> 0x100 -> 1

    def test_checksum_fold_to_ff(self):
        self.assertEqual(run_mod.checksum([0xFF] * 7), 0x00)  # 0x6F9 -> 0xFF


class ValidationTests(unittest.TestCase):
    def rejected(self, action, settings, needle=None, request=None):
        runner = FakeBus([objects_json()])
        reply = invoke(action, settings, runner, request=request)
        self.assertFalse(reply["ok"], reply)
        self.assertEqual(runner.calls, [], "runner must not be called")
        if needle:
            self.assertIn(needle, reply["error"])

    def test_bad_addresses(self):
        for bad in ["", "AA:BB:CC:DD:EE", "AA-BB-CC-DD-EE-FF", "AABBCCDDEEFF", "GG:BB:CC:DD:EE:FF",
                    " AA:BB:CC:DD:EE:FF", "AA:BB:CC:DD:EE:FF\n", "AA:BB:CC:DD:EE:FF:00", 5, None,
                    "ＡＡ:BB:CC:DD:EE:FF"]:
            with self.subTest(bad=bad):
                self.rejected("power-on", {"address": bad}, "address")

    def test_missing_address(self):
        self.rejected("power-on", {}, "address")

    def test_unknown_setting(self):
        self.rejected("power-on", {"address": ADDRESS, "hue": 3}, "unknown setting")

    def test_color_ranges_and_types(self):
        for key, bad in [("hue", -1), ("hue", 360), ("saturation", 101), ("saturation", -1),
                         ("brightness", 101), ("hue", True), ("hue", 1.0), ("hue", "5"),
                         ("brightness", False), ("saturation", None)]:
            with self.subTest(key=key, bad=bad):
                self.rejected("set-color", {"address": ADDRESS, key: bad}, key)

    def test_brightness_ranges(self):
        for bad in [-1, 101, True, 2.5, "7"]:
            with self.subTest(bad=bad):
                self.rejected("set-brightness", {"address": ADDRESS, "brightness": bad}, "brightness")

    def test_scene_ranges(self):
        for bad in [0, 118, True, 3.0, "2"]:
            with self.subTest(bad=bad):
                self.rejected("set-scene", {"address": ADDRESS, "scene": bad}, "scene")

    def test_settings_not_an_object(self):
        self.rejected("power-on", [], "settings")

    def test_unknown_and_non_string_action(self):
        for action in ["pair", "scan", 5, None, ["power-on"]]:
            with self.subTest(action=action):
                self.rejected(action, {"address": ADDRESS}, "unknown action")

    def test_address_is_normalised_to_upper_case(self):
        runner = FakeBus([objects_json()])
        reply = invoke("power-on", {"address": ADDRESS.lower()}, runner)
        self.assertTrue(reply["ok"], reply)


class SequenceTests(unittest.TestCase):
    def run_ok(self, action, settings, reads=None):
        runner = FakeBus(reads or [objects_json()])
        reply = invoke(action, settings, runner)
        self.assertTrue(reply["ok"], reply)
        return runner, reply

    def test_power_on_argv(self):
        runner, reply = self.run_ok("power-on", {"address": ADDRESS})
        self.assertEqual(runner.argvs(), [GET, write_argv([0x55, 1, 0xFF, 6, 1, 0xA2])])
        self.assertIn("Wrote 1 command packet", reply["message"])

    def test_power_off_argv(self):
        runner, _ = self.run_ok("power-off", {"address": ADDRESS})
        self.assertEqual(runner.argvs()[1], write_argv([0x55, 1, 0xFF, 6, 0, 0xA3]))

    def test_color_orders_power_colour_brightness(self):
        runner, _ = self.run_ok("set-color", {"address": ADDRESS, "hue": 120, "saturation": 50,
                                               "brightness": 100})
        self.assertEqual(runner.argvs(), [
            GET,
            write_argv([0x55, 1, 0xFF, 6, 1, 0xA2]),
            write_argv([0x55, 3, 0xFF, 9, 0, 0x78, 1, 0xF4, 0x30]),
            write_argv([0x55, 5, 0xFF, 7, 3, 0xE8, 0xB2]),
        ])

    def test_color_without_brightness_skips_it(self):
        runner, _ = self.run_ok("set-color", {"address": ADDRESS})
        self.assertEqual(len(runner.argvs()), 3)
        self.assertEqual(runner.argvs()[2], write_argv([0x55, 3, 0xFF, 9, 0, 0, 3, 0xE8, 0xB2]))

    def test_brightness_has_no_power_packet(self):
        runner, _ = self.run_ok("set-brightness", {"address": ADDRESS, "brightness": 100})
        self.assertEqual(runner.argvs(), [GET, write_argv([0x55, 5, 0xFF, 7, 3, 0xE8, 0xB2])])

    def test_scene_is_power_plus_two_writes(self):
        runner, reply = self.run_ok("set-scene", {"address": ADDRESS, "scene": 2})
        self.assertEqual(runner.argvs(), [
            GET,
            write_argv([0x55, 1, 0xFF, 6, 1, 0xA2]),
            write_argv([0x55, 6, 0xFF, 7, 0, 2, 0x9B]),
            write_argv([0x55, 0x0F, 0xFF, 7, 0x32, 0, 0x62]),
        ])
        self.assertIn("3 command packets", reply["message"])

    def test_scene_bounds_accepted(self):
        for scene in (1, 117):
            with self.subTest(scene=scene):
                self.run_ok("set-scene", {"address": ADDRESS, "scene": scene})

    def test_success_message_is_honest(self):
        _, reply = self.run_ok("power-on", {"address": ADDRESS})
        for needle in ("Wrote", "characteristic", "not confirmed", "unverified on a TG609"):
            self.assertIn(needle, reply["message"])

    def test_only_allowed_busctl_shapes(self):
        runner, _ = self.run_ok("set-scene", {"address": ADDRESS, "scene": 9},
                                reads=[objects_json(connected=False, resolved=False),
                                       objects_json()])
        for argv in runner.argvs():
            self.assertEqual(argv[:2], ["busctl", "--system"])
            self.assertTrue("GetManagedObjects" in argv or "Connect" in argv or "WriteValue" in argv)
            for banned in ("Pair", "Trust", "StartDiscovery", "RemoveDevice", "Disconnect",
                           "SetDiscoveryFilter", "StartNotify", "AcquireWrite"):
                self.assertNotIn(banned, argv)
        for argv in runner.argvs():
            self.assertFalse(argv[0] == "busctl" and any(a == "shell" for a in argv))

    def test_connect_then_wait_for_services(self):
        runner, _ = self.run_ok(
            "power-on", {"address": ADDRESS},
            reads=[objects_json(connected=False, resolved=False),
                   objects_json(connected=True, resolved=False),
                   objects_json(connected=True, resolved=True)])
        self.assertEqual(runner.argvs()[:2], [GET, CONNECT])
        self.assertEqual(runner.argvs().count(GET), 3)
        self.assertEqual(runner.argvs()[-1], write_argv([0x55, 1, 0xFF, 6, 1, 0xA2]))

    def test_no_connect_when_already_connected(self):
        runner, _ = self.run_ok("power-on", {"address": ADDRESS})
        self.assertNotIn(CONNECT, runner.argvs())

    def test_runner_flags(self):
        runner, _ = self.run_ok("power-on", {"address": ADDRESS})
        for argv, kwargs in runner.calls:
            self.assertIsInstance(argv, list)
            self.assertLessEqual(kwargs["timeout"], 5)
            self.assertNotIn("shell", kwargs)

    def test_characteristic_uuid_case_insensitive(self):
        self.run_ok("power-on", {"address": ADDRESS}, reads=[objects_json(uuid=UUID.upper())])


class FailureTests(unittest.TestCase):
    def failed(self, runner, needle, action="power-on", clock=None):
        reply = invoke(action, {"address": ADDRESS}, runner, clock=clock)
        self.assertFalse(reply["ok"], reply)
        self.assertIn(needle, reply["error"])
        self.assertLessEqual(len(reply["error"]), 512)
        return reply

    def test_device_absent(self):
        runner = FakeBus([objects_json(with_device=False)])
        self.failed(runner, "device not known to BlueZ; pair it first with bluetoothctl")
        self.assertEqual(runner.argvs(), [GET])

    def test_other_device_address_is_absent(self):
        runner = FakeBus([objects_json(address="11:22:33:44:55:66")])
        self.failed(runner, "not known to BlueZ")
        self.assertEqual(len(runner.calls), 1)

    def test_connect_timeout_polling(self):
        runner = FakeBus([objects_json(connected=True, resolved=False)])
        clock = Clock()
        self.failed(runner, "could not connect in time; close the MeRGBW app and try again",
                    clock=clock)
        self.assertTrue(clock.sleeps)
        self.assertFalse(any("WriteValue" in a for a in runner.argvs()))
        self.assertLessEqual(clock.now - 100.0, 4.6)

    def test_connect_failure_stops(self):
        runner = FakeBus([objects_json(connected=False, resolved=False)], fail_on="Connect")
        self.failed(runner, "connect")
        self.assertEqual(runner.argvs(), [GET, CONNECT])

    def test_connect_call_timeout(self):
        calls = []

        def runner(argv, **kw):
            calls.append(argv)
            if "Connect" in argv:
                raise subprocess.TimeoutExpired(argv, kw["timeout"])
            return subprocess.CompletedProcess(argv, 0, objects_json(connected=False, resolved=False), "")

        self.failed(runner, "could not connect in time")
        self.assertEqual(len(calls), 2)

    def test_device_disappears_while_waiting(self):
        runner = FakeBus([objects_json(connected=True, resolved=False),
                          objects_json(with_device=False)])
        self.failed(runner, "disappeared")

    def test_characteristic_missing(self):
        runner = FakeBus([objects_json(with_char=False)])
        self.failed(runner, "characteristic")
        self.assertEqual(runner.argvs(), [GET])

    def test_characteristic_of_other_device_ignored(self):
        other = "/org/bluez/hci0/dev_11_22_33_44_55_66/service0010/char0011"
        runner = FakeBus([objects_json(char=other)])
        self.failed(runner, "characteristic")

    def test_first_write_failure_stops(self):
        runner = FakeBus([objects_json()], fail_on="WriteValue")
        reply = self.failed(runner, "write packet 1/3", action="set-scene")
        self.assertEqual(len(runner.calls), 2)
        self.assertNotIn("Wrote", reply["error"])

    def test_second_write_failure_names_step(self):
        state = {"n": 0}

        def runner(argv, **kw):
            if "WriteValue" in argv:
                state["n"] += 1
                if state["n"] == 2:
                    return subprocess.CompletedProcess(argv, 1, "", "nope")
            out = objects_json() if "GetManagedObjects" in argv else ""
            return subprocess.CompletedProcess(argv, 0, out, "")

        self.failed(runner, "write packet 2/3", action="set-scene")
        self.assertEqual(state["n"], 2)

    def test_busctl_missing(self):
        self.failed(FakeBus([], raises=FileNotFoundError()), "busctl was not found")

    def test_busctl_timeout(self):
        self.failed(FakeBus([], raises=subprocess.TimeoutExpired(["busctl"], 5)), "timed out")

    def test_malformed_json(self):
        for text in ["", "not json", "{}", '{"data": []}', '{"data": [5]}', '{"data": 1}', "[]"]:
            with self.subTest(text=text):
                runner = FakeBus([text])
                self.failed(runner, "unreadable BlueZ answer")
                self.assertEqual(len(runner.calls), 1)

    def test_huge_output(self):
        runner = FakeBus(["x" * (1024 * 1024 + 1)])
        self.failed(runner, "too large")

    def test_hostile_device_path(self):
        bad = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF/../../../etc"
        runner = FakeBus([objects_json(dev=bad, with_char=False)])
        self.failed(runner, "unexpected object path")
        self.assertEqual(len(runner.calls), 1)

    def test_hostile_device_path_with_option_injection(self):
        runner = FakeBus([objects_json(dev="--user", with_char=False)])
        self.failed(runner, "unexpected object path")

    def test_hostile_characteristic_path(self):
        bad = DEV + "/service0010/char0011; rm -rf"
        runner = FakeBus([objects_json(char=bad)])
        self.failed(runner, "unexpected object path")
        self.assertEqual(len(runner.calls), 1)

    def test_busctl_output_is_not_echoed(self):
        raw = f"first line {SECRET}\n" + "second line SECONDLINE\n" + "z" * 5000
        runner = FakeBus([objects_json()], fail_on="WriteValue", stderr=raw)
        reply = self.failed(runner, "write packet 1/1")
        self.assertNotIn("SECONDLINE", reply["error"])
        self.assertLessEqual(len(reply["error"]), 200)
        garbage = FakeBus([SECRET * 10])
        reply = self.failed(garbage, "unreadable")
        self.assertNotIn(SECRET, reply["error"])

    def test_unsafe_characters_are_sanitised(self):
        runner = FakeBus([objects_json()], fail_on="WriteValue", stderr="bad\x1b[31m\x00text")
        reply = self.failed(runner, "write packet 1/1")
        self.assertTrue(reply["error"].isprintable())

    def test_total_time_bound(self):
        clock = Clock(step=2.0)  # every clock read costs two seconds
        runner = FakeBus([objects_json()])
        reply = invoke("set-scene", {"address": ADDRESS, "scene": 2}, runner, clock=clock)
        self.assertFalse(reply["ok"])
        self.assertIn("out of time", reply["error"])
        for _, kwargs in runner.calls:
            self.assertLessEqual(kwargs["timeout"], 4.5)
            self.assertGreater(kwargs["timeout"], 0)


ATT_0E = "Call failed: Operation failed with ATT error 0x0e (Unlikely Error)"


class ScriptedBus:
    """busctl fake: per-call outcomes for WriteValue, scripted reads, simulated call duration."""

    def __init__(self, clock, reads, writes=(), call_seconds=0.0):
        self.clock = clock
        self.reads = list(reads)
        self.writes = list(writes)  # None = success, str = stderr of a failing call, "timeout"
        self.call_seconds = call_seconds
        self.log = []  # "GET" / "CONNECT" / "WRITE"

    def __call__(self, argv, **kwargs):
        if self.call_seconds > kwargs["timeout"]:  # the real runner kills the call at its timeout
            self.clock.now += kwargs["timeout"]
            self.log.append("GET" if "GetManagedObjects" in argv else "WRITE")
            raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
        self.clock.now += self.call_seconds
        if "GetManagedObjects" in argv:
            self.log.append("GET")
            out = self.reads.pop(0) if len(self.reads) > 1 else self.reads[0]
            return subprocess.CompletedProcess(argv, 0, out, "")
        if "Connect" in argv:
            self.log.append("CONNECT")
            return subprocess.CompletedProcess(argv, 0, "", "")
        self.log.append("WRITE")
        outcome = self.writes.pop(0) if self.writes else None
        if outcome == "timeout":
            raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
        if outcome is not None:
            return subprocess.CompletedProcess(argv, 1, "", outcome)
        return subprocess.CompletedProcess(argv, 0, "", "")

    def writes_made(self):
        return self.log.count("WRITE")


class ReadinessTests(unittest.TestCase):
    def test_stale_pre_connect_snapshot_never_allows_a_write(self):
        clock = Clock()
        stale = objects_json(connected=False, resolved=True)
        bus = ScriptedBus(clock, [stale, objects_json(connected=True, resolved=False),
                                  objects_json(connected=True, resolved=True)])
        reply = invoke("power-on", {"address": ADDRESS}, bus, clock=clock)
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(bus.log, ["GET", "CONNECT", "GET", "GET", "WRITE"])
        self.assertEqual(len(clock.sleeps), 1)

    def test_one_extra_read_per_poll(self):
        clock = Clock()
        unresolved = objects_json(connected=True, resolved=False)
        bus = ScriptedBus(clock, [objects_json(connected=False, resolved=True), unresolved,
                                  unresolved, unresolved, objects_json()])
        reply = invoke("power-on", {"address": ADDRESS}, bus, clock=clock)
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(bus.log, ["GET", "CONNECT", "GET", "GET", "GET", "GET", "WRITE"])
        self.assertEqual(len(clock.sleeps), 3)

    def test_connected_but_unresolved_connect_not_called_and_waits(self):
        clock = Clock()
        bus = ScriptedBus(clock, [objects_json(connected=True, resolved=False), objects_json()])
        self.assertTrue(invoke("power-on", {"address": ADDRESS}, bus, clock=clock)["ok"])
        self.assertEqual(bus.log, ["GET", "GET", "WRITE"])

    def test_connected_and_resolved_writes_right_after_first_read(self):
        clock = Clock()
        bus = ScriptedBus(clock, [objects_json()])
        self.assertTrue(invoke("power-on", {"address": ADDRESS}, bus, clock=clock)["ok"])
        self.assertEqual(bus.log, ["GET", "WRITE"])

    def test_budget_ending_before_ready_reports_connect_timeout(self):
        clock = Clock()
        bus = ScriptedBus(clock, [objects_json(connected=False, resolved=True),
                                  objects_json(connected=True, resolved=False)])
        reply = invoke("power-on", {"address": ADDRESS}, bus, clock=clock)
        self.assertFalse(reply["ok"])
        self.assertIn("could not connect in time; close the MeRGBW app and try again",
                      reply["error"])
        self.assertEqual(bus.writes_made(), 0)
        self.assertLessEqual(clock.now - 100.0, 4.5)


class RetryTests(unittest.TestCase):
    def go(self, writes, action="set-scene", settings=None, reads=None, call_seconds=0.0,
           clock=None):
        clock = clock or Clock()
        bus = ScriptedBus(clock, reads or [objects_json()], writes, call_seconds)
        settings = settings or {"address": ADDRESS, "scene": 2}
        return invoke(action, settings, bus, clock=clock), bus, clock

    def test_att_0e_then_success_retries_once_after_a_fresh_read(self):
        reply, bus, _ = self.go([None, ATT_0E, None, None])
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(bus.log, ["GET", "WRITE", "WRITE", "GET", "WRITE", "WRITE"])
        self.assertIn("after 1 retry", reply["message"])
        self.assertIn("3 command packets", reply["message"])

    def test_retry_repeats_the_same_packet(self):
        runner = FakeBus([objects_json()])
        seen = []

        def wrapped(argv, **kw):
            if "WriteValue" in argv:
                seen.append(argv)
                if len(seen) == 1:
                    return subprocess.CompletedProcess(argv, 1, "", ATT_0E)
            return runner(argv, **kw)

        reply = invoke("set-brightness", {"address": ADDRESS, "brightness": 30}, wrapped)
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(seen, [write_argv(run_mod.brightness_packet(300))] * 2)

    def test_no_retry_without_failure_message(self):
        reply, _, _ = self.go([None, None, None], action="power-on",
                              settings={"address": ADDRESS})
        self.assertNotIn("retry", reply["message"])

    def test_att_0e_twice_fails_naming_the_step_and_sends_nothing_more(self):
        reply, bus, _ = self.go([None, ATT_0E, ATT_0E])
        self.assertFalse(reply["ok"])
        self.assertIn("write packet 2/3", reply["error"])
        self.assertIn("0x0e", reply["error"])
        self.assertEqual(bus.writes_made(), 3)
        self.assertEqual(bus.log.count("GET"), 2)

    def test_second_rejection_in_a_request_is_not_retried_again(self):
        reply, bus, _ = self.go([ATT_0E, None, ATT_0E])
        self.assertFalse(reply["ok"])
        self.assertIn("write packet 2/3", reply["error"])
        self.assertEqual(bus.writes_made(), 3)

    def test_other_att_code_is_not_retried(self):
        reply, bus, _ = self.go(["Call failed: Operation failed with ATT error 0x0a"])
        self.assertFalse(reply["ok"])
        self.assertIn("write packet 1/3", reply["error"])
        self.assertEqual(bus.writes_made(), 1)

    def test_plain_busctl_failure_is_not_retried(self):
        reply, bus, _ = self.go(["Failed to connect to bus"])
        self.assertFalse(reply["ok"])
        self.assertEqual(bus.writes_made(), 1)
        self.assertEqual(bus.log.count("GET"), 1)

    def test_timeout_is_not_retried(self):
        reply, bus, _ = self.go(["timeout"])
        self.assertFalse(reply["ok"])
        self.assertIn("write packet 1/3", reply["error"])
        self.assertEqual(bus.writes_made(), 1)

    def test_0x0e_outside_an_att_error_is_not_retried(self):
        reply, bus, _ = self.go(["error code 0x0e without the protocol name"])
        self.assertFalse(reply["ok"])
        self.assertEqual(bus.writes_made(), 1)

    def test_connect_failure_is_never_retried(self):
        clock = Clock()

        calls = []

        def runner(argv, **kw):
            calls.append(argv)
            if "Connect" in argv:
                return subprocess.CompletedProcess(argv, 1, "", ATT_0E)
            return subprocess.CompletedProcess(
                argv, 0, objects_json(connected=False, resolved=False), "")

        reply = invoke("power-on", {"address": ADDRESS}, runner, clock=clock)
        self.assertFalse(reply["ok"])
        self.assertEqual(calls, [GET, CONNECT])

    def test_no_retry_when_less_than_one_second_remains(self):
        clock = Clock()
        bus = ScriptedBus(clock, [objects_json()], [ATT_0E])

        def runner(argv, **kw):
            result = bus(argv, **kw)
            if "WriteValue" in argv:
                clock.now += 3.6  # leaves 0.9 s of the 4.5 s budget
            return result

        reply = invoke("power-on", {"address": ADDRESS}, runner, clock=clock)
        self.assertFalse(reply["ok"])
        self.assertIn("write packet 1/1", reply["error"])
        self.assertEqual(bus.writes_made(), 1)
        self.assertEqual(bus.log.count("GET"), 1)

    def test_retry_allowed_with_exactly_one_second_remaining(self):
        clock = Clock()
        bus = ScriptedBus(clock, [objects_json()], [ATT_0E, None])

        def runner(argv, **kw):
            result = bus(argv, **kw)
            if "WriteValue" in argv and bus.writes_made() == 1:
                clock.now += 3.5
            return result

        reply = invoke("power-on", {"address": ADDRESS}, runner, clock=clock)
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(bus.writes_made(), 2)

    def test_retry_waits_for_services_within_the_budget(self):
        reads = [objects_json(), objects_json(connected=False, resolved=False),
                 objects_json(connected=True, resolved=False), objects_json()]
        reply, bus, clock = self.go([ATT_0E, None, None, None], reads=reads)
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(bus.log[:5], ["GET", "WRITE", "GET", "GET", "GET"])
        self.assertEqual(bus.writes_made(), 4)

    def test_retry_that_cannot_become_ready_fails_without_writing(self):
        reads = [objects_json(), objects_json(connected=False, resolved=False)]
        reply, bus, clock = self.go([ATT_0E], reads=reads)
        self.assertFalse(reply["ok"])
        self.assertIn("write packet 1/3", reply["error"])
        self.assertEqual(bus.writes_made(), 1)
        self.assertLessEqual(clock.now - 100.0, 4.5)

    def test_worst_case_elapsed_time_stays_within_the_budget(self):
        for call_seconds in (0.0, 0.3, 0.9, 1.7):
            with self.subTest(call_seconds=call_seconds):
                reads = [objects_json(connected=False, resolved=False),
                         objects_json(connected=True, resolved=False)]
                reply, bus, clock = self.go([ATT_0E, ATT_0E, ATT_0E], reads=reads,
                                            call_seconds=call_seconds)
                self.assertLessEqual(clock.now - 100.0, 4.5 + 1e-9)
                self.assertLessEqual(bus.writes_made(), 2)
                for sleep in clock.sleeps:
                    self.assertLessEqual(sleep, 0.25)

    def test_error_text_for_a_rejection_stays_short_and_unechoed(self):
        raw = f"Call failed: Operation failed with ATT error 0x0e {SECRET}\n" + "z" * 5000
        reply, _, _ = self.go([raw, raw])
        self.assertFalse(reply["ok"])
        self.assertLessEqual(len(reply["error"]), 300)
        self.assertNotIn("zzzz", reply["error"])


class ColorBrightnessTests(unittest.TestCase):
    def packets(self, settings):
        return run_mod.build_packets("set-color", {"address": ADDRESS, **settings})[1]

    def test_without_brightness_only_power_and_colour(self):
        self.assertEqual(self.packets({"hue": 10}), [
            run_mod.power_packet(True), run_mod.color_packet(10, 1000)])

    def test_with_brightness_it_comes_last_scaled(self):
        self.assertEqual(self.packets({"hue": 10, "brightness": 30}), [
            run_mod.power_packet(True), run_mod.color_packet(10, 1000),
            run_mod.brightness_packet(300)])

    def test_zero_brightness_is_still_explicit(self):
        self.assertEqual(len(self.packets({"brightness": 0})), 3)

    def test_two_writes_without_and_three_with_brightness(self):
        clock = Clock()
        bus = ScriptedBus(clock, [objects_json()])
        invoke("set-color", {"address": ADDRESS}, bus, clock=clock)
        self.assertEqual(bus.writes_made(), 2)
        bus = ScriptedBus(clock, [objects_json()])
        invoke("set-color", {"address": ADDRESS, "brightness": 50}, bus, clock=clock)
        self.assertEqual(bus.writes_made(), 3)

    def test_set_brightness_keeps_its_default(self):
        packets = run_mod.build_packets("set-brightness", {"address": ADDRESS})[1]
        self.assertEqual(packets, [run_mod.brightness_packet(1000)])


class FramingTests(unittest.TestCase):
    def raw(self, text, runner=None):
        runner = runner or FakeBus([objects_json()])
        out, err = io.StringIO(), io.StringIO()
        code = run_mod.main([], io.StringIO(text), out, {}, run=runner, stderr=err,
                            monotonic=Clock().monotonic, sleep=Clock().sleep)
        self.assertEqual(code, 0)
        lines = out.getvalue().splitlines()
        self.assertEqual(len(lines), 1)
        self.assertNotIn("Traceback", out.getvalue())
        return json.loads(lines[0]), runner

    def test_success_shape(self):
        reply, _ = self.raw(json.dumps({"protocol_version": 1, "request_id": "abc", "type": "invoke",
                                        "action": "power-on", "settings": {"address": ADDRESS}}))
        self.assertEqual(reply["protocol_version"], 1)
        self.assertEqual(reply["request_id"], "abc")
        self.assertTrue(reply["ok"])
        self.assertLessEqual(len(reply["message"]), 512)
        self.assertNotIn("error", reply)

    def test_bad_protocol_versions(self):
        for version in (True, 1.0, "1", 2, None):
            with self.subTest(version=version):
                reply, runner = self.raw(json.dumps({
                    "protocol_version": version, "request_id": "abc", "type": "invoke",
                    "action": "power-on", "settings": {"address": ADDRESS}}))
                self.assertFalse(reply["ok"])
                self.assertEqual(runner.calls, [])

    def test_not_json_and_not_object(self):
        for text in ["", "garbage", "[1]", "5"]:
            with self.subTest(text=text):
                reply, runner = self.raw(text + "\n")
                self.assertFalse(reply["ok"])
                self.assertEqual(reply["request_id"], "unknown")
                self.assertEqual(runner.calls, [])

    def test_wrong_type(self):
        reply, _ = self.raw(json.dumps({"protocol_version": 1, "request_id": "a", "type": "x"}))
        self.assertFalse(reply["ok"])

    def test_internal_error_has_no_traceback_or_text(self):
        def boom(argv, **kw):
            raise RuntimeError(SECRET)

        reply, _ = self.raw(json.dumps({"protocol_version": 1, "request_id": "a", "type": "invoke",
                                        "action": "power-on", "settings": {"address": ADDRESS}}), boom)
        self.assertFalse(reply["ok"])
        self.assertNotIn(SECRET, json.dumps(reply))

    def test_long_message_is_clipped(self):
        self.assertEqual(len(run_mod.clip("x" * 900)), 512)


class SubprocessTests(unittest.TestCase):
    def test_runs_with_cleared_environment_and_fake_busctl(self):
        scratch = Path(tempfile.mkdtemp(prefix="mergbw-test-"))
        self.addCleanup(shutil.rmtree, scratch, True)
        bin_dir, log = scratch / "bin", scratch / "argv.log"
        bin_dir.mkdir()
        canned = scratch / "objects.json"
        canned.write_text(objects_json())
        fake = bin_dir / "busctl"
        fake.write_text(
            "#!/bin/sh\n"
            f'echo "$@" >> "{log}"\n'
            'case "$*" in *GetManagedObjects*) '
            f'IFS= read -r line < "{canned}"; echo "$line";; esac\nexit 0\n'
        )
        fake.chmod(0o755)
        (bin_dir / "python3").symlink_to(sys.executable)
        env = {
            "PATH": str(bin_dir),
            "HOME": str(scratch / "home"),
            "LANG": "C",
            "XDG_RUNTIME_DIR": str(scratch / "run"),
            "XDG_CONFIG_HOME": str(scratch / "config"),
            "XDG_DATA_HOME": str(scratch / "data"),
            "WAYLAND_DISPLAY": "wayland-test",
            "DBUS_SESSION_BUS_ADDRESS": "unix:path=/nonexistent",
        }
        probe = subprocess.run(["/bin/sh", "-c", "command -v busctl"], env={"PATH": str(bin_dir)},
                               capture_output=True, text=True, check=False)
        self.assertEqual(probe.stdout.strip(), str(fake))
        self.assertNotIn("/usr/bin", probe.stdout)
        request = json.dumps({"protocol_version": 1, "request_id": "sub-1", "type": "invoke",
                              "action": "power-on", "settings": {"address": ADDRESS}}) + "\n"
        done = subprocess.run([str(bin_dir / "python3"), "-B", "-I", str(RUN_PATH)], input=request,
                              env=env, capture_output=True, text=True, timeout=30, check=False)
        self.assertEqual(done.returncode, 0, done.stderr)
        reply = json.loads(done.stdout)
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(reply["request_id"], "sub-1")
        lines = log.read_text().splitlines()
        self.assertEqual(len(lines), 2)
        self.assertIn("GetManagedObjects", lines[0])
        self.assertEqual(lines[1], " ".join(write_argv([0x55, 1, 0xFF, 6, 1, 0xA2])[1:]))


if __name__ == "__main__":
    unittest.main()
