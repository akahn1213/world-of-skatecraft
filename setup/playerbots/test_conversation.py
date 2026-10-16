#!/usr/bin/env python3
"""Asset-free native context/history and outgoing HTTP prompt regression."""
import http.server
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from test_runtime import compile_fixture

class Mock(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.prompts.append(request['messages'][0]['content'])
        body = b'{"message":{"content":"I am looking for clues."}}'
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args): pass

if __name__ == '__main__':
    with http.server.ThreadingHTTPServer(('127.0.0.1', 0), Mock) as server:
        server.prompts = []
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            with tempfile.TemporaryDirectory(prefix='playerbots-conversation-') as temp:
                binary = Path(temp) / 'conversation-test'
                compile_fixture(Path(sys.argv[1]).resolve(), binary,
                                fixture=Path(__file__).resolve().parent / 'tests/conversation.cpp')
                subprocess.run([str(binary), str(server.server_port)], cwd=temp, check=True, timeout=60)
            assert len(server.prompts) == 1
            prompt = server.prompts[0]
            for expected in ['Earlier question é猫 "quoted" <bot name>', 'Delivered answer',
                             'Testing é猫', '2/3', 'What now?', 'not necessarily current goals']:
                assert expected in prompt, expected
            assert 'Old party' not in prompt and 'Public hello' not in prompt
            print('Mock endpoint received valid JSON with verified state and scoped conversational history.')
        finally:
            server.shutdown()
