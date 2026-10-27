"""Check Google sign-in through the Rust client and its platform credential store."""

import argparse
import getpass
import json
import queue
import subprocess
import sys
import threading
import uuid
from google_signin import SafeError, authorize, exchange, verify_identity


def check(command, identity_token):
    child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True, encoding='utf-8')
    try:
        received = queue.Queue()
        def read_challenge():
            try:
                received.put(child.stdout.readline(4097))
            except Exception:
                received.put('')
        threading.Thread(target=read_challenge, daemon=True).start()
        try:
            raw = received.get(timeout=30)
            challenge = json.loads(raw)
            if (len(raw) > 4096 or set(challenge) != {'challenge_id', 'nonce'}
                    or str(uuid.UUID(challenge['challenge_id'])) != challenge['challenge_id']
                    or not isinstance(challenge['nonce'], str) or len(challenge['nonce']) != 64
                    or any(c not in '0123456789abcdef' for c in challenge['nonce'])):
                raise ValueError()
        except (queue.Empty, ValueError, TypeError, KeyError, AttributeError):
            raise SafeError('The Rust client did not return a valid challenge. Check the API and credential store.') from None
        token = identity_token(challenge['nonce'])
        if not isinstance(token, str) or not token or len(token) > 12288 or '\n' in token or '\r' in token:
            raise SafeError('The identity token input is invalid.')
        output, _ = child.communicate(input=token + '\n', timeout=90)
        if child.returncode == 3:
            raise SafeError('A stored session already exists. Use --cleanup before this check.')
        if child.returncode != 0:
            raise SafeError('The Rust check failed. Use --cleanup to remove any incomplete test session.')
        try:
            result = json.loads(output)
            if result != {'restored': True, 'signed_out': True}:
                raise ValueError()
        except (ValueError, TypeError):
            raise SafeError('The Rust check returned an invalid result. Use --cleanup before retrying.') from None
        return result
    except subprocess.TimeoutExpired:
        raise SafeError('The Rust check timed out. Use --cleanup before retrying.') from None
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--client', required=True, help='Path to the built session_check executable.')
    parser.add_argument('--client-id', required=True)
    parser.add_argument('--email', required=True)
    parser.add_argument('--server', default='http://127.0.0.1:8788')
    parser.add_argument('--tenant', required=True)
    parser.add_argument('--person', required=True)
    parser.add_argument('--cleanup', action='store_true')
    args = parser.parse_args()
    command = [args.client, args.server, args.tenant, args.person]
    if args.cleanup:
        result = subprocess.run(command + ['cleanup'], capture_output=True, timeout=60)
        if result.returncode != 0:
            raise SafeError('Session cleanup failed. Check the API and credential store, then retry.')
        print('PASS: Session revocation and credential cleanup completed.')
        return
    secret = getpass.getpass('Desktop client secret (hidden; Enter if none was supplied): ')
    print('Client secret received. Its value is hidden.' if secret else 'No client secret was entered.')
    def identity_token(nonce):
        print(f'Select {args.email} in the browser. This check requests sign-in and email only.')
        code, redirect, nonce, verifier = authorize(args.client_id, nonce)
        token = exchange(args.client_id, secret, code, verifier, redirect)
        verify_identity(token, args.client_id, nonce, args.email)
        return token
    check(command, identity_token)
    print('PASS: The Rust client accepted the approved Google identity.')
    print('PASS: A new client restored the stored credential and renewed the session.')
    print('PASS: Sign-out completed and the stored credential was removed.')
    print('No time entries or Xero records were changed.')


if __name__ == '__main__':
    try:
        main()
    except SafeError as error:
        print(f'FAIL: {error}', file=sys.stderr)
        sys.exit(1)
    except (KeyboardInterrupt, EOFError):
        print('Check cancelled. Use --cleanup if sign-in already completed.', file=sys.stderr)
        sys.exit(1)
    except Exception:
        print('FAIL: The Rust check failed. Use --cleanup before retrying. No private response was logged.', file=sys.stderr)
        sys.exit(1)
