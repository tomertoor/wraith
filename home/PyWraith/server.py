"""PyWraith TCP server for receiving connections from wraith agents."""

import socket
import threading
import time
import uuid
import logging
from typing import Optional, Callable, Dict, Any, List
from dataclasses import dataclass, field
from enum import Enum

from PyWraith.protocol import WraithProtocol
from PyWraith.proto_gen import wraith_pb2 as pb


logger = logging.getLogger(__name__)


class ConnectionState(Enum):
    PENDING = "pending"
    REGISTERED = "registered"
    DISCONNECTED = "disconnected"


@dataclass
class AgentConnection:
    """Represents a connected wraith agent."""
    socket: socket.socket
    address: tuple
    agent_id: str = field(default_factory=lambda: str(uuid.uuid4()))
    state: ConnectionState = ConnectionState.PENDING
    hostname: str = ""
    username: str = ""
    ip_address: str = ""
    registered_at: float = field(default_factory=time.time)
    last_heartbeat: float = field(default_factory=time.time)


class WraithServer:
    """TCP server that listens for incoming wraith agent connections."""

    def __init__(self, host: str = "0.0.0.0", port: int = 4445):
        self.host = host
        self.port = port
        self.socket: Optional[socket.socket] = None
        self.running = False
        self.agents: Dict[str, AgentConnection] = {}
        self._lock = threading.Lock()
        self._accept_thread: Optional[threading.Thread] = None
        self._heartbeat_thread: Optional[threading.Thread] = None
        self._command_handlers: Dict[str, Callable] = {}

    def set_command_handler(self, action: str, handler: Callable):
        """Set a handler function for a command action."""
        self._command_handlers[action] = handler

    def start(self) -> bool:
        """Start listening for connections."""
        try:
            self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            self.socket.bind((self.host, self.port))
            self.socket.listen(5)
            self.socket.settimeout(1.0)
            self.running = True

            self._accept_thread = threading.Thread(target=self._accept_loop, daemon=True)
            self._accept_thread.start()

            self._heartbeat_thread = threading.Thread(target=self._heartbeat_loop, daemon=True)
            self._heartbeat_thread.start()

            logger.info(f"Server listening on {self.host}:{self.port}")
            return True
        except Exception as e:
            logger.error(f"Failed to start server: {e}")
            return False

    def stop(self):
        """Stop the server and disconnect all agents."""
        self.running = False

        with self._lock:
            for agent in self.agents.values():
                try:
                    agent.socket.close()
                except Exception:
                    pass
            self.agents.clear()

        if self.socket:
            try:
                self.socket.close()
            except Exception:
                pass
            self.socket = None

        logger.info("Server stopped")

    def _accept_loop(self):
        """Accept incoming connections."""
        while self.running:
            try:
                client_socket, address = self.socket.accept()
                client_socket.settimeout(30.0)
                agent = AgentConnection(socket=client_socket, address=address)
                with self._lock:
                    self.agents[agent.agent_id] = agent

                thread = threading.Thread(
                    target=self._handle_agent,
                    args=(agent.agent_id,),
                    daemon=True
                )
                thread.start()
                logger.info(f"Agent connected from {address}")
            except socket.timeout:
                continue
            except Exception as e:
                if self.running:
                    logger.error(f"Accept error: {e}")

    def _heartbeat_loop(self):
        """Check for stale connections."""
        while self.running:
            try:
                time.sleep(10)
                with self._lock:
                    now = time.time()
                    stale = []
                    for agent_id, agent in self.agents.items():
                        if now - agent.last_heartbeat > 60:
                            if agent.state != ConnectionState.DISCONNECTED:
                                logger.info(f"Agent {agent_id} heartbeat timeout")
                            stale.append(agent_id)

                    for agent_id in stale:
                        self._disconnect_agent(agent_id)
            except Exception:
                pass

    def _handle_agent(self, agent_id: str):
        """Handle messages from an agent."""
        agent = None
        with self._lock:
            agent = self.agents.get(agent_id)

        if not agent:
            return

        try:
            while self.running:
                data = WraithProtocol.read_frame(agent.socket)
                if not data:
                    with self._lock:
                        self._disconnect_agent(agent_id)
                    break

                msg = WraithProtocol.decode_message(data)
                self._process_message(agent, msg)
        except Exception as e:
            logger.error(f"Agent {agent_id} error: {e}")
            with self._lock:
                self._disconnect_agent(agent_id)

    def _process_message(self, agent: AgentConnection, msg: pb.WraithMessage):
        """Process a message from an agent."""
        if msg.msg_type == pb.REGISTRATION:
            agent.hostname = msg.registration.hostname
            agent.username = msg.registration.username
            agent.ip_address = msg.registration.ip_address
            agent.state = ConnectionState.REGISTERED
            agent.last_heartbeat = time.time()
            logger.info(f"Agent registered: {agent.username}@{agent.hostname}")

        elif msg.msg_type == pb.HEARTBEAT:
            agent.last_heartbeat = time.time()
            agent.state = ConnectionState.REGISTERED

        elif msg.msg_type == pb.COMMAND_RESULT:
            logger.info(f"Command result from {agent.agent_id}: {msg.message_id}")

        elif msg.msg_type == pb.COMMAND:
            command_id = msg.message_id
            action = msg.command.action
            params = dict(msg.command.params)
            timeout = msg.command.timeout

            logger.info(f"Command received: {action} (id: {command_id})")

            handler = self._command_handlers.get(action)
            if handler:
                try:
                    result = handler(params)
                    if result is None:
                        result = {"status": "ok", "output": ""}
                    elif isinstance(result, str):
                        result = {"status": "ok", "output": result}
                except Exception as e:
                    result = {"status": "error", "error": str(e)}
            else:
                result = {"status": "error", "error": f"Unknown action: {action}"}

            response = WraithProtocol.create_command_result(
                command_id=command_id,
                status=result.get("status", "ok"),
                output=result.get("output", ""),
                exit_code=0 if result.get("status") == "ok" else 1,
                duration_ms=int((time.time() - msg.timestamp / 1000) * 1000),
                error=result.get("error", "")
            )
            agent.socket.sendall(WraithProtocol.encode_message(response))

    def _disconnect_agent(self, agent_id: str):
        """Disconnect an agent."""
        agent = self.agents.get(agent_id)
        if agent:
            try:
                agent.socket.close()
            except Exception:
                pass
            agent.state = ConnectionState.DISCONNECTED
            logger.info(f"Agent {agent_id} disconnected")

        if agent_id in self.agents:
            del self.agents[agent_id]

    def send_command(
        self,
        agent_id: str,
        action: str,
        params: Optional[Dict[str, str]] = None,
        timeout: int = 30
    ) -> tuple[bool, Dict[str, Any]]:
        """Send a command to a specific agent."""
        with self._lock:
            agent = self.agents.get(agent_id)

        if not agent or agent.state != ConnectionState.REGISTERED:
            return False, {"error": "Agent not found or not registered"}

        command_id = str(uuid.uuid4())
        msg = WraithProtocol.create_command(command_id, action, params, timeout)
        data = WraithProtocol.encode_message(msg)

        try:
            agent.socket.sendall(data)

            start = time.time()
            while time.time() - start < timeout:
                response_data = WraithProtocol.read_frame(agent.socket)
                if response_data:
                    response = WraithProtocol.decode_message(response_data)
                    if response.msg_type == pb.COMMAND_RESULT:
                        return True, WraithProtocol.parse_command_result(response)
                time.sleep(0.1)

            return False, {"error": "Timeout waiting for response"}
        except Exception as e:
            return False, {"error": str(e)}

    def list_agents(self) -> List[Dict[str, Any]]:
        """List all connected agents."""
        with self._lock:
            return [
                {
                    "agent_id": agent.agent_id,
                    "hostname": agent.hostname,
                    "username": agent.username,
                    "ip_address": agent.ip_address,
                    "state": agent.state.value,
                    "registered_at": agent.registered_at,
                    "last_heartbeat": agent.last_heartbeat,
                }
                for agent in self.agents.values()
            ]
