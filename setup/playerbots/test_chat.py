#!/usr/bin/env python3
"""Native chat queue/holder and deterministic local HTTP generation regression.
All modes reach native handler entry; whisper/party also run their real handlers.
Full guild/channel permission and in-world gameplay tests still need game data.
"""
import http.server
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import threading
import time
from test_runtime import compile_fixture

class Mock(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', 0)))
        # Alternate immediate and slow generation; output delays must remain unchanged.
        self.server.requests += 1
        if self.server.requests % 2 == 0: time.sleep(0.2)
        body = b'{"message":{"role":"assistant","content":"PONG."},"done":true}'
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args): pass

if __name__ == '__main__':
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Mock)
    server.requests = 0
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix='playerbots-chat-') as temp:
            output = Path(temp) / 'chat-test'
            compile_fixture(Path(sys.argv[1]).resolve(), output,
                fixture=Path(__file__).resolve().parent / 'tests/chat.cpp',
                link_flags=['-rdynamic', '-Wl,--wrap=_ZN12WorldSession23HandleChatMessageOpcodeERKN12WorldPackets4Chat11ChatMessageE', '-Wl,--wrap=_ZN12WorldSession10SendPacketEPK11WorldPacket'])
            result = subprocess.run([str(output), str(server.server_port)], cwd=temp, timeout=60)
            if result.returncode and shutil.which('gdb'):
                subprocess.run(['gdb', '--batch', '-ex', 'run', '-ex', 'bt', '--args',
                                str(output), str(server.server_port)], cwd=temp, timeout=90)
            result.check_returncode()
    finally:
        server.shutdown()
        server.server_close()
