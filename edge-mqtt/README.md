# edge-mqtt

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

A wrapper for the [`rumqttc`]() crate that adapts it to async [MQTT traits]() of the `embedded-svc` crate.

**NOTE**: Needs STD!

The plan for the future is to retire this crate in favor of []() once the latter gets MQTT 3.1 compatibility, and implements a more ergonomic API where sending can be done independently from receiving MQTT messages.

... or implement a true `no_std` no-alloc alternative - just like all other `edge-*` crates - if "" does not see further development.

## Example

```rust
use async_compat::CompatExt;

use embedded_svc::mqtt::client::asynch::{Client, Connection, Publish, QoS};
use embedded_svc::mqtt::client::Event;

use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};

use edge_mqtt::io::{AsyncClient, MqttClient, MqttConnection, MqttOptions};

use log::*;

const MQTT_HOST: &str = "broker.emqx.io";
const MQTT_PORT: u16 = 1883;
const MQTT_CLIENT_ID: &str = "edge-mqtt-demo";
const MQTT_TOPIC: &str = "edge-mqtt-demo";

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let (client, conn) = mqtt_create(MQTT_CLIENT_ID, MQTT_HOST, MQTT_PORT).unwrap();

    futures_lite::future::block_on(
        run(client, conn, MQTT_TOPIC).compat(), /* necessary for tokio */
    )
    .unwrap()
}

async fn run<M, C>(mut client: M, mut connection: C, topic: &str) -> Result<(), anyhow::Error>
where
    M: Client + Publish + 'static,
    M::Error: std::error::Error + Send + Sync + 'static,
    C: Connection + 'static,
{
    info!("About to start the MQTT client");

    info!("MQTT client started");

    client.subscribe(topic, QoS::AtMostOnce).await?;

    info!("Subscribed to topic \"{topic}\"");

    let res = select(
        async move {
            info!("MQTT Listening for messages");

            while let Ok(event) = connection.next().await {
                info!("[Queue] Event: {}", event.payload());
            }

            info!("Connection closed");

            Ok(())
        },
        async move {
            // Just to give a chance of our connection to get even the first published message
            Timer::after(Duration::from_millis(500)).await;

            let payload = "Hello from edge-mqtt-demo!";

            loop {
                client
                    .publish(topic, QoS::AtMostOnce, false, payload.as_bytes())
                    .await?;

                info!("Published \"{payload}\" to topic \"{topic}\"");

                let sleep_secs = 2;

                info!("Now sleeping for {sleep_secs}s...");
                Timer::after(Duration::from_secs(sleep_secs)).await;
            }
        },
    )
    .await;

    match res {
        Either::First(res) => res,
        Either::Second(res) => res,
    }
}

fn mqtt_create(
    client_id: &str,
    host: &str,
    port: u16,
) -> Result<(MqttClient, MqttConnection), anyhow::Error> {
    let mut mqtt_options = MqttOptions::new(client_id, host, port);

    mqtt_options.set_keep_alive(core::time::Duration::from_secs(10));

    let (rumqttc_client, rumqttc_eventloop) = AsyncClient::new(mqtt_options, 10);

    let mqtt_client = MqttClient::new(rumqttc_client);
    let mqtt_conn = MqttConnection::new(rumqttc_eventloop);

    Ok((mqtt_client, mqtt_conn))
}
```
