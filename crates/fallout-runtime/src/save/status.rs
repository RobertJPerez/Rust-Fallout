//! The host observes one existing save ticket. A terminal result stays visible
//! until the host releases this adapter; observing it cannot publish a save.
use super::{CompletionError, SaveTicket, WriteReceipt};

#[derive(Debug)]
pub enum SaveState {
    Pending,
    Published(WriteReceipt),
    Failed(CompletionError),
}

#[derive(Debug)]
#[must_use = "observe the terminal save status before releasing it"]
pub struct SaveStatus {
    ticket: Option<SaveTicket>,
    state: SaveState,
}

impl SaveStatus {
    pub fn new(ticket: SaveTicket) -> Self {
        Self {
            ticket: Some(ticket),
            state: SaveState::Pending,
        }
    }

    /// Read the last observed result without consuming it or contacting the
    /// writer. Admission and a joined worker do not imply publication.
    pub fn state(&self) -> &SaveState {
        &self.state
    }

    /// Nonblocking collection for a host frame. Success requires this request's
    /// actual publication receipt. Failures, including a stopped worker or an
    /// already-consumed ticket, stay failures on every subsequent observation.
    pub fn poll(&mut self) -> &SaveState {
        if let Some(ticket) = &mut self.ticket {
            match ticket.try_wait() {
                Ok(None) => {}
                Ok(Some(receipt)) => {
                    self.state = SaveState::Published(receipt);
                    self.ticket = None;
                }
                Err(error) => {
                    self.state = SaveState::Failed(error);
                    self.ticket = None;
                }
            }
        }
        &self.state
    }

    /// Blocking collection outside the frame loop, for shutdown and offline
    /// consumers. This is the existing ticket wait, not another writer.
    pub fn wait(self) -> Result<WriteReceipt, CompletionError> {
        match self.state {
            SaveState::Pending => self.ticket.expect("pending status owns its ticket").wait(),
            SaveState::Published(receipt) => Ok(receipt),
            SaveState::Failed(error) => Err(error),
        }
    }
}
