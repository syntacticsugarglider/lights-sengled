use lights_sengled::SengledApi;
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
            .find(|device| device.name == "Guess")
            .unwrap();
        loop {
            api.set_color(&device, (255, 0, 0)).await.unwrap();
            smol::Timer::after(std::time::Duration::from_millis(200)).await;
            api.set_color(&device, (0, 0, 255)).await.unwrap();
            smol::Timer::after(std::time::Duration::from_millis(200)).await;
        }
    });
}
