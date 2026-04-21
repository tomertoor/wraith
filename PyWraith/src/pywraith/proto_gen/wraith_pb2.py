"""Generated protobuf code for wraith protocol."""


# Message types
REGISTRATION = 0
HEARTBEAT = 1
COMMAND = 2
COMMAND_RESULT = 3
RELAY_CREATE = 4
RELAY_DELETE = 5
RELAY_LIST = 6
RELAY_LIST_RESPONSE = 7


# =============================================================================
# Protobuf encoding/decoding utilities
# =============================================================================

def _encode_varint(value):
    """Encode an integer as a varint."""
    result = []
    while value > 0x7f:
        result.append((value & 0x7f) | 0x80)
        value >>= 7
    result.append(value & 0x7f)
    return bytes(result)


def _decode_varint(data, pos):
    """Decode a varint, returning (value, new_pos)."""
    result = 0
    shift = 0
    while True:
        b = data[pos]
        pos += 1
        result |= (b & 0x7f) << shift
        if not (b & 0x80):
            break
        shift += 7
    return result, pos


def _encode_tag(field_number, wire_type):
    """Encode a protobuf tag."""
    return _encode_varint((field_number << 3) | wire_type)


def _decode_tag(data):
    """Decode a protobuf tag, returning (tag, wire_type)."""
    tag, pos = _decode_varint(data, 0)
    return tag, tag & 0x7


def _varint_size(value):
    """Return the number of bytes needed to encode a varint."""
    if value < 0:
        return 10  # max varint size for signed encoding
    result = 1
    while value > 0x7f:
        result += 1
        value >>= 7
    return result


def _encode_string(value):
    """Encode a string as length-delimited."""
    encoded = value.encode('utf-8')
    return _encode_varint(len(encoded)) + encoded


def _decode_string(data, pos):
    """Decode a length-delimited string, returning (string, new_pos)."""
    length, pos = _decode_varint(data, pos)
    return data[pos:pos+length].decode('utf-8'), pos + length


def _skip_field(data, pos, wire_type):
    """Skip a field given its wire type, return new position."""
    if wire_type == 0:  # Varint
        _, pos = _decode_varint(data, pos)
    elif wire_type == 2:  # Length-delimited
        length, pos = _decode_varint(data, pos)
        pos += length
    elif wire_type == 5:  # 32-bit
        pos += 4
    elif wire_type == 1:  # 64-bit
        pos += 8
    else:
        # Unknown wire type, skip remaining
        pos = len(data)
    return pos


# =============================================================================
# Registration message
# =============================================================================

class Registration:
    """Registration message for agent startup."""
    def __init__(self, hostname='', username='', os='', ip_address=''):
        self.hostname = hostname
        self.username = username
        self.os = os
        self.ip_address = ip_address

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        result = []
        if self.hostname:
            result.append(_encode_tag(1, 2) + _encode_string(self.hostname))
        if self.username:
            result.append(_encode_tag(2, 2) + _encode_string(self.username))
        if self.os:
            result.append(_encode_tag(3, 2) + _encode_string(self.os))
        if self.ip_address:
            result.append(_encode_tag(4, 2) + _encode_string(self.ip_address))
        return b''.join(result)

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 2:
                msg.hostname, pos = _decode_string(data, pos)
            elif tag_field == 2 and tag_wire == 2:
                msg.username, pos = _decode_string(data, pos)
            elif tag_field == 3 and tag_wire == 2:
                msg.os, pos = _decode_string(data, pos)
            elif tag_field == 4 and tag_wire == 2:
                msg.ip_address, pos = _decode_string(data, pos)
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg


# =============================================================================
# Heartbeat message
# =============================================================================

class Heartbeat:
    """Heartbeat message for keep-alive."""
    def __init__(self, last_command_time=0, status=''):
        self.last_command_time = last_command_time
        self.status = status

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        result = []
        if self.last_command_time != 0:
            result.append(_encode_tag(1, 0) + _encode_varint(self.last_command_time))
        if self.status:
            result.append(_encode_tag(2, 2) + _encode_string(self.status))
        return b''.join(result)

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 0:
                msg.last_command_time, pos = _decode_varint(data, pos)
            elif tag_field == 2 and tag_wire == 2:
                msg.status, pos = _decode_string(data, pos)
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg


# =============================================================================
# Command message
# =============================================================================

