"""Tests for CI fault capture without starting a guest."""

import json
from pathlib import Path
import socket
import tempfile
import threading
import unittest

from QemuFaultCapture import QemuFaultCapture


class FaultCaptureTests(unittest.TestCase):
    def test_prepare_arguments_preserves_other_devices(self):
        arguments = ["-device", "nvme,drive=disk", "-device", "isa-debug-exit,iobase=0xf4", "-m", "8192"]
        prepared = QemuFaultCapture.prepare_arguments(arguments, "/tmp/test-qmp")
        self.assertEqual(prepared, ["-device", "nvme,drive=disk", "-m", "8192",
                                    "-qmp", "unix:/tmp/test-qmp,server=on,wait=off"])

    def test_fault_captures_original_stack_on_all_cpus(self):
        with tempfile.TemporaryDirectory() as directory:
            capture = QemuFaultCapture(directory)
            server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            server.bind(capture.socket_path)
            server.listen(1)
            server.settimeout(5)
            requests = []
            errors = []

            def serve():
                try:
                    connection, _ = server.accept()
                    connection.settimeout(5)
                    with connection, connection.makefile("rwb", buffering=0) as stream:
                        stream.write(b'{"QMP": {}}\n')
                        while True:
                            request = json.loads(stream.readline())
                            requests.append(request)
                            result = [{"cpu-index": 0}, {"cpu-index": 1}] if request['execute'] == 'query-cpus-fast' else "ok"
                            stream.write(b'{"event": "STOP"}\n')
                            stream.write(json.dumps({"return": result}).encode() + b"\n")
                            if request['execute'] == 'quit':
                                break
                except Exception as error:
                    errors.append(error)

            worker = threading.Thread(target=serve)
            worker.start()
            try:
                capture.write("ERROR - EXCEPTION: PAGE FAULT\nAccessed Address: 0x27E902048\n")
                capture.write("Instruction Pointer: 0x27F5DC056\nStack Point")
                capture.write("er: 0x27E0169A8\nDumping Exception Stack Trace:\n")
                capture.write("panicked at handler\n")
                worker.join(5)
                self.assertFalse(worker.is_alive())
                self.assertEqual(errors, [])
                self.assertTrue(capture.failed)
                records = json.loads((Path(directory) / "fault.json").read_text())
                self.assertEqual(records['addresses']['Stack Pointer'], 0x27E0169A8)
                commands = [request['arguments']['command-line'] for request in requests if 'arguments' in request]
                self.assertEqual(commands.count('x /64gx 0x27e0169a8'), 2)
                self.assertEqual(sum(request['execute'] == 'quit' for request in requests), 1)
            finally:
                capture.close()
                server.close()
                worker.join(5)

    def test_timeout_records_missing_qmp_socket(self):
        with tempfile.TemporaryDirectory() as directory:
            capture = QemuFaultCapture(directory)
            try:
                capture.collect()
                result = json.loads((Path(directory) / "fault.json").read_text())
                self.assertFalse(result['fault_seen'])
                self.assertIn('error', result)
                self.assertTrue(capture.failed)
            finally:
                capture.close()


if __name__ == '__main__':
    unittest.main()