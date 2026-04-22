use crate::proto::wraith::WraithMessage;
use std::io::Result;

pub trait Connection: Send {
    fn init(&mut self) -> Result<()>;

    fn connect(&mut self) -> Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "connect not implemented",
        ))
    }

    fn listen(&mut self) -> Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "listen not implemented",
        ))
    }

    fn close(&mut self) -> Result<()>;

    fn is_connected(&self) -> bool;

    async fn send_message(&mut self, msg: &WraithMessage) -> Result<()>;
    async fn read_message(&mut self) -> Result<WraithMessage>;
}