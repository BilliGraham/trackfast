"""Verify Google sign-in for one approved pilot email without saving credentials."""

import argparse
import base64
import getpass
import hashlib
import importlib
import json
import secrets
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser
from http.server import BaseHTTPRequestHandler, HTTPServer


class SafeError(Exception):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise SafeError('Google returned an unexpected redirect. The request stopped.')


def request_json(url, data=None):
    if url not in ('https://oauth2.googleapis.com/token',
                   'https://www.googleapis.com/oauth2/v1/certs'):
        raise SafeError('The Google endpoint is not permitted.')
    headers = {'Accept': 'application/json'}
    if data is not None:
        headers['Content-Type'] = 'application/x-www-form-urlencoded'
    request = urllib.request.Request(url, data=data, headers=headers)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    try:
        with opener.open(request, timeout=15) as response:
            raw = response.read(1_000_001)
            if len(raw) > 1_000_000:
                raise SafeError('The Google response exceeded the size limit.')
            return json.loads(raw)
    except urllib.error.HTTPError as error:
        raise SafeError(f'Google returned HTTP {error.code}. No response body was logged.') from None
    except (urllib.error.URLError, OSError, ValueError, UnicodeError):
        raise SafeError('The Google request failed or returned an invalid response.') from None


def authorization_url(client_id, redirect, state, nonce, verifier):
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode('ascii')).digest()).rstrip(b'=').decode('ascii')
    return 'https://accounts.google.com/o/oauth2/v2/auth?' + urllib.parse.urlencode({
        'client_id': client_id, 'redirect_uri': redirect, 'response_type': 'code',
        'scope': 'openid email', 'state': state, 'nonce': nonce,
        'code_challenge': challenge, 'code_challenge_method': 'S256',
        'prompt': 'select_account',
    })


def callback_code(path, expected_state):
    parsed = urllib.parse.urlsplit(path)
    if parsed.path != '/google/callback' or parsed.scheme or parsed.netloc or parsed.fragment:
        raise SafeError('The callback path is invalid.')
    values = urllib.parse.parse_qs(parsed.query, keep_blank_values=True, max_num_fields=20)
    states = values.get('state', [])
    if len(states) != 1 or not secrets.compare_digest(states[0], expected_state):
        raise SafeError('The callback state is invalid. Restart sign-in.')
    if 'error' in values:
        raise SafeError('Google sign-in was declined or failed.')
    codes = values.get('code', [])
    if len(codes) != 1 or not codes[0]:
        raise SafeError('The callback has no unique authorization code.')
    return codes[0]


def exchange(client_id, client_secret, code, verifier, redirect):
    fields = {'client_id': client_id, 'code': code, 'code_verifier': verifier,
              'redirect_uri': redirect, 'grant_type': 'authorization_code'}
    if client_secret:
        fields['client_secret'] = client_secret
    response = request_json('https://oauth2.googleapis.com/token',
                            urllib.parse.urlencode(fields).encode())
    if not isinstance(response, dict) or not isinstance(response.get('id_token'), str):
        raise SafeError('Google did not return an identity token.')
    return response['id_token']


def certificate_request(url, method='GET', **kwargs):
    if method != 'GET' or url != 'https://www.googleapis.com/oauth2/v1/certs':
        raise SafeError('The certificate request is not permitted.')
    class Response:
        status = 200
        data = json.dumps(request_json(url)).encode()
    return Response()


def verification_error(error):
    if isinstance(error, SafeError):
        return 'Cannot retrieve Google signing certificates. Check the connection and retry.'
    text = str(error)
    reasons = (
        ('Token used too early', 'The token is newer than the computer clock. Sync the system clock and retry.'),
        ('Token expired', 'The token expired according to the computer clock. Check the system clock and retry.'),
        ('Token has wrong audience', 'The token does not match this client ID. Check the Desktop app registration.'),
        ('Wrong issuer.', 'The token issuer is not Google. Sign-in was rejected.'),
        ('Could not verify token signature.', 'The token signature is invalid. Sign-in was rejected.'),
        ('Certificate for key id ', 'The signing key is not in the Google certificates. Restart sign-in.'),
    )
    for prefix, message in reasons:
        if text.startswith(prefix):
            return message
    return 'Google token validation failed. No token or provider response was logged.'


