pub use rumqttc::*;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use embedded_svc::mqtt::client::asynch::{
        Client, Connection, Details, ErrorType, Event, EventPayload, MessageId, Publish, QoS,
    };

    use log::trace;

    use rumqttc::{self, AsyncClient, EventLoop, PubAck, SubAck, UnsubAck};

    pub use rumqttc::{ClientError, ConnectionError, RecvError};

    pub struct MqttClient(AsyncClient);

    impl MqttClient {
        pub const fn new(client: AsyncClient) -> Self {
            Self(client)
        }
    }

    impl ErrorType for MqttClient {
        type Error = ClientError;
    }

    impl Client for MqttClient {
        async fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<MessageId, Self::Error> {
            self.0.subscribe(topic, to_qos(qos)).await?;

            Ok(0)
        }

        async fn unsubscribe(&mut self, topic: &str) -> Result<MessageId, Self::Error> {
            self.0.unsubscribe(topic).await?;

            Ok(0)
        }
    }

    impl Publish for MqttClient {
        async fn publish(
            &mut self,
            topic: &str,
            qos: embedded_svc::mqtt::client::QoS,
            retain: bool,
            payload: &[u8],
        ) -> Result<MessageId, Self::Error> {
            self.0.publish(topic, to_qos(qos), retain, payload).await?;

            Ok(0)
        }
    }

    pub struct MqttEvent(Result<rumqttc::Event, ConnectionError>);

    impl MqttEvent {
        fn payload(&self) -> EventPayload<'_, ConnectionError> {
            self.maybe_payload().unwrap()
        }

        fn maybe_payload(&self) -> Option<EventPayload<'_, ConnectionError>> {
            match &self.0 {
                Ok(event) => match event {
                    rumqttc::Event::Incoming(incoming) => match incoming {
                        rumqttc::Packet::Connect(_) => Some(EventPayload::BeforeConnect),
                        rumqttc::Packet::ConnAck(_) => Some(EventPayload::Connected(true)),
                        rumqttc::Packet::Disconnect => Some(EventPayload::Disconnected),
                        rumqttc::Packet::PubAck(PubAck { pkid, .. }) => {
                            Some(EventPayload::Published(*pkid as _))
                        }
                        rumqttc::Packet::SubAck(SubAck { pkid, .. }) => {
                            Some(EventPayload::Subscribed(*pkid as _))
                        }
                        rumqttc::Packet::UnsubAck(UnsubAck { pkid, .. }) => {
                            Some(EventPayload::Unsubscribed(*pkid as _))
                        }
                        rumqttc::Packet::Publish(rumqttc::Publish {
                            pkid,
                            topic,
                            payload,
                            ..
                        }) => Some(EventPayload::Received {
                            id: *pkid as _,
                            topic: Some(topic.as_str()),
                            data: payload,
                            details: Details::Complete,
                        }),
                        _ => None,
                    },
                    rumqttc::Event::Outgoing(_) => None,
                },
                Err(err) => Some(EventPayload::Error(err)),
            }
        }
    }

    impl ErrorType for MqttEvent {
        type Error = ConnectionError;
    }

    impl Event for MqttEvent {
        fn payload(&self) -> EventPayload<'_, Self::Error> {
            MqttEvent::payload(self)
        }
    }

    pub struct MqttConnection(EventLoop, bool);

    impl MqttConnection {
        pub const fn new(event_loop: EventLoop) -> Self {
            Self(event_loop, false)
        }
    }

    impl ErrorType for MqttConnection {
        type Error = RecvError;
    }

    impl Connection for MqttConnection {
        type Event<'a> = MqttEvent where Self: 'a;

        async fn next(&mut self) -> Result<Self::Event<'_>, Self::Error> {
            if self.1 {
                Err(RecvError)
            } else {
                loop {
                    let event = self.0.poll().await;
                    trace!("Got event: {:?}", event);

                    let event = MqttEvent(event);
                    if let Some(payload) = event.maybe_payload() {
                        if matches!(payload, EventPayload::Error(ConnectionError::RequestsDone)) {
                            self.1 = true;
                            trace!("Done with requests");
                            break Err(RecvError);
                        } else {
                            break Ok(event);
                        }
                    }
                }
            }
        }
    }

    fn to_qos(qos: QoS) -> rumqttc::QoS {
        match qos {
            QoS::AtMostOnce => rumqttc::QoS::AtMostOnce,
            QoS::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
            QoS::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
        }
    }
}
