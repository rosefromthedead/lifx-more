use lifx_more::Message;
use lifx_more::Light;

use std::fs::read_to_string;

#[cfg(feature = "effect")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap();
    let sequence_string = read_to_string(path)?;
    let sequence: lifx_more::effect::Sequence = ron::from_str(&sequence_string)?;

    let lights = Light::enumerate_v4(2000).await?;
    lights[0].run_sequence(&sequence).await?;

    Ok(())
}

#[cfg(not(feature = "effect"))]
fn main() {
    panic!("this example requires the 'effect' feature");
}
