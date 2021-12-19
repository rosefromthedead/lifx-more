use std::{collections::HashMap, sync::{Arc, Mutex, Weak, atomic::{AtomicU8, Ordering}}, time::Duration};

use displaydoc::Display;
use lifx_core::{BuildOptions, RawMessage, Service};
use tokio::{net::UdpSocket, sync::oneshot::{self, Sender}};

pub use lifx_core::{HSBK, Message};

#[cfg(feature = "effect")]
pub mod effect;

// beep
static SOURCE_ID: u32 = 7355608;

pub const OFF: HSBK = HSBK {
    hue: 0, saturation: 0, brightness: 0, kelvin: 4500,
};

#[derive(Debug, Display)]
pub enum Error {
    /// A packet was transmitted only partially - perhaps the MTU is too low
    IncompleteTransmission,
    /// A message was sent, and an unexpected reply was received
    WrongResponse,

    /// A lifx error occurred
    LifxError(lifx_core::Error),
    /// {0}
    IoError(tokio::io::Error),
}

impl std::error::Error for Error {}

impl From<lifx_core::Error> for Error {
    fn from(e: lifx_core::Error) -> Self {
        Error::LifxError(e)
    }
}

impl From<tokio::io::Error> for Error {
    fn from(e: tokio::io::Error) -> Self {
        Error::IoError(e)
    }
}

pub struct Light {
    pub id: u64,
    socket: UdpSocket,
    requests: Mutex<HashMap<u8, Sender<Option<Message>>>>,
    seq: AtomicU8,
}

impl Light {
    pub async fn enumerate_v4(timeout: u64) -> Result<Vec<Arc<Self>>, Error> {
        let options = BuildOptions {
            target: None,
            ack_required: false,
            res_required: true,
            sequence: 0,
            source: SOURCE_ID,
        };
        let msg_raw = RawMessage::build(&options, Message::GetService)?;
        let msg_bytes = msg_raw.pack()?;

        let socket = UdpSocket::bind("0.0.0.0:56700").await?;
        socket.set_broadcast(true)?;
        let bytes_sent = socket.send_to(&msg_bytes, "255.255.255.255:56700").await?;
        if bytes_sent != msg_bytes.len() {
            return Err(Error::IncompleteTransmission);
        }

        let mut lights = Vec::new();
        let handle_responses = async {
            loop {
                let mut buffer = vec![0; 128];
                let (_n_bytes, mut addr) = socket.recv_from(&mut buffer).await?;
                let response_raw = match RawMessage::unpack(&buffer) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let response = match Message::from_raw(&response_raw) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match response {
                    Message::StateService { port, service: Service::UDP } => {
                        dbg!(&addr);
                        addr.set_port(port as u16);
                        let socket = UdpSocket::bind("0.0.0.0:0").await?;
                        socket.connect(addr).await?;
                        let light = Arc::new(Light {
                            id: response_raw.frame_addr.target,
                            socket,
                            requests: Mutex::new(HashMap::new()),
                            seq: AtomicU8::new(0),
                        });
                        tokio::spawn(Self::handle(Arc::downgrade(&light)));
                        lights.push(light);
                    },
                    Message::GetService => continue,
                    _ => return Err(Error::WrongResponse),
                }
            }
            // type check moment
            #[allow(unreachable_code)]
            Ok(())
        };

        tokio::select! {
            res = handle_responses => {
                res?;
            },
            _ = tokio::time::sleep(Duration::from_millis(timeout)) => {},
        }

        Ok(lights)
    }

/*
    pub async fn enumerate_v6(timeout: u64) -> Result<Vec<Self>, Error> {
        let options = BuildOptions {
            target: None,
            ack_required: false,
            res_required: true,
            sequence: 0,
            source: SOURCE_ID,
        };
        let msg_raw = RawMessage::build(&options, Message::GetService)?;
        let msg_bytes = msg_raw.pack()?;

        let socket = UdpSocket::bind("[::]:56700").await?;
        socket.set_broadcast(true)?;
        let bytes_sent = socket.send_to(&msg_bytes, "[fe80::d273:d5ff:fe43:6be1]:56700").await?;
        if bytes_sent != msg_bytes.len() {
            return Err(Error::IncompleteTransmission);
        }

        let mut lights = Vec::new();
        let handle_responses = async {
            loop {
                let mut buffer = vec![0; 128];
                let (_n_bytes, mut addr) = socket.recv_from(&mut buffer).await?;
                let response_raw = match RawMessage::unpack(&buffer) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let response = match Message::from_raw(&response_raw) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match response {
                    Message::StateService { port, service: Service::UDP } => {
                        dbg!(&addr);
                        addr.set_port(port as u16);
                        let socket = UdpSocket::bind("[::]:0").await?;
                        socket.connect(addr).await?;
                        lights.push(Light {
                            id: response_raw.frame_addr.target,
                            socket,
                            requests: Mutex::new(HashMap::new()),
                            seq: AtomicU8::new(0),
                        });
                    },
                    Message::GetService => continue,
                    _ => return Err(Error::WrongResponse),
                }
            }
            // type check moment
            #[allow(unreachable_code)]
            Ok(())
        };

        tokio::select! {
            res = handle_responses => {
                res?;
            },
            _ = tokio::time::sleep(Duration::from_millis(timeout)) => {},
        }

        Ok(lights)
    }
*/
    pub async fn send(&self, msg: Message) -> Result<(), Error> {
        let sequence = self.seq.fetch_add(1, Ordering::Relaxed);
        let options = BuildOptions {
            target: Some(self.id),
            ack_required: true,
            res_required: false,
            sequence,
            source: SOURCE_ID,
        };
        let msg_raw = RawMessage::build(&options, msg)?;
        let msg_bytes = msg_raw.pack()?;

        let bytes_sent = self.socket.send(&msg_bytes).await?;
        if bytes_sent != msg_bytes.len() {
            return Err(Error::IncompleteTransmission);
        }

        let (tx, rx) = oneshot::channel();
        self.requests.lock().unwrap().insert(sequence, tx);
        let response = rx.await.unwrap();
        match response {
            Some(Message::Acknowledgement { seq }) if seq == sequence => Ok(()),
            Some(_) | None => Err(Error::WrongResponse),
        }
    }

