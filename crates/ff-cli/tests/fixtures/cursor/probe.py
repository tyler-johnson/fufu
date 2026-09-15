"""Temporary command hook used for the authenticated CLI capture. Set FF_CURSOR_CAPTURE to the output JSONL path; wire this script on each candidate event and invoke it with --shell from the client tool."""
import json
import os
import sys

shell = '--shell' in sys.argv
payload = {} if shell else json.load(sys.stdin)
if 'user_email' in payload:
    payload['user_email'] = '<redacted>'
names = ['CURSOR_AGENT', 'CURSOR_CONVERSATION_ID', 'CURSOR_REQUEST_ID',
         'CURSOR_PLUGIN_ROOT', 'CLAUDE_PLUGIN_ROOT', 'PLUGIN_ROOT',
         'FF_SESSION', 'PWD', 'HOME']
record = {'kind': 'shell' if shell else 'hook', 'stdin': payload,
          'env': {name: os.environ[name] for name in names if name in os.environ},
          'cwd': os.getcwd()}
with open(os.environ['FF_CURSOR_CAPTURE'], 'a') as capture:
    capture.write(json.dumps(record) + '\n')
if shell:
    print(json.dumps(record))
elif payload.get('hook_event_name') == 'sessionStart':
    print(json.dumps({'additional_context': 'fufu probe notice: the violet kestrel landed on runway 158.'}))
else:
    print('{}')
