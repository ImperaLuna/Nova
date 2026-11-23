use super::Shared;
use nova_bluetooth_api::{BLUETOOTH_PIN_MAX_BYTES, PairingAnswer, PairingAsk};
use std::sync::Arc;
use std::time::Duration;
use zbus::interface;
use zbus::wire::OwnedObjectPath;

pub const AGENT_PATH: &str = "/org/nova/bluetooth/agent";
pub const CAPABILITY: &str = "KeyboardDisplay";
pub const ANSWER_TIMEOUT: Duration = if cfg!(test) { Duration::from_secs(3) } else { Duration::from_secs(60) };
const PASSKEY_DIGITS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Answered(PairingAnswer),
    TimedOut,
    Cancelled,
}

pub type Slot = nova_base::reply_slot::Slot<Reply>;

pub fn fits(ask: &PairingAsk, answer: &PairingAnswer) -> Result<(), String> {
    match (ask, answer) {
        (_, PairingAnswer::Reject) => Ok(()),
        (PairingAsk::Confirm { .. } | PairingAsk::Authorize, PairingAnswer::Accept) => Ok(()),
        (PairingAsk::EnterPin, PairingAnswer::Code(pin)) => valid_pin(pin)
            .then_some(())
            .ok_or_else(|| format!("a PIN is 1 to {BLUETOOTH_PIN_MAX_BYTES} letters, digits or symbols")),
        (PairingAsk::EnterPasskey, PairingAnswer::Code(code)) => {
            passkey(code).map(|_| ()).ok_or_else(|| format!("the code is up to {PASSKEY_DIGITS} digits"))
        }
        _ => Err("that isn't an answer to this question".into()),
    }
}

fn valid_pin(pin: &str) -> bool {
    !pin.is_empty() && pin.len() <= BLUETOOTH_PIN_MAX_BYTES && pin.chars().all(|c| c.is_ascii_graphic())
}

fn passkey(code: &str) -> Option<u32> {
    let code = code.trim();
    let digits = !code.is_empty() && code.len() <= PASSKEY_DIGITS && code.chars().all(|c| c.is_ascii_digit());
    digits.then(|| code.parse().ok()).flatten()
}

fn code(passkey: u32) -> String {
    format!("{passkey:0PASSKEY_DIGITS$}")
}

pub struct Agent {
    pub shared: Arc<Shared>,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
pub enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

impl Agent {
    async fn ask(&self, device: &OwnedObjectPath, ask: PairingAsk) -> Result<PairingAnswer, AgentError> {
        let Some((id, slot)) = self.shared.ask(device.as_str(), ask) else {
            return Err(AgentError::Rejected("not a device novad knows".into()));
        };
        let reply = slot.wait().await;
        self.shared.asked(id);
        match reply {
            Reply::Answered(PairingAnswer::Reject) => Err(AgentError::Rejected("turned down by the user".into())),
            Reply::Answered(answer) => Ok(answer),
            Reply::TimedOut => Err(AgentError::Canceled("nobody answered".into())),
            Reply::Cancelled => Err(AgentError::Canceled("cancelled".into())),
        }
    }

    async fn allow(&self, device: &OwnedObjectPath, ask: PairingAsk) -> Result<(), AgentError> {
        self.ask(device, ask).await.map(|_| ())
    }
}

#[interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {
        self.shared.logger.info(super::LOG_SOURCE, "BlueZ released the pairing agent");
    }

    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
        match self.ask(&device, PairingAsk::EnterPin).await? {
            PairingAnswer::Code(pin) => Ok(pin),
            _ => Err(AgentError::Rejected("no PIN".into())),
        }
    }

    fn display_pin_code(&self, device: OwnedObjectPath, pincode: String) {
        self.shared.show(device.as_str(), PairingAsk::ShowPin { pin: pincode });
    }

    async fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
        match self.ask(&device, PairingAsk::EnterPasskey).await? {
            PairingAnswer::Code(typed) => passkey(&typed).ok_or_else(|| AgentError::Rejected("no passkey".into())),
            _ => Err(AgentError::Rejected("no passkey".into())),
        }
    }

    fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, entered: u16) {
        let typed = u8::try_from(entered).unwrap_or(u8::MAX).min(PASSKEY_DIGITS as u8);
        self.shared.show(device.as_str(), PairingAsk::ShowPasskey { code: code(passkey), typed });
    }

    async fn request_confirmation(&self, device: OwnedObjectPath, passkey: u32) -> Result<(), AgentError> {
        self.allow(&device, PairingAsk::Confirm { code: code(passkey) }).await
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
        self.allow(&device, PairingAsk::Authorize).await
    }

    async fn authorize_service(&self, device: OwnedObjectPath, _uuid: String) -> Result<(), AgentError> {
        self.allow(&device, PairingAsk::Authorize).await
    }

    fn cancel(&self) {
        self.shared.cancel_pairing();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_must_fit_the_question() {
        let confirm = PairingAsk::Confirm { code: "012345".into() };
        let code = |text: &str| PairingAnswer::Code(text.into());
        assert!(fits(&confirm, &PairingAnswer::Accept).is_ok());
        assert!(fits(&confirm, &code("012345")).is_err());
        assert!(fits(&PairingAsk::EnterPin, &code("0000")).is_ok());
        assert!(fits(&PairingAsk::EnterPin, &code("")).is_err());
        assert!(fits(&PairingAsk::EnterPin, &code("has space")).is_err());
        assert!(fits(&PairingAsk::EnterPin, &code(&"9".repeat(17))).is_err());
        assert!(fits(&PairingAsk::EnterPin, &PairingAnswer::Accept).is_err());
        assert!(fits(&PairingAsk::EnterPasskey, &code("012345")).is_ok());
        assert!(fits(&PairingAsk::EnterPasskey, &code("1234567")).is_err());
        assert!(fits(&PairingAsk::EnterPasskey, &code("12a4")).is_err());
        let shown = PairingAsk::ShowPin { pin: "1234".into() };
        assert!(fits(&shown, &PairingAnswer::Accept).is_err(), "a code to type there needs no answer");
        assert!(fits(&shown, &PairingAnswer::Reject).is_ok());
    }

    #[test]
    fn passkeys_are_six_digits() {
        assert_eq!(code(42), "000042");
        assert_eq!(passkey("000042"), Some(42));
        assert_eq!(passkey(" 999999 "), Some(999_999));
    }

    #[test]
    fn a_slot_keeps_its_first_reply() {
        let slot = Slot::default();
        slot.resolve(Reply::TimedOut);
        slot.resolve(Reply::Answered(PairingAnswer::Accept));
        assert_eq!(zbus::block_on(slot.wait()), Reply::TimedOut);
    }
}
