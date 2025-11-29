#!/usr/bin/env python3

# TODO: create pool of workers to handle requests concurrently
#       the socket should be a listener socket and each worker should accept connections from it
#       the socket should accept requests as newline-separated JSON objects
#       there should be a watchdog to monitor worker health and restart them if they die
#       graceful shutdown should be implemented for the workers
#       optional logging of requests and responses
#       use systemd notify to signal readiness and amount of connections handled

import json
import os
from socket import AF_UNIX, SOCK_DGRAM, SOCK_STREAM, fromfd, socket
from multiprocessing import Pool


def get_listener_from_systemd() -> socket:
    listen_fds = int(os.getenv("LISTEN_FDS", "0"))
    listen_pid = int(os.getenv("LISTEN_PID", "0"))
    if listen_fds != 1 or listen_pid != os.getpid():
        raise RuntimeError("No socket passed from systemd")
    assert listen_fds == 1
    sock = fromfd(3, AF_UNIX, SOCK_STREAM)
    sock.setblocking(False)
    return sock


def get_notify_socket_from_systemd() -> socket:
    notify_socket_path = os.getenv("NOTIFY_SOCKET")
    if not notify_socket_path:
        raise RuntimeError("No notify socket path found in environment")
    sock = socket(AF_UNIX, SOCK_DGRAM)
    sock.connect(notify_socket_path)
    return sock


def run_auth_daemon(sock: socket):
    sock.listen()
    print("Auth daemon is running and listening for connections...")
    with Pool() as worker_pool:
        with get_notify_socket_from_systemd() as notify_socket:
            notify_socket.sendall(b"READY=1\n")
        while True:
            conn, _ = sock.accept()
            worker_pool.apply_async(session_handler, args=(conn,))


def session_handler(sock: socket):
    buffer = ""
    while True:
        data = sock.recv(4096).decode("utf-8")
        if not data:
            print("Connection closed by client")
            break
        buffer += data
        if buffer.endswith("\n"):
            requests = buffer.strip().split("\n")
            buffer = ""
            for request in requests:
                try:
                    req_json = json.loads(request)
                    username = req_json.get("username", "")
                    groups = req_json.get("groups", [])
                    resource_type = req_json.get("resource_type", "")
                    resource = req_json.get("resource", "")
                    allowed = process_request(username, groups, resource_type, resource)
                    response = {"allowed": allowed}
                except json.JSONDecodeError:
                    response = {"error": "Invalid JSON"}
                sock.sendall((json.dumps(response) + "\n").encode("utf-8"))


def process_request(
    username: str,
    groups: list[str],
    resource_type: str,
    resource: str,
) -> bool:
    ...


if __name__ == "__main__":
    listener_socket = get_listener_from_systemd()
    run_auth_daemon(listener_socket)
