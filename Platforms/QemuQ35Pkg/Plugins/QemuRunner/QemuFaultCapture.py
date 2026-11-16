"""Opt-in, bounded QMP diagnostics for firmware exceptions."""

import json
import logging
from pathlib import Path
import re
import socket
import tempfile
import threading


class QemuFaultCapture:
    """Capture the original exception stack before terminating a failed guest."""

    def __init__(self, directory, timeout=600):
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="patina-qmp-")
        self.socket_path = str(Path(self.temporary.name) / "qmp.sock")
        self.output = (self.directory / "guest.log").open("w")
        self.pending = ""
        self.addresses = {}
        self.fault_seen = False
        self.failed = False
        self.finished = False
        self.lock = threading.Lock()
        self.timer = threading.Timer(timeout, self.collect)
        self.timer.daemon = True

    @staticmethod
    def prepare_arguments(arguments, socket_path):
        """Keep panic output available by removing only the debug-exit device."""
        prepared = []
        position = 0
        while position < len(arguments):
            if (arguments[position] == "-device" and position + 1 < len(arguments)
                    and arguments[position + 1].split(",", 1)[0] == "isa-debug-exit"):
                position += 2
            else:
                prepared.append(arguments[position])
                position += 1
        prepared.extend(["-qmp", f"unix:{socket_path},server=on,wait=off"])
        return prepared

    def start(self):
        """Start the diagnostic run deadline."""
        self.timer.start()

    def write(self, text):
        """Mirror guest output and collect once the fault registers are printed."""
        self.output.write(text)
        self.output.flush()
        self.pending += text
        while "\n" in self.pending:
            line, self.pending = self.pending.split("\n", 1)
            if "EXCEPTION:" in line:
                self.fault_seen = True
            if self.fault_seen:
                for label in ("Accessed Address", "Instruction Pointer", "Stack Pointer"):
                    match = re.search(rf"{label}:\s*(0x[0-9a-fA-F]+)", line)
                    if match:
                        self.addresses[label] = int(match.group(1), 16)
                if "Dumping Exception Stack Trace" in line or "panicked at" in line:
                    self.collect()

    def flush(self):
        self.output.flush()

    @staticmethod
    def receive(stream):
        while True:
            line = stream.readline()
            if not line:
                raise ConnectionError("QMP disconnected")
            message = json.loads(line)
            if "event" not in message:
                return message

    def collect(self):
        """Stop all CPUs, save bounded stack/code reads, and terminate QEMU."""
        with self.lock:
            if self.finished:
                return
            self.finished = True
            self.failed = True
            records = {"fault_seen": self.fault_seen, "addresses": self.addresses.copy(), "commands": []}
            connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            connection.settimeout(10)
            try:
                connection.connect(self.socket_path)
                with connection.makefile("rwb", buffering=0) as stream:
                    records["greeting"] = self.receive(stream)

                    def execute(command, arguments=None):
                        request = {"execute": command}
                        if arguments is not None:
                            request["arguments"] = arguments
                        stream.write(json.dumps(request).encode() + b"\n")
                        response = self.receive(stream)
                        records["commands"].append({"request": request, "response": response})
                        return response.get("return")

                    def monitor(command):
                        return execute("human-monitor-command", {"command-line": command})

                    execute("qmp_capabilities")
                    execute("stop")
                    execute("query-version")
                    execute("query-status")
                    cpus = execute("query-cpus-fast") or []
                    for cpu in cpus:
                        monitor(f"cpu {cpu['cpu-index']}")
                        monitor("info registers")
                        for label, address in self.addresses.items():
                            monitor(f"gva2gpa {address:#x}")
                            if label == "Stack Pointer":
                                monitor(f"x /64gx {address:#x}")
                            elif label == "Instruction Pointer":
                                monitor(f"x /32bx {address:#x}")
                    execute("quit")
            except (OSError, ValueError, ConnectionError) as error:
                records["error"] = str(error)
                logging.error("QMP fault capture failed: %s", error)
            finally:
                connection.close()
                (self.directory / "fault.json").write_text(json.dumps(records, indent=2))

    def close(self):
        """Cancel the deadline and release capture files and the QMP socket."""
        self.timer.cancel()
        with self.lock:
            self.finished = True
        if self.timer.ident is not None:
            self.timer.join()
        self.output.close()
        self.temporary.cleanup()