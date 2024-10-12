use std::time::{self, SystemTime};

use anyhow::Result;
use chrono::{DateTime, Utc};
use embedded_hal::blocking::delay::DelayMs;
use embedded_svc::mqtt::client::{EventPayload::Error, EventPayload::Received, QoS};
use esp32c3_wifi::wifi;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::FreeRtos,
        i2c::{I2cConfig, I2cDriver},
        peripherals::Peripherals,
        prelude::*,
    },
    mqtt::client::{EspMqttClient, MqttClientConfiguration, MqttProtocolVersion},
    sntp::{EspSntp, SyncStatus},
    wifi::AuthMethod,
};
use hex::encode;
use log::{error, info, warn};
use md5::Md5;
use serde::{Deserialize, Serialize};
use shtcx::{self, PowerMode};

#[derive(Debug)]
#[toml_cfg::toml_config]
pub struct Config {
    #[default("localhost")]
    host: &'static str,
    #[default("")]
    product: &'static str,
    #[default("")]
    client_id: &'static str,
    #[default("")]
    product_id: &'static str,
    #[default("")]
    key: &'static str,
    #[default("")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
}

use hmac::{Hmac, Mac};

type HmacMd5 = Hmac<Md5>;

pub fn hmac_md5(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = <HmacMd5 as Mac>::new_from_slice(key).expect("Invalid key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

#[derive(Serialize, Deserialize, Debug)]
struct MyData {
    id: String,
    version: String,
    sys: Sys,
    params: Params,
}

#[derive(Serialize, Deserialize, Debug)]
struct Sys {
    ack: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct Params {
    CurrentTemperature: Param,
    CurrentHumidity: Param,
}

#[derive(Serialize, Deserialize, Debug)]
struct Param {
    value: f32,
    time: i64,
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let sysloop = EspSystemEventLoop::take()?;

    let app_config = CONFIG;
    info!("App Config: {:?}", app_config);

    let auth_method = AuthMethod::WPA2Personal;
    let _wifi = match wifi(
        app_config.wifi_ssid,
        app_config.wifi_psk,
        auth_method,
        peripherals.modem,
        sysloop,
    ) {
        Ok(_conn) => _conn,
        Err(_err) => {
            error!("WiFi  连接失败{_err}");
            return Ok(());
        }
    };

    esp_idf_svc::hal::task::block_on(async {
        info!("请求SNTP时间同步");
        let ntp = EspSntp::new_default().unwrap();
        while ntp.get_sync_status() != SyncStatus::Completed {}
        info!("时间同步完成");
    });

    let sda = peripherals.pins.gpio10;
    let scl = peripherals.pins.gpio8;
    let config = I2cConfig::new().baudrate(400.kHz().into());
    let i2c = I2cDriver::new(peripherals.i2c0, sda, scl, &config)?;
    let mut sht = shtcx::shtc3(i2c);
    let device_id = sht.device_identifier().unwrap();
    info!("Device ID SHTC3: {:#02x}", device_id);

    let user_name = format!("{}&{}", app_config.client_id, app_config.product);
    let password = encode(hmac_md5(app_config.key.as_bytes(), user_name.as_bytes()));
    let mqtt_config = MqttClientConfiguration {
        // 配置项，如用户名、密码、客户端ID等
        client_id: Some(app_config.client_id),
        username: Some(&user_name),
        password: Some(&password),
        protocol_version: Some(MqttProtocolVersion::V3_1_1),
        network_timeout: time::Duration::from_secs(10),
        keep_alive_interval: Some(time::Duration::from_secs(60)),
        ..Default::default()
    };
    info!("MQTT 配置: {:#?}", mqtt_config);
    let broker_url = format!("mqtt://{}", app_config.host);
    info!("MQTT Broker: {:#?}", broker_url); //192.168.31.76
    let mut client =
        EspMqttClient::new_cb(
            &broker_url,
            &mqtt_config,
            |message_event| match message_event.payload() {
                Received { data, details, .. } => {
                    info!("Received from MQTT: {:?}", data);
                    info!("Received from MQTT: {:?}", details);
                }
                Error(e) => warn!("Received error from MQTT: {:?}", e),
                _ => info!("Received from MQTT: {:?}", message_event.payload()),
            },
        )?;
    let post_reply = format!(
        "/sys/{}/{}/thing/property/post_reply",
        app_config.client_id, app_config.product_id
    );
    info!(" MQTT 属性响应订阅: {}", post_reply);
    if let Err(err) = client.subscribe(&post_reply, QoS::AtLeastOnce) {
        error!("MQTT 属性响应订阅失败{err}");
    };
    loop {
        sht.start_measurement(PowerMode::NormalMode).unwrap();
        FreeRtos.delay_ms(100u32);
        let measurement = sht.get_measurement_result().unwrap();

        info!(
            "TEMP: {:.2} °C | HUM: {:.2} %",
            measurement.temperature.as_degrees_celsius(),
            measurement.humidity.as_percent(),
        );
        let st_now = SystemTime::now();
        // Convert to UTC Time
        let dt_now_utc: DateTime<Utc> = st_now.clone().into();
        let paylod = MyData {
            id: format!("{}-{}", device_id, dt_now_utc.timestamp_millis()),
            version: "1.0".to_string(),
            sys: Sys { ack: true },
            params: Params {
                CurrentTemperature: Param {
                    value: measurement.temperature.as_degrees_celsius(),
                    time: dt_now_utc.timestamp_millis() as i64,
                },
                CurrentHumidity: Param {
                    value: measurement.humidity.as_percent(),
                    time: dt_now_utc.timestamp_millis() as i64,
                },
            },
        };
        let payload = serde_json::to_string(&paylod).unwrap();
        let topic = format!(
            "/sys/{}/{}/thing/property/post",
            app_config.client_id, app_config.product_id
        );
        client.publish(&topic, QoS::AtMostOnce, false, payload.as_bytes())?;
        FreeRtos.delay_ms(500u32);
    }
}
