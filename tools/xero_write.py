"""Run a bounded write test against the selected Xero demo tenant."""

import datetime
import json
import uuid

from xero_demo import SafeError, request_json


BASE = 'https://api.xero.com/projects.xro/2.0'


def identifier(value):
    try:
        return str(uuid.UUID(value))
    except (ValueError, TypeError, AttributeError):
        raise SafeError('Xero returned an invalid record ID.') from None


class Api:
    def __init__(self, headers):
        self.headers = headers

    def call(self, path, method='GET', payload=None, key=None):
        headers = dict(self.headers)
        data = None
        if payload is not None:
            headers['Content-Type'] = 'application/json'
            data = json.dumps(payload, sort_keys=True).encode()
        if key:
            headers['Idempotency-Key'] = key
        return request_json(BASE + path, headers, data, method)

    def items(self, path):
        result = []
        for page in range(1, 21):
            data = self.call(f'{path}?page={page}&pageSize=100')
            if not isinstance(data, dict) or not isinstance(data.get('items'), list):
                raise SafeError('Xero returned an invalid list.')
            result.extend(data['items'])
            if len(data['items']) < 100:
                return result
        raise SafeError('The demo exceeded the page limit. No incomplete list will be used.')


def unique_named(items, name, key):
    matches = [item for item in items if item.get('name') == name]
    if len(matches) != 1:
        raise SafeError(f'Exactly one {name} record is required. Check the demo project.')
    return identifier(matches[0].get(key))


def verify_entry(entry, payload):
    if not isinstance(entry, dict) or any(entry.get(k) != payload[k]
                                        for k in ('userId', 'taskId', 'duration', 'description')):
        raise SafeError('The saved test entry does not match the requested values.')


def exercise(api, project_id, task_id, user_id, marker):
    path = f'/Projects/{identifier(project_id)}/Time'
    payload = {'userId': identifier(user_id), 'taskId': identifier(task_id),
               'dateUtc': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT00:00:00Z'),
               'duration': 15, 'description': marker}
    key = str(uuid.uuid4())
    print(f'Test label: {marker}')
    print('If this process stops, remove only entries with this exact label from the demo project.')
    attempted = False
    try:
        attempted = True
        first = api.call(path, 'POST', payload, key)
        first_id = identifier(first.get('timeEntryId'))
        verify_entry(api.call(path + '/' + first_id), payload)
        print('PASS: Created and read a 15-minute entry.')
        # Discard the create result to model a client that lost the response.
        first = None
        replay = api.call(path, 'POST', payload, key)
        replay_id = identifier(replay.get('timeEntryId'))
        matches = [item for item in api.items(path) if item.get('description') == marker]
        if replay_id != first_id or len(matches) != 1 or matches[0].get('timeEntryId') != first_id:
            raise SafeError('The repeated create request did not resolve to one entry.')
        print('PASS: Replayed the create request with the same key. Exactly one test entry exists.')
        changed = dict(payload, duration=20)
        api.call(path + '/' + first_id, 'PUT', changed, str(uuid.uuid4()))
        verify_entry(api.call(path + '/' + first_id), changed)
        print('PASS: Changed the duration to 20 minutes and read it back.')
    finally:
        if attempted:
            try:
                matches = [item for item in api.items(path) if item.get('description') == marker]
                for item in matches:
                    if item.get('taskId') != task_id or item.get('userId') != user_id:
                        raise SafeError('A test label matched unexpected ownership. Cleanup stopped.')
                    api.call(path + '/' + identifier(item.get('timeEntryId')), 'DELETE')
                if any(item.get('description') == marker for item in api.items(path)):
                    raise SafeError('A test entry remains after deletion.')
                print('PASS: Cleanup confirmed. No entries with this test label remain.')
            except Exception:
                print(f'CLEANUP NEEDS ATTENTION: Check the exact test label {marker} in Xero.')
                raise SafeError('Cleanup could not be confirmed. Do not rerun until the demo is checked.') from None


def run_write_test(headers):
    api = Api(headers)
    project = unique_named(api.items('/Projects'), 'New Website', 'projectId')
    task = unique_named(api.items(f'/Projects/{project}/Tasks'), 'Development', 'taskId')
    users = api.items('/ProjectsUsers')
    if not users:
        raise SafeError('The demo has no Projects users.')
    print('Select the Projects user that represents your signed-in account:')
    for index, user in enumerate(users, 1):
        print(f"{index}: {user.get('name', '(unnamed)')} ({user.get('email', '')})")
    print('Enter 0 if none represents your account. The test can still check a sample user.')
    try:
        choice = int(input('User number: '))
    except ValueError:
        raise SafeError('Enter a listed user number.') from None
    if not 0 <= choice <= len(users):
        raise SafeError('The user number is outside the list.')
    selected = users[choice - 1] if choice else users[0]
    run_id = str(uuid.uuid4())
    exercise(api, project, task, selected['userId'], 'Trackfast demo test ' + run_id + ' primary')
    others = [user for user in users if user.get('userId') != selected.get('userId')]
    if others:
        exercise(api, project, task, others[0]['userId'], 'Trackfast demo test ' + run_id + ' second-user')
        print('PASS: The same connection wrote time for two distinct Projects user IDs.')
    else:
        print('NOT TESTED: A second Projects user is unavailable.')
    print('The user selection was manual. Production roles and invoiced-entry behaviour remain untested.')
