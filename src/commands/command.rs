use crate::proto::wraith::{Command as ProtoCommand, CommandResult};

pub trait Command: Send {
    fn execute(&self, cmd: &ProtoCommand) -> CommandResult;
}