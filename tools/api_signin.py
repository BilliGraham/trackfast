"""Check Google sign-in, API session renewal, and sign-out on the local pilot API."""

import argparse
import getpass
import json
import sys
import urllib.error
import urllib.request
from google_signin import SafeError, NoRedirect, authorize, exchange, verify_identity


class Unauthorized(SafeError):
    pass


def api_request(path, body=None, token=None):
    if path not in ('/v1/auth/google/challenge', '/v1/auth/google/exchange',
                    '/v1/auth/refresh', '/v1/auth/logout', '/v1/me'):
        raise SafeError('The API path is not permitted.')
    headers = {'Accept': 'application/json', 'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    request = urllib.request.Request('http://127.0.0.1:8788' + path,
                                     data=None if body is None else json.dumps(body).encode(),
                                     headers=headers)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    try:
        with opener.open(request, timeout=30) as response:
            raw = response.read(16385)
            if len(raw) > 16384:
                raise SafeError('The API response exceeded the size limit.')
            return json.loads(raw) if raw else None
    except urllib.error.HTTPError as error:
        if error.code == 401:
            raise Unauthorized('The API rejected the session or membership.') from None
        raise SafeError(f'The API returned HTTP {error.code}. No response body was logged.') from None
    except (urllib.error.URLError, OSError, ValueError, UnicodeError):
        raise SafeError('The local API request failed or returned an invalid response.') from None


def check_session(challenge_id, identity_token):
    tokens = api_request('/v1/auth/google/exchange',
                         {'challenge_id': challenge_id, 'id_token': identity_token})
    try:
        identity = api_request('/v1/me', token=tokens['access_token'])
        tokens = api_request('/v1/auth/refresh', {'refresh_token': tokens['refresh_token']})
        if api_request('/v1/me', token=tokens['access_token']) != identity:
            raise SafeError('The renewed session returned a different identity.')
    finally:
        api_request('/v1/auth/logout', {'refresh_token': tokens['refresh_token']})
    try:
        api_request('/v1/me', token=tokens['access_token'])
    except Unauthorized:
        return identity
    raise SafeError('The API accepted the session after sign-out.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--client-id', required=True)
    parser.add_argument('--email', required=True)
    args = parser.parse_args()
    secret = getpass.getpass('Desktop client secret (hidden; Enter if none was supplied): ')
    print('Client secret received. Its value is hidden.' if secret else 'No client secret was entered.')
    challenge = api_request('/v1/auth/google/challenge', {})
    print(f'Select {args.email} in the browser. This check requests sign-in and email only.')
    code, redirect, nonce, verifier = authorize(args.client_id, challenge['nonce'])
    token = exchange(args.client_id, secret, code, verifier, redirect)
    verify_identity(token, args.client_id, nonce, args.email)
    identity = check_session(challenge['challenge_id'], token)
    print('PASS: The API accepted the approved Google identity.')
    print('PASS: Session renewal kept the same tenant and person.')
    print('PASS: Sign-out revoked API access.')
    print(f"Tenant: {identity['tenant_id']}. Person: {identity['person_id']}.")
    print('No credentials were saved. No time entries or Xero records were changed.')


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
        print('FAIL: The API sign-in check failed. No private response was logged.', file=sys.stderr)
        sys.exit(1)
