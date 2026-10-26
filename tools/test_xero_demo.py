import unittest
import uuid
from unittest.mock import patch
from xero_demo import SafeError, callback_code, demo_tenant
from xero_write import exercise, unique_named


class FakeApi:
    def __init__(self, duplicate=False, lose_response=False, update_failure=False, cleanup_failure=False):
        self.records = {'existing': {'description': 'Existing work'}}
        self.keys = {}
        self.posts = []
        self.duplicate = duplicate
        self.lose_response = lose_response
        self.update_failure = update_failure
        self.cleanup_failure = cleanup_failure

    def items(self, path):
        return list(self.records.values())

    def call(self, path, method='GET', payload=None, key=None):
        record_id = path.rsplit('/', 1)[-1]
        if method == 'POST':
            self.posts.append((key, dict(payload)))
            if key not in self.keys or self.duplicate:
                self.keys[key] = str(uuid.uuid4())
            record_id = self.keys[key]
            self.records[record_id] = dict(payload, timeEntryId=record_id)
            if self.lose_response:
                raise SafeError('Simulated lost response')
            return self.records[record_id]
        if method == 'PUT':
            if self.update_failure:
                raise SafeError('Simulated update failure')
            self.records[record_id] = dict(payload, timeEntryId=record_id)
        elif method == 'DELETE':
            if self.cleanup_failure:
                raise SafeError('Simulated cleanup failure')
            del self.records[record_id]
        else:
            return self.records[record_id]


class WriteGuards(unittest.TestCase):
    def run_exercise(self, api):
        with patch('builtins.print'):
            exercise(api, str(uuid.uuid4()), str(uuid.uuid4()), str(uuid.uuid4()), 'unique-test-marker')

    def test_create_replay_update_delete_preserves_existing(self):
        api = FakeApi()
        self.run_exercise(api)
        self.assertEqual(list(api.records), ['existing'])
        self.assertEqual(len(api.posts), 2)
        self.assertEqual(api.posts[0], api.posts[1])

    def test_duplicate_detection_removes_both_test_entries(self):
        api = FakeApi(duplicate=True)
        with self.assertRaisesRegex(SafeError, 'one entry'):
            self.run_exercise(api)
        self.assertEqual(list(api.records), ['existing'])

    def test_lost_response_still_cleans_unknown_entry(self):
        api = FakeApi(lose_response=True)
        with self.assertRaisesRegex(SafeError, 'lost response'):
            self.run_exercise(api)
        self.assertEqual(list(api.records), ['existing'])

    def test_update_failure_still_cleans_entry(self):
        api = FakeApi(update_failure=True)
        with self.assertRaisesRegex(SafeError, 'update failure'):
            self.run_exercise(api)
        self.assertEqual(list(api.records), ['existing'])

    def test_cleanup_failure_never_reports_success(self):
        api = FakeApi(cleanup_failure=True)
        with self.assertRaisesRegex(SafeError, 'Cleanup could not be confirmed'):
            self.run_exercise(api)

    def test_ambiguous_project_rejected(self):
        with self.assertRaises(SafeError):
            unique_named([{'name': 'New Website'}, {'name': 'New Website'}], 'New Website', 'projectId')


class ConnectionGuards(unittest.TestCase):
    def test_valid_callback(self):
        self.assertEqual(callback_code('/xero/callback?state=abc&code=test', 'abc'), 'test')

    def test_reject_invalid_callbacks(self):
        for path in ['/other?state=abc&code=test', '/xero/callback?state=bad&code=test',
                     '/xero/callback?state=abc&state=abc&code=test',
                     '/xero/callback?state=abc&code=a&code=b',
                     '/xero/callback?state=abc&error=access_denied']:
            with self.subTest(path=path), self.assertRaises(SafeError):
                callback_code(path, 'abc')

    def test_live_organisation_rejected(self):
        with self.assertRaises(SafeError):
            demo_tenant([{'tenantName': 'Real Company', 'tenantId': 'live',
                          'tenantType': 'ORGANISATION'}])

    def test_only_demo_selected(self):
        demo = {'tenantName': 'Demo Company (Global)', 'tenantId': 'demo',
                'tenantType': 'ORGANISATION'}
        self.assertEqual(demo_tenant([{'tenantName': 'Real Company'}, demo]), 'demo')
        with self.assertRaises(SafeError):
            demo_tenant([demo, demo])


if __name__ == '__main__':
    unittest.main()
