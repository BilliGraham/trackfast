"""Exercise the API document against valid and invalid client payloads."""

import copy
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator, FormatChecker
from openapi_spec_validator import validate


ROOT = Path(__file__).resolve().parents[1]


class ApiContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = json.loads((ROOT / 'api/openapi.json').read_text(encoding='utf-8'))
        cls.cases = json.loads((ROOT / 'api/examples.json').read_text(encoding='utf-8'))

    def validator(self, name):
        schema = {'$ref': f'#/components/schemas/{name}',
                  'components': self.document['components']}
        return Draft202012Validator(schema, format_checker=FormatChecker())

    def test_openapi_is_valid_and_all_local_references_resolve(self):
        validate(self.document)

        def walk(node):
            if isinstance(node, dict):
                if '$ref' in node:
                    target = self.document
                    self.assertTrue(node['$ref'].startswith('#/'))
                    for part in node['$ref'][2:].split('/'):
                        target = target[part]
                for value in node.values():
                    walk(value)
            elif isinstance(node, list):
                for value in node:
                    walk(value)

        walk(self.document)

    def test_documented_examples_are_accepted(self):
        for case in self.cases:
            with self.subTest(case=case['name']):
                self.validator(case['schema']).validate(case['value'])

    def duration(self):
        return copy.deepcopy(next(c['value'] for c in self.cases if c['name'] == 'duration_create'))

    def test_identity_response_requires_both_scope_ids(self):
        self.assertIn('/v1/me', self.document['paths'])
        value = {'tenant_id': '00000000-0000-4000-8000-000000000001',
                 'person_id': '00000000-0000-4000-8000-000000000002'}
        self.validator('Identity').validate(value)
        del value['tenant_id']
        self.assertFalse(self.validator('Identity').is_valid(value))

    def test_duration_only_rejects_invented_timestamps(self):
        value = self.duration()
        value['started_at'] = '2026-10-26T09:03:00Z'
        self.assertFalse(self.validator('EntryCreate').is_valid(value))

    def test_client_cannot_assign_ownership_or_export_status(self):
        for field, content in [('person_id', '00000000-0000-4000-8000-000000000001'),
                               ('effective_seconds', 900), ('export_state', 'exported'),
                               ('version', 9)]:
            value = self.duration()
            value[field] = content
            with self.subTest(field=field):
                self.assertFalse(self.validator('EntryCreate').is_valid(value))

    def test_bad_dates_and_durations_are_rejected(self):
        for field, content in [('entry_date', '2026-02-30'), ('original_seconds', -1),
                               ('original_seconds', 1.5), ('entry_id', 'not-a-uuid')]:
            value = self.duration()
            value[field] = content
            with self.subTest(field=field):
                self.assertFalse(self.validator('EntryCreate').is_valid(value))

    def test_timed_entries_require_both_endpoints(self):
        value = copy.deepcopy(next(c['value'] for c in self.cases if c['name'] == 'timed_create'))
        del value['stopped_at']
        self.assertFalse(self.validator('EntryCreate').is_valid(value))

    def test_both_rounding_modes_are_supported(self):
        for interval in (None, 5):
            settings = {'rounding_interval_minutes': interval, 'weekly_target_hours': 35,
                        'visual_indicator': 'progress'}
            self.validator('SettingsWrite').validate(settings)

    def test_nonpositive_target_is_rejected(self):
        value = {'rounding_interval_minutes': None, 'weekly_target_hours': 0,
                 'visual_indicator': 'progress'}
        self.assertFalse(self.validator('SettingsWrite').is_valid(value))

    def test_settings_defaults_and_independent_fields(self):
        fields = self.document['components']['schemas']['SettingsWrite']['properties']
        self.assertEqual(fields['rounding_interval_minutes']['default'], 5)
        self.assertEqual(fields['weekly_target_hours']['default'], 35)
        self.assertEqual(fields['visual_indicator']['default'], 'progress')
        for interval in (None, 5):
            for indicator in ('progress', 'glass'):
                self.validator('SettingsWrite').validate({
                    'rounding_interval_minutes': interval,
                    'weekly_target_hours': 32.5,
                    'visual_indicator': indicator})

    def test_concurrency_errors_have_actionable_codes(self):
        responses = self.document['paths']['/v1/me/entries/{entry_id}']['put']['responses']
        for status in ('401', '404', '409', '412', '422', '428'):
            self.assertIn(status, responses)
        schema = self.document['components']['schemas']['Error']['properties']['code']
        self.assertIn('version_conflict', schema['enum'])
        self.assertIn('precondition_required', schema['enum'])


if __name__ == '__main__':
    unittest.main()
