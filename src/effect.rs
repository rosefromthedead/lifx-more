use std::collections::HashMap;

use lifx_core::HSBK;
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
pub struct ColourSpec(u16, u16, u16, u16);

impl From<ColourSpec> for HSBK {
    fn from(x: ColourSpec) -> Self {
        HSBK {
            hue: x.0,
            saturation: x.1,
            brightness: x.2,
            kelvin: x.3,
        }
    }
}

#[derive(Deserialize)]
pub enum Effect {
    SolidColour(ColourSpec),
    MultiColour {
        colours: Vec<Option<ColourSpec>>,
        scale_factor: u8,
    },
}

#[derive(Deserialize)]
pub enum Operation {
    Transition {
        to: String,
        #[serde(default)]
        transition_ms: u32,
    },
    DelayMs(u64),
    Rotate {
        period: u32,
        duration_ns: Option<u64>,
    },
}

#[derive(Deserialize)]
pub struct Sequence {
    pub effects: HashMap<String, Effect>,
    pub ops: Vec<Operation>,
}