class Command:
    """Command message for agent instructions."""
    def __init__(self, command_id='', action='', params=None, timeout=0):
        self.command_id = command_id
        self.action = action
        self.params = params or {}
        self.timeout = timeout

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        result = []
        if self.command_id:
            result.append(_encode_tag(1, 2) + _encode_string(self.command_id))
        if self.action:
            result.append(_encode_tag(2, 2) + _encode_string(self.action))
        if self.params:
            for k, v in self.params.items():
                result.append(_encode_tag(3, 2) + _encode_string(f"{k}={v}"))
        if self.timeout != 0:
            result.append(_encode_tag(4, 0) + _encode_varint(self.timeout))
        return b''.join(result)

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 2:
                msg.command_id, pos = _decode_string(data, pos)
            elif tag_field == 2 and tag_wire == 2:
                msg.action, pos = _decode_string(data, pos)
            elif tag_field == 3 and tag_wire == 2:
                param_str, pos = _decode_string(data, pos)
                if '=' in param_str:
                    k, v = param_str.split('=', 1)
                    msg.params[k] = v
            elif tag_field == 4 and tag_wire == 0:
                msg.timeout, pos = _decode_varint(data, pos)
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg


# =============================================================================
# CommandResult message
# =============================================================================

class CommandResult:
    """Command result message for agent responses."""
    def __init__(self, command_id='', status='', output='', exit_code=0, duration_ms=0, error=''):
        self.command_id = command_id
        self.status = status
        self.output = output
        self.exit_code = exit_code
        self.duration_ms = duration_ms
        self.error = error

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        result = []
        if self.command_id:
            result.append(_encode_tag(1, 2) + _encode_string(self.command_id))
        if self.status:
            result.append(_encode_tag(2, 2) + _encode_string(self.status))
        if self.output:
            result.append(_encode_tag(3, 2) + _encode_string(self.output))
        if self.exit_code != 0:
            result.append(_encode_tag(4, 0) + _encode_varint(self.exit_code))
        if self.duration_ms != 0:
            result.append(_encode_tag(5, 0) + _encode_varint(self.duration_ms))
        if self.error:
            result.append(_encode_tag(6, 2) + _encode_string(self.error))
        return b''.join(result)

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 2:
                msg.command_id, pos = _decode_string(data, pos)
            elif tag_field == 2 and tag_wire == 2:
                msg.status, pos = _decode_string(data, pos)
            elif tag_field == 3 and tag_wire == 2:
                msg.output, pos = _decode_string(data, pos)
            elif tag_field == 4 and tag_wire == 0:
                msg.exit_code, pos = _decode_varint(data, pos)
            elif tag_field == 5 and tag_wire == 0:
                msg.duration_ms, pos = _decode_varint(data, pos)
            elif tag_field == 6 and tag_wire == 2:
                msg.error, pos = _decode_string(data, pos)
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg


# =============================================================================
# RelayCreate message
# =============================================================================

class RelayCreate:
    """Relay create message."""
    def __init__(self, relay_id='', listen_host='', listen_port=0, forward_host='', forward_port=0):
        self.relay_id = relay_id
        self.listen_host = listen_host
        self.listen_port = listen_port
        self.forward_host = forward_host
        self.forward_port = forward_port

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        result = []
        if self.relay_id:
            result.append(_encode_tag(1, 2) + _encode_string(self.relay_id))
        if self.listen_host:
            result.append(_encode_tag(2, 2) + _encode_string(self.listen_host))
        if self.listen_port != 0:
            result.append(_encode_tag(3, 0) + _encode_varint(self.listen_port))
        if self.forward_host:
            result.append(_encode_tag(4, 2) + _encode_string(self.forward_host))
        if self.forward_port != 0:
            result.append(_encode_tag(5, 0) + _encode_varint(self.forward_port))
        return b''.join(result)

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 2:
                msg.relay_id, pos = _decode_string(data, pos)
            elif tag_field == 2 and tag_wire == 2:
                msg.listen_host, pos = _decode_string(data, pos)
            elif tag_field == 3 and tag_wire == 0:
                msg.listen_port, pos = _decode_varint(data, pos)
            elif tag_field == 4 and tag_wire == 2:
                msg.forward_host, pos = _decode_string(data, pos)
            elif tag_field == 5 and tag_wire == 0:
                msg.forward_port, pos = _decode_varint(data, pos)
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg


# =============================================================================
# RelayDelete message
# =============================================================================

class RelayDelete:
    """Relay delete message."""
    def __init__(self, relay_id=''):
        self.relay_id = relay_id

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        if self.relay_id:
            return _encode_tag(1, 2) + _encode_string(self.relay_id)
        return b''

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 2:
                msg.relay_id, pos = _decode_string(data, pos)
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg


# =============================================================================
# RelayList message
# =============================================================================

class RelayList:
    """Relay list request message (empty body)."""
    def __init__(self):
        pass

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        return b''

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        return cls()


# =============================================================================
# RelayInfo message
# =============================================================================

class RelayInfo:
    """Relay info message."""
    def __init__(self, relay_id='', listen_host='', listen_port=0, forward_host='', forward_port=0, active=False):
        self.relay_id = relay_id
        self.listen_host = listen_host
        self.listen_port = listen_port
        self.forward_host = forward_host
        self.forward_port = forward_port
        self.active = active

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        result = []
        if self.relay_id:
            result.append(_encode_tag(1, 2) + _encode_string(self.relay_id))
        if self.listen_host:
            result.append(_encode_tag(2, 2) + _encode_string(self.listen_host))
        if self.listen_port != 0:
            result.append(_encode_tag(3, 0) + _encode_varint(self.listen_port))
        if self.forward_host:
            result.append(_encode_tag(4, 2) + _encode_string(self.forward_host))
        if self.forward_port != 0:
            result.append(_encode_tag(5, 0) + _encode_varint(self.forward_port))
        if self.active:
            result.append(_encode_tag(6, 0) + _encode_varint(1 if self.active else 0))
        return b''.join(result)

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 2:
                msg.relay_id, pos = _decode_string(data, pos)
            elif tag_field == 2 and tag_wire == 2:
                msg.listen_host, pos = _decode_string(data, pos)
            elif tag_field == 3 and tag_wire == 0:
                msg.listen_port, pos = _decode_varint(data, pos)
            elif tag_field == 4 and tag_wire == 2:
                msg.forward_host, pos = _decode_string(data, pos)
            elif tag_field == 5 and tag_wire == 0:
                msg.forward_port, pos = _decode_varint(data, pos)
            elif tag_field == 6 and tag_wire == 0:
                active_val, pos = _decode_varint(data, pos)
                msg.active = active_val == 1
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg


# =============================================================================
# RelayListResponse message
# =============================================================================

class RelayListResponse:
    """Relay list response message."""
    def __init__(self, relays=None):
        self.relays = relays or []

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        result = []
        for relay in self.relays:
            relay_bytes = relay.SerializeToString() if isinstance(relay, RelayInfo) else RelayInfo(**relay).SerializeToString()
            result.append(_encode_tag(1, 2))
            result.append(_encode_varint(len(relay_bytes)))
            result.append(relay_bytes)
        return b''.join(result)

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                relay_data = data[pos:pos+length]
                pos += length
                msg.relays.append(RelayInfo.ParseFromString(relay_data))
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg


# =============================================================================
# WraithMessage message (top-level envelope)
# =============================================================================