    pub async fn request(&self, msg: Message) -> Result<Message, Error> {
        let sequence = self.seq.fetch_add(1, Ordering::Relaxed);
        let options = BuildOptions {
            target: Some(self.id),
            ack_required: false,
            res_required: true,
            sequence,
            source: SOURCE_ID,
        };
        let msg_raw = RawMessage::build(&options, msg)?;
        let msg_bytes = &msg_raw.pack()?;

        let bytes_sent = self.socket.send(&msg_bytes).await?;
        if bytes_sent != msg_bytes.len() {
            return Err(Error::IncompleteTransmission);
        }

        let (tx, rx) = oneshot::channel();
        self.requests.lock().unwrap().insert(sequence, tx);
        let response = rx.await.unwrap();
        match response {
            Some(v) => Ok(v),
            None => Err(Error::WrongResponse),
        }
    }

    async fn handle(this: Weak<Self>) -> Result<(), Error> {
        loop {
            let this = match this.upgrade() {
                Some(v) => v,
                None => break,
            };
            let mut buffer = vec![0; 1500];
            this.socket.recv(&mut buffer).await?;
            let msg_raw = RawMessage::unpack(&buffer)?;
            let tx = match this.requests.lock().unwrap().remove(&msg_raw.frame_addr.sequence) {
                Some(v) => v,
                None => {
                    eprintln!("A reply was received unexpectedly");
                    continue;
                },
            };
            let _ = tx.send(Some(Message::from_raw(&msg_raw)?));
        }
        Ok(())
    }

    #[cfg(feature = "effect")]
    pub async fn apply(&self, effect: &effect::Effect, transition_ms: u32) -> Result<(), Error> {
        use effect::Effect;
        use lifx_core::ApplicationRequest;

        match effect {
            Effect::SolidColour(colour) => {
                self.send(Message::LightSetColor {
                    reserved: 0,
                    color: (*colour).into(),  // three monkeys on a stick
                    duration: transition_ms,
                }).await?;
            },
            Effect::MultiColour { colours, scale_factor } => {
                for (i, colour) in colours.iter().enumerate() {
                    let i = i * *scale_factor as usize;
                    self.send(Message::SetColorZones {
                        start_index: i as u8,
                        end_index: i as u8 + scale_factor,
                        color: colour.map(Into::into).unwrap_or(OFF),
                        duration: transition_ms,
                        apply: ApplicationRequest::NoApply,
                    }).await?;
                }
                self.send(Message::SetColorZones {
                    start_index: 0,
                    end_index: 0,
                    color: OFF,
                    duration: transition_ms,
                    apply: ApplicationRequest::ApplyOnly,
                }).await?;
            },

        }
        tokio::time::sleep(Duration::from_millis(transition_ms as u64)).await;
        Ok(())
    }

    #[cfg(feature = "effect")]
    pub async fn run_sequence(&self, sequence: &effect::Sequence) -> Result<(), Error> {
        use effect::Operation;

        for operation in sequence.ops.iter() {
            match operation {
                Operation::Transition { to, transition_ms } => {
                    let effect = sequence.effects.get(to).unwrap();
                    self.apply(effect, *transition_ms).await?;
                },
                Operation::DelayMs(ms) => {
                    tokio::time::sleep(Duration::from_millis(*ms)).await;
                },
                Operation::Rotate { period, duration_ns } => {
                    let duration = duration_ns.unwrap_or(u64::MAX / 2);
                    self.send(Message::SetMultiZoneEffect {
                        instance_id: 1,
                        ty: lifx_core::MultiZoneEffectType::Move,
                        reserved6: 0,
                        period: *period,
                        duration,
                        reserved7: 0,
                        parameters: [0, 1, 0, 0, 0, 0, 0, 0],
                    }).await?;
                    tokio::time::sleep(Duration::from_nanos(duration)).await;
                },
            }
        }
        Ok(())
    }
}
