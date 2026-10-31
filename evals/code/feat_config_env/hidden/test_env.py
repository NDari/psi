import os
import unittest
from unittest import mock

import settings

INI = "/hidden/mailer.ini"


class Env(unittest.TestCase):
    def test_env_overrides_default(self):
        s = settings.load(env={"MAILER_SMTP_RELAY": "relay.example", "MAILER_QUEUE_DIR": "/tmp/q"})
        self.assertEqual(s["smtp"]["relay"], "relay.example")
        self.assertEqual(s["queue"]["dir"], "/tmp/q")
        self.assertEqual(s["server"]["port"], 8025)

    def test_env_overrides_file(self):
        s = settings.load(INI, env={"MAILER_SERVER_PORT": "9100"})
        self.assertEqual(s["server"]["port"], 9100)
        self.assertEqual(s["smtp"]["relay"], "mail.internal")
        self.assertIs(s["server"]["debug"], True)

    def test_types_converted(self):
        s = settings.load(env={"MAILER_SERVER_DEBUG": "On", "MAILER_SMTP_USE_TLS": "0",
                               "MAILER_SMTP_TIMEOUT": "3", "MAILER_QUEUE_RETRY_MINUTES": " 5 "})
        self.assertIs(s["server"]["debug"], True)
        self.assertIs(s["smtp"]["use_tls"], False)
        self.assertIsInstance(s["smtp"]["timeout"], float)
        self.assertEqual(s["smtp"]["timeout"], 3.0)
        self.assertEqual(s["queue"]["retry_minutes"], 5)

    def test_key_with_underscore(self):
        s = settings.load(env={"MAILER_SERVER_MAX_CONNECTIONS": "200"})
        self.assertEqual(s["server"]["max_connections"], 200)

    def test_bad_value_names_variable(self):
        with self.assertRaises(settings.ConfigError) as cm:
            settings.load(env={"MAILER_SERVER_PORT": "eighty"})
        self.assertIn("MAILER_SERVER_PORT", str(cm.exception))
        with self.assertRaises(settings.ConfigError) as cm:
            settings.load(env={"MAILER_SMTP_USE_TLS": "maybe"})
        self.assertIn("MAILER_SMTP_USE_TLS", str(cm.exception))

    def test_unknown_variables_ignored(self):
        s = settings.load(env={"MAILER_SERVER_COLOR": "red", "MAILER_": "x", "MAILER_PORT": "1",
                               "OTHER_SERVER_PORT": "1", "MAILER_SMTP_RELAY": "r"})
        want = settings.load(env={})
        want["smtp"]["relay"] = "r"
        self.assertEqual(s, want)

    def test_default_env_is_os_environ(self):
        with mock.patch.dict(os.environ, {"MAILER_SERVER_HOST": "0.0.0.0"}):
            self.assertEqual(settings.load()["server"]["host"], "0.0.0.0")
            self.assertEqual(settings.load(env={})["server"]["host"], "127.0.0.1")


class Existing(unittest.TestCase):
    def test_file_still_loads(self):
        s = settings.load(INI)
        self.assertEqual((s["server"]["port"], s["smtp"]["timeout"]), (9000, 2.5))
        with self.assertRaises(settings.ConfigError):
            settings.load("/hidden/no-such-file.ini")
