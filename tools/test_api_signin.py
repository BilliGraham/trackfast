import unittest
from unittest.mock import patch
import api_signin


class ApiSigninTests(unittest.TestCase):
    def test_session_check_rotates_and_revokes_without_returning_tokens(self):
        calls = []
        def request(path, body=None, token=None):
            calls.append((path, body, token))
            if path.endswith('/exchange'):
                return {'access_token': 'access-one', 'refresh_token': 'refresh-one'}
            if path.endswith('/refresh'):
                return {'access_token': 'access-two', 'refresh_token': 'refresh-two'}
            if path == '/v1/me':
                if token == 'access-two' and calls[-2][0].endswith('/logout'):
                    raise api_signin.Unauthorized()
                return {'tenant_id': 'tenant', 'person_id': 'person'}
        with patch.object(api_signin, 'api_request', side_effect=request):
            self.assertEqual(api_signin.check_session('challenge', 'identity-token'),
                             {'tenant_id': 'tenant', 'person_id': 'person'})
        self.assertIn(('/v1/auth/refresh', {'refresh_token': 'refresh-one'}, None), calls)
        self.assertIn(('/v1/auth/logout', {'refresh_token': 'refresh-two'}, None), calls)

    def test_request_rejects_unlisted_paths(self):
        for path in ['https://example.com', '//example.com', '/v1/me?token=private']:
            with self.assertRaises(api_signin.SafeError):
                api_signin.api_request(path)


if __name__ == '__main__':
    unittest.main()
