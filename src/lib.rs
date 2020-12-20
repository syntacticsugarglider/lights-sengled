use paho_mqtt as mqtt;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{convert::TryInto, fmt::Display};
use surf::Body;
use thiserror::Error;

struct SengledOsType;

impl Serialize for SengledOsType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        "ios".serialize(serializer)
    }
}

struct SengledUuid;

impl Serialize for SengledUuid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        "xxx".serialize(serializer)
    }
}

struct SengledProductCode;

impl Serialize for SengledProductCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        "life".serialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SengledLoginRequest {
    user: String,
    pwd: String,
    os_type: SengledOsType,
    uuid: SengledUuid,
    product_code: SengledProductCode,
    app_code: SengledProductCode,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("http error occurred: {0}")]
    Http(surf::Error),
    #[error("authentication failed")]
    AuthenticationFailure,
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("mqtt error: {0}")]
    Mqtt(#[from] mqtt::Error),
}

impl From<surf::Error> for Error {
    fn from(e: surf::Error) -> Self {
        Error::Http(e)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LoginResponse {
    Failure,
    Success {
        #[serde(rename = "jsessionId")]
        session_id: String,
    },
}

#[derive(Debug)]
pub struct Device {
    pub name: String,
    uuid: Mac,
}

impl Device {
    pub fn uuid(&self) -> [u8; 6] {
        self.uuid.0
    }
}

#[derive(Debug, Clone)]
struct Mac([u8; 6]);

impl Display for Mac {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.0
                .iter()
                .map(|byte| format!("{:X}", byte))
                .collect::<Vec<_>>()
                .join(":")
        )
    }
}

#[derive(Deserialize, Debug)]
struct Attribute {
    name: String,
    value: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RawDeviceResponse {
    device_uuid: String,
    attribute_list: Vec<Attribute>,
}

impl<'de> Deserialize<'de> for Device {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawDeviceResponse::deserialize(deserializer)?;
        let name = raw
            .attribute_list
            .into_iter()
            .find(|item| item.name == "name")
            .ok_or(serde::de::Error::custom("no name field in attributes"))?
            .value;

        Ok(Device {
            name,
            uuid: Mac(raw
                .device_uuid
                .split(':')
                .map(|item| u8::from_str_radix(item, 16))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| serde::de::Error::custom(format!("invalid UUID: {}", e)))?
                .as_slice()
                .try_into()
                .map_err(|e| serde::de::Error::custom(format!("invalid UUID: {}", e)))?),
        })
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DevicesResponse {
    device_list: Vec<Device>,
}

pub struct SengledApi {
    session_id: String,
    client: mqtt::AsyncClient,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum CommandType {
    Switch,
    Brightness,
    Color,
}

#[derive(Serialize)]
pub struct Command {
    #[serde(rename = "type")]
    ty: CommandType,
    dn: Mac,
    value: String,
    time: CurrentTime,
}

impl Serialize for Mac {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        format!("{}", self).serialize(serializer)
    }
}

pub struct CurrentTime;

impl Serialize for CurrentTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis()
            .serialize(serializer)
    }
}

impl SengledApi {
    pub async fn new<T: AsRef<str>, U: AsRef<str>>(user: T, pass: U) -> Result<Self, Error> {
        match surf::post("https://ucenter.cloud.sengled.com/user/app/customer/v2/AuthenCross.json")
            .body(Body::from_json(&SengledLoginRequest {
                user: user.as_ref().into(),
                pwd: pass.as_ref().into(),
                os_type: SengledOsType,
                product_code: SengledProductCode,
                app_code: SengledProductCode,
                uuid: SengledUuid,
            })?)
            .recv_json()
            .await?
        {
            LoginResponse::Success { session_id } => {
                let client = mqtt::CreateOptionsBuilder::new()
                    .client_id(format!("{}@lifeApp", session_id))
                    .persistence(mqtt::PersistenceType::None)
                    .server_uri("wss://us-mqtt.cloud.sengled.com:443/mqtt")
                    .create_client()
                    .unwrap();
                client
                    .connect(
                        mqtt::ConnectOptionsBuilder::new()
                            .http_headers(&[
                                ("Cookie", format!("JSESSIONID={}", session_id).as_str()),
                                ("X-Requested-With", "com.sengled.life2"),
                            ])
                            .ssl_options(mqtt::SslOptionsBuilder::new().finalize())
                            .finalize(),
                    )
                    .await
                    .unwrap();
                Ok(SengledApi { session_id, client })
            }
            _ => Err(Error::AuthenticationFailure),
        }
    }
    async fn request<S: Serialize, T>(&self, uri: &str, data: Option<&S>) -> Result<T, surf::Error>
    where
        for<'de> T: Deserialize<'de>,
    {
        let mut request = surf::post(uri);
        if let Some(data) = data {
            request = request.body(Body::from_json(data)?);
        }
        request = request.header("Cookie", format!("JSESSIONID={}", self.session_id));
        request.recv_json().await
    }
    async fn send_command(&self, command: &Command) -> Result<(), Error> {
        self.client
            .publish(
                mqtt::MessageBuilder::new()
                    .topic(format!("wifielement/{}/update", command.dn))
                    .payload(serde_json::to_string(command)?)
                    .finalize(),
            )
            .await?;
        Ok(())
    }
    pub async fn get_devices(&self) -> Result<Vec<Device>, Error> {
        let resp: DevicesResponse = self
            .request(
                "https://life2.cloud.sengled.com/life2/device/list.json",
                None::<&()>,
            )
            .await?;
        Ok(resp.device_list)
    }
    pub async fn turn_on(&self, device: &Device) -> Result<(), Error> {
        self.send_command(&Command {
            dn: device.uuid.clone(),
            ty: CommandType::Switch,
            value: "1".into(),
            time: CurrentTime,
        })
        .await
    }
    pub async fn turn_off(&self, device: &Device) -> Result<(), Error> {
        self.send_command(&Command {
            dn: device.uuid.clone(),
            ty: CommandType::Switch,
            value: "0".into(),
            time: CurrentTime,
        })
        .await
    }
    pub async fn set_brightness(&self, device: &Device, brightness: u8) -> Result<(), Error> {
        self.send_command(&Command {
            dn: device.uuid.clone(),
            ty: CommandType::Brightness,
            value: format!("{}", ((brightness as f32 / 255.) * 100.) as u8),
            time: CurrentTime,
        })
        .await
    }
    pub async fn set_color(&self, device: &Device, color: (u8, u8, u8)) -> Result<(), Error> {
        self.send_command(&Command {
            dn: device.uuid.clone(),
            ty: CommandType::Color,
            value: format!("{}:{}:{}", color.0, color.1, color.2),
            time: CurrentTime,
        })
        .await
    }
}
