"""Test the isolated Google sign-in check without contacting Google."""

import json
import time
import unittest
from urllib.parse import parse_qs, urlsplit
from unittest.mock import patch

import google_signin as signin


class GoogleSignInTests(unittest.TestCase):
    def test_verification_errors_name_the_failed_check_without_private_values(self):
        from google.auth import exceptions
        failures = [
            (exceptions.InvalidValue('Token used too early, private-value'), 'clock'),
            (exceptions.InvalidValue('Token expired, private-value'), 'expired'),
            (exceptions.InvalidValue('Token has wrong audience private-value'), 'client ID'),
            (exceptions.MalformedError('Could not verify token signature.'), 'signature'),
            (exceptions.GoogleAuthError('Wrong issuer. private-value'), 'issuer'),
            (signin.SafeError('private-value'), 'certificates'),
        ]
        for error, expected in failures:
            with self.subTest(expected=expected), patch('google.oauth2.id_token.verify_oauth2_token', side_effect=error):
                with self.assertRaises(signin.SafeError) as failure:
                    signin.verify_identity('private-token', 'client', 'nonce', 'person@example.com')
                self.assertIn(expected, str(failure.exception))
                self.assertNotIn('private-value', str(failure.exception))
                self.assertNotIn('private-token', str(failure.exception))

    def test_verified_claim_errors_are_distinct(self):
        claims = {'aud': 'client', 'nonce': 'nonce', 'email_verified': True,
                  'email': 'person@example.com', 'sub': 'subject'}
        for field, value, expected in [('nonce', None, 'nonce'), ('azp', 'other', 'client ID'),
                                       ('email_verified', False, 'verified'),
                                       ('email', 'other@example.com', 'approved email'),
                                       ('sub', '', 'subject')]:
            with self.subTest(field=field), patch('google.oauth2.id_token.verify_oauth2_token', return_value={**claims, field: value}):
                with self.assertRaises(signin.SafeError) as failure:
                    signin.verify_identity('token', 'client', 'nonce', 'person@example.com')
                self.assertIn(expected, str(failure.exception))
                self.assertNotIn('other@example.com', str(failure.exception))

    def test_authorization_uses_pkce_state_nonce_and_only_identity_scopes(self):
        url = signin.authorization_url('test-client', 'http://127.0.0.1:49152/google/callback',
                                       'state', 'nonce', 'verifier')
        parsed = urlsplit(url)
        self.assertEqual(parsed.netloc, 'accounts.google.com')
        values = parse_qs(parsed.query)
        self.assertEqual(values['scope'], ['openid email'])
        self.assertEqual(values['state'], ['state'])
        self.assertEqual(values['nonce'], ['nonce'])
        self.assertEqual(values['code_challenge_method'], ['S256'])
        self.assertNotIn('verifier', url)
        self.assertNotIn('access_type', values)

    def test_callback_rejects_wrong_state_duplicate_values_and_foreign_paths(self):
        self.assertEqual(signin.callback_code('/google/callback?state=expected&code=code',
                                             'expected'), 'code')
        for path in ['/other?state=expected&code=code',
                     '/google/callback?state=wrong&code=code',
                     '/google/callback?state=expected&state=expected&code=code',
                     '/google/callback?state=expected&code=a&code=b',
                     'https://example.com/google/callback?state=expected&code=code',
                     '/google/callback?state=expected&error=access_denied']:
            with self.subTest(path=path), self.assertRaises(signin.SafeError):
                signin.callback_code(path, 'expected')

    def test_signed_tokens_require_audience_issuer_expiry_nonce_and_approved_email(self):
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric import rsa
        from google.auth import crypt, jwt

        key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        private = key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                    serialization.NoEncryption())
        public = key.public_key().public_bytes(serialization.Encoding.PEM,
                                              serialization.PublicFormat.SubjectPublicKeyInfo)
        signer = crypt.RSASigner.from_string(private, key_id='test-key')
        now = int(time.time())
        claims = {'iss': 'https://accounts.google.com', 'aud': 'test-client',
                  'sub': 'stable-subject', 'iat': now, 'exp': now + 300,
                  'email': 'person@example.com', 'email_verified': True, 'nonce': 'nonce'}
        response = type('Response', (), {'status': 200, 'data': json.dumps(
            {'test-key': public.decode()}).encode()})()
        request = lambda *args, **kwargs: response
        token = jwt.encode(signer, claims).decode()
        self.assertEqual(signin.verify_identity(token, 'test-client', 'nonce',
                                               'person@example.com', request), 'stable-subject')
        for field, value in [('iss', 'https://evil.example'), ('aud', 'other-client'),
                             ('exp', now - 1), ('iat', now + 600), ('nonce', 'wrong'),
                             ('email', 'other@example.com'), ('email_verified', False),
                             ('email_verified', 'true'), ('sub', ''), ('azp', 'other-client')]:
            bad = {**claims, field: value}
            with self.subTest(field=field), self.assertRaises(signin.SafeError):
                signin.verify_identity(jwt.encode(signer, bad).decode(), 'test-client', 'nonce',
                                       'person@example.com', request)
        with self.assertRaises(signin.SafeError):
            signin.verify_identity(token[:-8] + 'invalid!', 'test-client', 'nonce',
                                   'person@example.com', request)

    def test_exchange_errors_do_not_expose_codes_or_provider_responses(self):
        with patch.object(signin, 'request_json', side_effect=signin.SafeError('Request failed.')):
            with self.assertRaises(signin.SafeError) as failure:
                signin.exchange('client', 'secret', 'private-code', 'verifier', 'http://127.0.0.1:1/google/callback')
        self.assertNotIn('private-code', str(failure.exception))
        self.assertNotIn('secret', str(failure.exception))


if __name__ == '__main__':
    unittest.main()
