import io
import json
import os
import threading
import unittest
import uuid
from http.server import BaseHTTPRequestHandler, HTTPServer
from unittest.mock import patch
import rust_signin


class Child:
    def __init__(self):
        self.stdout = io.StringIO('{"challenge_id":"00000000-0000-4000-8000-000000000001","nonce":"' + 'a' * 64 + '"}\n')
        self.returncode = None
        self.input = None

    def communicate(self, input=None, timeout=None):
        self.input = input
        self.returncode = 0
        return '{"restored":true,"signed_out":true}\n', ''

    def poll(self):
        return self.returncode

    def kill(self):
        self.returncode = -1

    def wait(self):
        return self.returncode


class RustSigninTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get('TRACKFAST_SESSION_CHECK_BIN'), 'Requires the built Rust executable and an unlocked credential store.')
    def test_real_rust_process_restores_and_removes_synthetic_credential(self):
        tenant, person = str(uuid.uuid4()), str(uuid.uuid4())
        state = {'active': False, 'renewed': False, 'logged_out': False}
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def reply(self, status, body):
                raw = json.dumps(body).encode() if body is not None else b''
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def do_POST(self):
                size = int(self.headers.get('Content-Length', '0'))
                self.rfile.read(size)
                if self.path.endswith('/challenge'):
                    self.reply(200, {'challenge_id': str(uuid.uuid4()), 'nonce': 'c' * 64, 'expires_in': 300})
                elif self.path.endswith('/exchange'):
                    state['active'] = True
                    self.reply(200, {'access_token': 'a' * 64, 'refresh_token': 'b' * 64, 'token_type': 'Bearer', 'expires_in': 900})
                elif self.path.endswith('/refresh'):
                    state['renewed'] = True
                    self.reply(200, {'access_token': 'd' * 64, 'refresh_token': 'e' * 64, 'token_type': 'Bearer', 'expires_in': 900})
                elif self.path.endswith('/logout'):
                    state['active'] = False
                    state['logged_out'] = True
                    self.reply(204, None)
                else:
                    self.reply(404, {})

            def do_GET(self):
                expected = ('d' if state['renewed'] else 'a') * 64
                if state['active'] and self.headers.get('Authorization') == 'Bearer ' + expected:
                    self.reply(200, {'tenant_id': tenant, 'person_id': person})
                else:
                    self.reply(401, {})

        with HTTPServer(('127.0.0.1', 0), Handler) as server:
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            try:
                command = [os.environ['TRACKFAST_SESSION_CHECK_BIN'],
                           f'http://127.0.0.1:{server.server_port}', tenant, person]
                result = rust_signin.check(command, lambda _: 'synthetic.id.token')
                self.assertEqual(result, {'restored': True, 'signed_out': True})
                self.assertEqual(state, {'active': False, 'renewed': True, 'logged_out': True})
            finally:
                server.shutdown()
                worker.join()

    def test_google_token_uses_stdin_and_never_process_arguments(self):
        child = Child()
        with patch.object(rust_signin.subprocess, 'Popen', return_value=child) as spawn:
            result = rust_signin.check(['client', 'scope'], lambda nonce: 'private-token')
        self.assertEqual(child.input, 'private-token\n')
        self.assertNotIn('private-token', str(spawn.call_args))
        self.assertEqual(result, {'restored': True, 'signed_out': True})

    def test_invalid_challenge_stops_before_google_signin(self):
        child = Child()
        child.stdout = io.StringIO('{"nonce":"private-value"}\n')
        with patch.object(rust_signin.subprocess, 'Popen', return_value=child):
            with self.assertRaises(rust_signin.SafeError) as error:
                rust_signin.check(['client'], lambda _: self.fail('Must not contact Google'))
        self.assertNotIn('private-value', str(error.exception))
        self.assertEqual(child.returncode, -1)

    def test_child_failure_never_prints_private_output(self):
        child = Child()
        def fail(input=None, timeout=None):
            child.returncode = 1
            return 'private-token', 'private-secret'
        child.communicate = fail
        with patch.object(rust_signin.subprocess, 'Popen', return_value=child):
            with self.assertRaises(rust_signin.SafeError) as error:
                rust_signin.check(['client'], lambda _: 'private-token')
        self.assertNotIn('private-token', str(error.exception))
        self.assertNotIn('private-secret', str(error.exception))


if __name__ == '__main__':
    unittest.main()
