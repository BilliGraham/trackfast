"""Check a Xero demo connection without saving credentials."""

import base64
import argparse
import getpass
import json
import secrets
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser
from http.server import BaseHTTPRequestHandler, HTTPServer


REDIRECT = "http://localhost:8787/xero/callback"
DEMO_NAME = "Demo Company (Global)"


class SafeError(Exception):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise SafeError("Xero returned an unexpected redirect. The request stopped.")


def request_json(url, headers, data=None, method=None):
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    try:
        with opener.open(request, timeout=30) as response:
            content = response.read(2_000_001)
            if len(content) > 2_000_000:
                raise SafeError("The response exceeded the size limit.")
            return json.loads(content) if content else None
    except urllib.error.HTTPError as error:
        raise SafeError(f"Xero returned HTTP {error.code}. No response body was logged.") from None
    except (urllib.error.URLError, TimeoutError, OSError):
        raise SafeError("The network request failed. Check the connection and retry.") from None
    except (ValueError, UnicodeError):
        raise SafeError("Xero returned an invalid JSON response.") from None


def callback_code(path, expected_state):
    parsed = urllib.parse.urlsplit(path)
    if parsed.path != "/xero/callback" or parsed.scheme or parsed.netloc:
        raise SafeError("The callback path is invalid.")
    values = urllib.parse.parse_qs(parsed.query, keep_blank_values=True)
    states = values.get("state", [])
    if len(states) != 1 or not secrets.compare_digest(states[0], expected_state):
        raise SafeError("The callback state is invalid. Restart the connection.")
    if "error" in values:
        raise SafeError("Xero authorization was declined or failed.")
    codes = values.get("code", [])
    if len(codes) != 1 or not codes[0]:
        raise SafeError("The callback has no unique authorization code.")
    return codes[0]


def demo_tenant(connections):
    if not isinstance(connections, list):
        raise SafeError("The connection response is invalid.")
    matches = [item for item in connections if isinstance(item, dict)
               and item.get("tenantName") == DEMO_NAME
               and item.get("tenantType") == "ORGANISATION"]
    if len(matches) != 1 or not matches[0].get("tenantId"):
        raise SafeError("Exactly one Demo Company (Global) connection is required.")
    return matches[0]["tenantId"]


def authorize(client_id, write_test=False):
    state = secrets.token_urlsafe(32)
    result = {}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, format, *args):
            pass

        def do_GET(self):
            if self.headers.get("Host") != "localhost:8787":
                self.send_error(400, "Invalid host")
                return
            try:
                code = callback_code(self.path, state)
            except SafeError:
                self.send_error(400, "Authorization callback rejected")
                return
            result["code"] = code
            body = b"Connection received. Close this tab and return to the terminal."
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("Referrer-Policy", "no-referrer")
            self.end_headers()
            self.wfile.write(body)

    class Server(HTTPServer):
        def get_request(self):
            connection, address = super().get_request()
            connection.settimeout(2)
            return connection, address

        def handle_error(self, request, client_address):
            pass

    try:
        server = Server(("127.0.0.1", 8787), Handler)
    except OSError:
        raise SafeError("Cannot open localhost port 8787. Close its current listener and retry.") from None
    with server:
        server.timeout = 1
        query = urllib.parse.urlencode({
            "response_type": "code", "client_id": client_id,
            "redirect_uri": REDIRECT, "scope": "projects" if write_test else "projects.read", "state": state,
        })
        print("Select only Demo Company (Global) in the browser.")
        print("This tool requests Projects write access." if write_test else "This tool requests read-only access.")
        if not webbrowser.open("https://login.xero.com/identity/connect/authorize?" + query):
            raise SafeError("Cannot open the browser. Set a default browser and retry.")
        deadline = time.monotonic() + 300
        while "code" not in result and time.monotonic() < deadline:
            server.handle_request()
        if "code" not in result:
            raise SafeError("Authorization timed out after five minutes. Restart the tool.")
    return result["code"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write-test', action='store_true', help='Test labelled demo time entries and remove them.')
    args = parser.parse_args()
    print("Trackfast Xero demo connection check")
    print("Credentials remain in process memory.")
    client_id = input("Client ID: ").strip()
    client_secret = getpass.getpass("Client secret (hidden): ").strip()
    if not client_id or not client_secret:
        raise SafeError("Both credential fields are required.")
    code = authorize(client_id, args.write_test)
    basic = base64.b64encode(f"{client_id}:{client_secret}".encode()).decode()
    tokens = request_json("https://identity.xero.com/connect/token", {
        "Authorization": "Basic " + basic,
        "Content-Type": "application/x-www-form-urlencoded",
    }, urllib.parse.urlencode({"grant_type": "authorization_code", "code": code,
                              "redirect_uri": REDIRECT}).encode())
    if not isinstance(tokens, dict) or not isinstance(tokens.get("access_token"), str):
        raise SafeError("Xero did not return an access token.")
    headers = {"Authorization": "Bearer " + tokens["access_token"], "Accept": "application/json"}
    tenant = demo_tenant(request_json("https://api.xero.com/connections", headers))
    headers["Xero-tenant-id"] = tenant
    if args.write_test:
        from xero_write import run_write_test
        run_write_test(headers)
        return
    projects = request_json("https://api.xero.com/projects.xro/2.0/Projects?pageSize=100", headers)
    if not isinstance(projects, dict) or not isinstance(projects.get("items"), list):
        raise SafeError("The Projects response is invalid.")
    print(f"PASS: Connected to {DEMO_NAME}.")
    print(f"PASS: Read {len(projects['items'])} project(s) from the first page.")
    print("No time entries were created or changed. Close this terminal when finished.")


if __name__ == "__main__":
    sys.modules.setdefault('xero_demo', sys.modules[__name__])
    try:
        main()
    except SafeError as error:
        print(f"STOP: {error}")
        raise SystemExit(1)
    except (KeyboardInterrupt, EOFError):
        print("Connection cancelled.")
        raise SystemExit(1)
    except Exception:
        print("STOP: An unexpected error occurred. Private error details were suppressed.")
        raise SystemExit(1)