def verify_identity(token, client_id, nonce, approved_email, request=certificate_request):
    from google.oauth2 import id_token
    try:
        claims = id_token.verify_oauth2_token(token, request, audience=client_id)
    except Exception as error:
        raise SafeError(verification_error(error)) from None
    if claims.get('aud') != client_id or claims.get('azp', client_id) != client_id:
        raise SafeError('The token does not match this client ID. Check the Desktop app registration.')
    received_nonce = claims.get('nonce')
    if (not isinstance(received_nonce, str) or not received_nonce.isascii()
            or not secrets.compare_digest(received_nonce, nonce)):
        raise SafeError('The sign-in nonce is missing or does not match. Restart sign-in.')
    if claims.get('email_verified') is not True:
        raise SafeError('Google did not mark the account email as verified.')
    if (not isinstance(claims.get('email'), str)
            or claims['email'].casefold() != approved_email.casefold()):
        raise SafeError('The Google account does not match the approved email. Select the approved account and retry.')
    if (not isinstance(claims.get('sub'), str)
            or not claims['sub'] or len(claims['sub']) > 255):
        raise SafeError('The Google subject ID is missing or invalid.')
    return claims['sub']


def authorize(client_id, nonce=None):
    state, verifier = (secrets.token_urlsafe(32) for _ in range(2))
    nonce = nonce or secrets.token_urlsafe(32)
    result = {}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, format, *args):
            pass

        def do_GET(self):
            if self.headers.get_all('Host') != [f'127.0.0.1:{self.server.server_port}']:
                self.send_error(400, 'Invalid host')
                return
            try:
                code = callback_code(self.path, state)
            except (SafeError, ValueError, TypeError):
                self.send_error(400, 'Sign-in callback rejected')
                return
            result['code'] = code
            body = b'Sign-in response received. Close this tab and check the terminal result.'
            self.send_response(200)
            self.send_header('Content-Type', 'text/plain; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Referrer-Policy', 'no-referrer')
            self.end_headers()
            self.wfile.write(body)

    class Server(HTTPServer):
        def get_request(self):
            connection, address = super().get_request()
            connection.settimeout(2)
            return connection, address

        def handle_error(self, request, client_address):
            pass

    with Server(('127.0.0.1', 0), Handler) as server:
        server.timeout = 1
        redirect = f'http://127.0.0.1:{server.server_port}/google/callback'
        url = authorization_url(client_id, redirect, state, nonce, verifier)
        if not webbrowser.open(url):
            raise SafeError('Cannot open the browser. Set a default browser and retry.')
        deadline = time.monotonic() + 300
        while 'code' not in result and time.monotonic() < deadline:
            server.handle_request()
        if 'code' not in result:
            raise SafeError('Sign-in timed out after five minutes. Restart the tool.')
    return result['code'], redirect, nonce, verifier


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--client-id', required=True)
    parser.add_argument('--email', required=True, help='The exact email approved for this local test.')
    args = parser.parse_args()
    if not args.client_id.endswith('.apps.googleusercontent.com') or '@' not in args.email:
        raise SafeError('Check the Google client ID and approved email.')
    try:
        importlib.import_module("google.oauth2.id_token")
    except ImportError:
        raise SafeError('Install tools/requirements-signin.txt in the isolated Python environment.') from None
    print('Use the Desktop app client from Google Auth Platform.')
    secret = getpass.getpass('Desktop client secret (hidden; Enter if none was supplied): ')
    print('Client secret received. Its value is hidden.' if secret else 'No client secret was entered.')
    print(f'Select {args.email} in the browser. This check requests sign-in and email only.')
    code, redirect, nonce, verifier = authorize(args.client_id)
    token = exchange(args.client_id, secret, code, verifier, redirect)
    print('PASS: Google accepted the code exchange and returned an identity token.')
    subject = verify_identity(token, args.client_id, nonce, args.email)
    print('PASS: Google signature, issuer, audience, token times, and sign-in nonce are valid.')
    print('PASS: The verified email matches the approved pilot account.')
    print(f'Google subject ID for API membership: {subject}')
    print('No credentials were saved. No Trackfast or Xero entries were changed.')
    print('This check does not create an API session. Use tools/api_signin.py for that check.')


if __name__ == '__main__':
    try:
        main()
    except SafeError as error:
        print(f'FAIL: {error}', file=sys.stderr)
        sys.exit(1)
    except (KeyboardInterrupt, EOFError):
        print('Sign-in cancelled. No credentials were saved.', file=sys.stderr)
        sys.exit(1)
    except Exception:
        print('FAIL: The sign-in check failed. No private response was logged.', file=sys.stderr)
        sys.exit(1)
