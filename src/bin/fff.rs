use lifx_more::{Light, Message};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let lights = Light::enumerate_v4(2000).await.unwrap();
    let light = &lights[0];
    light.send(Message::SetMultiZoneEffect {
        instance_id: 0,
        ty: lifx_core::MultiZoneEffectType::Off,
        reserved6: 0,
        period: 0,
        duration: 0,
        reserved7: 0,
        parameters: [0; 8],
    }).await;
}
