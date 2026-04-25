use crate::proto::wraith::{
    CommandResult, Heartbeat, Registration, RelayCreate, RelayDelete,
    RelayList, RelayListResponse, RelayInfo, WraithMessage, MessageType,
};
use crate::relay::{RelayConfig, Transport};
use chrono::Utc;
use prost::Message;
use uuid::Uuid;

pub struct MessageCodec;

impl MessageCodec {
    pub fn encode(msg: &WraithMessage) -> Vec<u8> {
        msg.encode_to_vec()
    }

    pub fn decode(data: &[u8]) -> Result<WraithMessage, prost::DecodeError> {
        WraithMessage::decode(data)
    }

    pub fn create_message(msg_type: MessageType) -> WraithMessage {
        let mut msg = WraithMessage::default();
        msg.msg_type = msg_type as i32;
        msg.message_id = Uuid::new_v4().to_string();
        msg.timestamp = Utc::now().timestamp_millis();
        msg
    }

    pub fn create_registration(
        hostname: String,
        username: String,
        os: String,
        ip_address: String,
    ) -> WraithMessage {
        let mut reg = Registration::default();
        reg.hostname = hostname;
        reg.username = username;
        reg.os = os;
        reg.ip_address = ip_address;

        let mut msg = WraithMessage::default();
        msg.msg_type = MessageType::Registration as i32;
        msg.message_id = Uuid::new_v4().to_string();
        msg.timestamp = Utc::now().timestamp_millis();
        msg.payload = Some(crate::proto::wraith::wraith_message::Payload::Registration(reg));
        msg
    }

    pub fn create_heartbeat(last_command_time: i64, status: String) -> WraithMessage {
        let mut hb = Heartbeat::default();
        hb.last_command_time = last_command_time;
        hb.status = status;

        let mut msg = WraithMessage::default();
        msg.msg_type = MessageType::Heartbeat as i32;
        msg.message_id = Uuid::new_v4().to_string();
        msg.timestamp = Utc::now().timestamp_millis();
        msg.payload = Some(crate::proto::wraith::wraith_message::Payload::Heartbeat(hb));
        msg
    }

    pub fn create_command_result(
        command_id: String,
        status: String,
        output: String,
        exit_code: i32,
        duration_ms: i64,
        error: String,
    ) -> WraithMessage {
        let mut result = CommandResult::default();
        result.command_id = command_id;
        result.status = status;
        result.output = output;
        result.exit_code = exit_code;
        result.duration_ms = duration_ms;
        result.error = error;

        let mut msg = WraithMessage::default();
        msg.msg_type = MessageType::CommandResult as i32;
        msg.message_id = Uuid::new_v4().to_string();
        msg.timestamp = Utc::now().timestamp_millis();
        msg.payload = Some(crate::proto::wraith::wraith_message::Payload::Result(result));
        msg
    }

    pub fn create_relay_create(
        relay_id: String,
        config: RelayConfig,
    ) -> WraithMessage {
        let mut rc = RelayCreate::default();
        rc.relay_id = relay_id;
        rc.config = Some(crate::proto::wraith::RelayConfig {
            listen: Some(crate::proto::wraith::RelayEndpoint {
                host: config.listen.host,
                port: config.listen.port as i32,
                protocol: match config.listen.protocol {
                    Transport::Tcp => "tcp".to_string(),
                    Transport::Udp => "udp".to_string(),
                },
            }),
            forward: Some(crate::proto::wraith::RelayEndpoint {
                host: config.forward.host,
                port: config.forward.port as i32,
                protocol: match config.forward.protocol {
                    Transport::Tcp => "tcp".to_string(),
                    Transport::Udp => "udp".to_string(),
                },
            }),
        });

        let mut msg = WraithMessage::default();
        msg.msg_type = MessageType::RelayCreate as i32;
        msg.message_id = Uuid::new_v4().to_string();
        msg.timestamp = Utc::now().timestamp_millis();
        msg.payload = Some(crate::proto::wraith::wraith_message::Payload::RelayCreate(rc));
        msg
    }

    pub fn create_relay_delete(relay_id: String) -> WraithMessage {
        let mut rd = RelayDelete::default();
        rd.relay_id = relay_id;

        let mut msg = WraithMessage::default();
        msg.msg_type = MessageType::RelayDelete as i32;
        msg.message_id = Uuid::new_v4().to_string();
        msg.timestamp = Utc::now().timestamp_millis();
        msg.payload = Some(crate::proto::wraith::wraith_message::Payload::RelayDelete(rd));
        msg
    }

    pub fn create_relay_list() -> WraithMessage {
        let mut msg = WraithMessage::default();
        msg.msg_type = MessageType::RelayList as i32;
        msg.message_id = Uuid::new_v4().to_string();
        msg.timestamp = Utc::now().timestamp_millis();
        msg.payload = Some(crate::proto::wraith::wraith_message::Payload::RelayList(RelayList {}));
        msg
    }

    pub fn create_relay_list_response(relays: Vec<RelayInfo>) -> WraithMessage {
        let mut resp = RelayListResponse::default();
        resp.relays = relays;

        let mut msg = WraithMessage::default();
        msg.msg_type = MessageType::RelayListResponse as i32;
        msg.message_id = Uuid::new_v4().to_string();
        msg.timestamp = Utc::now().timestamp_millis();
        msg.payload = Some(crate::proto::wraith::wraith_message::Payload::RelayListResponse(resp));
        msg
    }
}