class WraithMessage:
    """Top-level WraithMessage envelope."""
    def __init__(self, msg_type=0, message_id='', timestamp=0, registration=None, heartbeat=None,
                 command=None, result=None, relay_create=None, relay_delete=None,
                 relay_list=None, relay_list_response=None):
        self.msg_type = msg_type
        self.message_id = message_id
        self.timestamp = timestamp
        self._payload = None

        # Handle oneof payload - note: msg_type field is correctly named now
        if registration is not None:
            reg_bytes = registration.SerializeToString() if isinstance(registration, Registration) else registration
            self._payload = ('registration', reg_bytes)
        elif heartbeat is not None:
            hb_bytes = heartbeat.SerializeToString() if isinstance(heartbeat, Heartbeat) else heartbeat
            self._payload = ('heartbeat', hb_bytes)
        elif command is not None:
            cmd_bytes = command.SerializeToString() if isinstance(command, Command) else command
            self._payload = ('command', cmd_bytes)
        elif result is not None:
            res_bytes = result.SerializeToString() if isinstance(result, CommandResult) else result
            self._payload = ('result', res_bytes)
        elif relay_create is not None:
            rc_bytes = relay_create.SerializeToString() if isinstance(relay_create, RelayCreate) else relay_create
            self._payload = ('relay_create', rc_bytes)
        elif relay_delete is not None:
            rd_bytes = relay_delete.SerializeToString() if isinstance(relay_delete, RelayDelete) else relay_delete
            self._payload = ('relay_delete', rd_bytes)
        elif relay_list is not None:
            rl_bytes = relay_list.SerializeToString() if isinstance(relay_list, RelayList) else relay_list
            self._payload = ('relay_list', rl_bytes)
        elif relay_list_response is not None:
            rlr_bytes = relay_list_response.SerializeToString() if isinstance(relay_list_response, RelayListResponse) else relay_list_response
            self._payload = ('relay_list_response', rlr_bytes)

    @property
    def registration(self):
        if self._payload and self._payload[0] == 'registration':
            return Registration.ParseFromString(self._payload[1])
        return None

    @property
    def heartbeat(self):
        if self._payload and self._payload[0] == 'heartbeat':
            return Heartbeat.ParseFromString(self._payload[1])
        return None

    @property
    def command(self):
        if self._payload and self._payload[0] == 'command':
            return Command.ParseFromString(self._payload[1])
        return None

    @property
    def result(self):
        if self._payload and self._payload[0] == 'result':
            return CommandResult.ParseFromString(self._payload[1])
        return None

    @property
    def relay_create(self):
        if self._payload and self._payload[0] == 'relay_create':
            return RelayCreate.ParseFromString(self._payload[1])
        return None

    @property
    def relay_delete(self):
        if self._payload and self._payload[0] == 'relay_delete':
            return RelayDelete.ParseFromString(self._payload[1])
        return None

    @property
    def relay_list(self):
        if self._payload and self._payload[0] == 'relay_list':
            return RelayList.ParseFromString(self._payload[1])
        return None

    @property
    def relay_list_response(self):
        if self._payload and self._payload[0] == 'relay_list_response':
            return RelayListResponse.ParseFromString(self._payload[1])
        return None

    def SerializeToString(self):
        """Serialize to protobuf binary format."""
        result = []
        # msg_type field (field 1, varint) - now correctly using msg_type
        result.append(_encode_tag(1, 0) + _encode_varint(self.msg_type))
        # message_id field (field 2, length-delimited)
        if self.message_id:
            result.append(_encode_tag(2, 2) + _encode_string(self.message_id))
        # timestamp field (field 3, varint)
        if self.timestamp != 0:
            result.append(_encode_tag(3, 0) + _encode_varint(self.timestamp))
        # oneof payload (fields 4-11, length-delimited)
        if self._payload:
            payload_field_map = {
                'registration': 4,
                'heartbeat': 5,
                'command': 6,
                'result': 7,
                'relay_create': 8,
                'relay_delete': 9,
                'relay_list': 10,
                'relay_list_response': 11,
            }
            field_num = payload_field_map.get(self._payload[0])
            if field_num:
                payload_bytes = self._payload[1]
                if isinstance(payload_bytes, str):
                    payload_bytes = payload_bytes.encode()
                result.append(_encode_tag(field_num, 2) + _encode_varint(len(payload_bytes)) + payload_bytes)
        return b''.join(result)

    @classmethod
    def ParseFromString(cls, data):
        """Deserialize from protobuf binary format."""
        msg = cls()
        msg._payload = None
        pos = 0
        while pos < len(data):
            tag, wire_type = _decode_tag(data[pos:])
            pos += _varint_size(tag)
            tag_field = tag >> 3
            tag_wire = tag & 0x7
            if tag_field == 1 and tag_wire == 0:
                msg.msg_type, pos = _decode_varint(data, pos)
            elif tag_field == 2 and tag_wire == 2:
                msg.message_id, pos = _decode_string(data, pos)
            elif tag_field == 3 and tag_wire == 0:
                msg.timestamp, pos = _decode_varint(data, pos)
            elif tag_field == 4 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                payload = data[pos:pos+length]
                pos += length
                msg._payload = ('registration', payload)
            elif tag_field == 5 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                payload = data[pos:pos+length]
                pos += length
                msg._payload = ('heartbeat', payload)
            elif tag_field == 6 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                payload = data[pos:pos+length]
                pos += length
                msg._payload = ('command', payload)
            elif tag_field == 7 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                payload = data[pos:pos+length]
                pos += length
                msg._payload = ('result', payload)
            elif tag_field == 8 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                payload = data[pos:pos+length]
                pos += length
                msg._payload = ('relay_create', payload)
            elif tag_field == 9 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                payload = data[pos:pos+length]
                pos += length
                msg._payload = ('relay_delete', payload)
            elif tag_field == 10 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                payload = data[pos:pos+length]
                pos += length
                msg._payload = ('relay_list', payload)
            elif tag_field == 11 and tag_wire == 2:
                length, pos = _decode_varint(data, pos)
                payload = data[pos:pos+length]
                pos += length
                msg._payload = ('relay_list_response', payload)
            else:
                pos = _skip_field(data, pos, tag_wire)
        return msg