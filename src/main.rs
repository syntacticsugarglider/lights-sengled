use lights_sengled::{Color, SengledApi};
use smol::block_on;

fn main() {
    block_on(async move {
        let api = SengledApi::new(
            std::env::var("SENGLED_USER").unwrap(),
            std::env::var("SENGLED_PASS").unwrap(),
        )
        .await
        .unwrap();
        let devices = api.get_devices().await.unwrap();
        let device = devices
            .into_iter()
            .find(|device| device.name == "Sparkle")
            .unwrap();
        loop {
            api.set_color(&device, Color::White { temperature: 2700 })
                .await
                .unwrap();
            smol::Timer::after(std::time::Duration::from_millis(200)).await;
            api.set_color(&device, Color::White { temperature: 6500 })
                .await
                .unwrap();
            smol::Timer::after(std::time::Duration::from_millis(200)).await;
        }
    });
}
